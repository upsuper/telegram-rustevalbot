use super::parse::Flags;
use crate::eval::parse::{get_help_message, Channel, Mode};
use crate::utils;
use futures::{future, FutureExt as _};
use htmlescape::{encode_attribute, encode_minimal};
use log::{debug, warn};
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write;
use std::future::Future;

pub fn execute<'p>(
    client: &'p Client,
    content: &'p str,
    flags: Flags,
    is_private: bool,
) -> Option<impl Future<Output = Result<String, reqwest::Error>> + 'p> {
    Some(if flags.help {
        future::ok(get_help_message()).left_future().left_future()
    } else if flags.version {
        let channel = flags.channel;
        get_version(client, channel).right_future().left_future()
    } else if !content.trim().is_empty() {
        run_code(client, content, flags, is_private).right_future()
    } else {
        return None;
    })
}

async fn get_version(client: &Client, channel: Option<Channel>) -> Result<String, reqwest::Error> {
    let url = format!(
        "https://play.rust-lang.org/meta/version/{}",
        channel.unwrap_or(Channel::Stable).as_str(),
    );
    let resp = client.get(&url).send().await?;
    let v: Version = resp.error_for_status()?.json().await?;
    Ok(format!("rustc {} ({:.9} {})", v.version, v.hash, v.date))
}

#[derive(Deserialize)]
struct Version {
    date: String,
    hash: String,
    version: String,
}

async fn run_code(
    client: &Client,
    code: &str,
    flags: Flags,
    is_private: bool,
) -> Result<String, reqwest::Error> {
    let code = generate_code_to_send(code, flags.bare);
    let channel = flags.channel.unwrap_or(Channel::Stable);
    let req = Request {
        channel,
        edition: flags.edition.unwrap_or("2018"),
        mode: flags.mode.unwrap_or(Mode::Debug),
        crate_type: CrateType::Bin,
        tests: false,
        backtrace: false,
        code,
    };
    const URL: &str = "https://play.rust-lang.org/execute";
    let resp = client.post(URL).json(&req).send().await?;
    let resp = resp.error_for_status()?.json().await?;
    Ok(generate_result_from_response(resp, channel, is_private))
}

fn generate_code_to_send(code: &str, bare: bool) -> String {
    if bare || code.find("fn main()").is_some() {
        return code.to_string();
    }
    macro_rules! template {
        ($($line:expr,)+) => {
            concat!($($line, '\n',)+)
        }
    }
    let (header, body) = extract_code_headers(code);
    debug!("extract: {:?} -> ({:?}, {:?})", code, header, body);
    let code = if body.contains("println!") || body.contains("print!") {
        Cow::from(body)
    } else {
        Cow::from(format!(
            template! {
                // Template below would provide the indent of this line.
                "println!(\"{{:?}}\", {{",
                "        {code}",
                "    }})",
            },
            code = body
        ))
    };
    format!(
        template! {
            "#![allow(unreachable_code)]",
            "{header}",
            "{preludes}",
            "fn main() -> Result<(), Box<dyn std::error::Error>> {{",
            "    {code};",
            "    Ok(())",
            "}}",
        },
        header = header,
        preludes = &*PRELUDES,
        code = code,
    )
}

fn generate_result_from_response(resp: Response, channel: Channel, is_private: bool) -> String {
    if resp.success {
        let output = resp.stdout.trim();
        let output = if is_private {
            output.into()
        } else {
            const MAX_LINES: usize = 3;
            const MAX_TOTAL_COLUMNS: usize = MAX_LINES * 72;
            utils::truncate_output(output, MAX_LINES, MAX_TOTAL_COLUMNS)
        };
        if output.is_empty() {
            return "(no output)".to_string();
        }
        return format!("<pre>{}</pre>", encode_minimal(&output));
    }

    static RE_ERROR: Lazy<Regex> = Lazy::new(|| Regex::new(r"^error\[(E\d{4})\]:").unwrap());
    static RE_CODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"`(.+?)`").unwrap());
    static RE_ISSUE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\(see issue #(\d+)\)").unwrap());
    let mut return_line: Option<&str> = None;
    for line in resp.stderr.split('\n') {
        let line = line.trim();
        if line.starts_with("Compiling")
            || line.starts_with("Finished")
            || line.starts_with("Running")
            || line.is_empty()
        {
            continue;
        }
        if line.starts_with("error") {
            return_line = Some(line);
            break;
        }
        if return_line.is_none() {
            return_line = Some(line);
        }
    }
    if let Some(line) = return_line {
        let line = encode_minimal(line);
        let line = RE_ERROR.replacen(&line, 1, |captures: &Captures<'_>| {
            let err_num = captures.get(1).unwrap().as_str();
            let url = format!(
                "https://doc.rust-lang.org/{}/error-index.html#{}",
                channel.as_str(),
                err_num,
            );
            format!(
                r#"error<a href="{}">[{}]</a>:"#,
                encode_attribute(&url),
                err_num,
            )
        });
        let line = RE_CODE.replace_all(&line, |captures: &Captures<'_>| {
            format!("<code>{}</code>", captures.get(1).unwrap().as_str())
        });
        let line = RE_ISSUE.replacen(&line, 1, |captures: &Captures<'_>| {
            let issue_num = captures.get(1).unwrap().as_str();
            let url = format!("https://github.com/rust-lang/rust/issues/{}", issue_num);
            format!(r#"(see issue <a href="{}">#{}</a>)"#, url, issue_num)
        });
        format!("{}", line)
    } else {
        "(nothing??)".to_string()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    channel: Channel,
    edition: &'static str,
    mode: Mode,
    crate_type: CrateType,
    tests: bool,
    backtrace: bool,
    code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum CrateType {
    Bin,
}

#[derive(Debug, Deserialize)]
struct Response {
    stderr: String,
    stdout: String,
    success: bool,
}

fn extract_code_headers(code: &str) -> (&str, &str) {
    use combine::parser::char::{alpha_num, space, spaces, string};
    use combine::parser::choice::choice;
    use combine::parser::combinator::{attempt, ignore};
    use combine::parser::range::recognize;
    use combine::parser::repeat::{skip_many, skip_many1};
    use combine::parser::token::{none_of, token};
    use combine::parser::Parser;
    use std::iter::once;
    let spaces1 = || (space(), spaces());
    let attr_content = || (token('['), skip_many(none_of(once(']'))), token(']'));
    let outer_attr = (token('#'), spaces(), attr_content());
    let inner_attr = (token('#'), spaces(), token('!'), spaces(), attr_content());
    let extern_crate = (
        skip_many(outer_attr),
        spaces(),
        string("extern"),
        spaces1(),
        string("crate"),
        spaces1(),
        skip_many1(choice((alpha_num(), token('_')))),
        spaces(),
        token(';'),
    );
    let mut header = recognize((
        spaces(),
        skip_many((
            choice((attempt(ignore(extern_crate)), attempt(ignore(inner_attr)))),
            spaces(),
        )),
    ));
    header.parse(code).unwrap_or_else(|_| {
        debug_assert!(false, "extract_code_headers should always succeed");
        warn!("failed to split code: {}", code);
        ("", code)
    })
}

static PRELUDES: Lazy<String> = Lazy::new(get_preludes);

fn get_preludes() -> String {
    const LIST: &[&str] = &[
        "lazy_static::lazy_static",
        "std::{str, slice}",
        "std::borrow::Cow",
        "std::cell::{Cell, RefCell, UnsafeCell}",
        "std::char",
        "std::cmp::{Eq, Ord, PartialEq, PartialOrd, max, min}",
        "std::collections::{BTreeMap, BTreeSet, HashMap, HashSet}",
        "std::ffi::{CStr, CString, OsStr, OsString}",
        "std::fmt::{self, Debug, Display, Formatter}",
        "std::fs::File",
        "std::io",
        "std::io::prelude::*",
        "std::marker::PhantomData",
        "std::mem::{MaybeUninit, replace, size_of, swap, transmute}",
        "std::ops::*",
        "std::path::{Path, PathBuf}",
        "std::ptr::{NonNull, null, null_mut}",
        "std::rc::Rc",
        "std::sync::{Arc, Mutex, RwLock}",
        "std::sync::atomic::{self, AtomicBool, AtomicPtr}",
        "std::sync::atomic::{AtomicI8, AtomicI16, AtomicI32, AtomicI64, AtomicIsize}",
        "std::sync::atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, AtomicUsize}",
    ];

    let mut result = String::new();
    for item in LIST {
        writeln!(&mut result, "#[allow(unused_imports)] use {};", item).unwrap();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_code_headers() {
        let tests = &[
            (
                "#![feature(test)]\n\
                 #[macro_use] extern crate lazy_static;\n\
                 extern crate a_crate;\n",
                "1 + 1",
            ),
            (
                "  #\n!  \n [  feature(test)   ]  \
                 #  [ macro_use \r]extern crate lazy_static;\n\
                 extern \n\ncrate\r\r a_crate;",
                "1 + 1",
            ),
            ("", "externcrate a;"),
            ("", "extern cratea;"),
            ("", "extern crate a-b;"),
            ("", "extern crate ab"),
        ];
        for &(header, body) in tests {
            let code = format!("{}{}", header, body);
            let result = extract_code_headers(&code);
            assert_eq!(result, (header, body));
        }
    }
}
