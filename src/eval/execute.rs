use super::parse::Flags;
use crate::eval::parse::{get_help_message, Channel, Mode};
use crate::utils;
use futures::{Future, IntoFuture};
use htmlescape::{encode_attribute, encode_minimal};
use lazy_static::lazy_static;
use log::{debug, warn};
use regex::{Captures, Regex};
use reqwest::r#async::Client;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write;

pub fn execute(
    client: &Client,
    content: &str,
    flags: Flags,
    is_private: bool,
) -> Option<Box<dyn Future<Item = String, Error = &'static str> + Send>> {
    if flags.help {
        Some(Box::new(Ok(get_help_message()).into_future()))
    } else if flags.version {
        Some(Box::new(get_version(client, flags.channel)))
    } else if !content.trim().is_empty() {
        Some(Box::new(run_code(client, content, flags, is_private)))
    } else {
        None
    }
}

fn get_version(
    client: &Client,
    channel: Option<Channel>,
) -> impl Future<Item = String, Error = &'static str> {
    let url = format!(
        "https://play.rust-lang.org/meta/version/{}",
        channel.unwrap_or(Channel::Stable).as_str(),
    );
    client
        .get(&url)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|mut resp| resp.json())
        .map(|resp: Version| format!("rustc {} ({:.9} {})", resp.version, resp.hash, resp.date))
        .map_err(|e| utils::map_reqwest_error(&e))
}

#[derive(Deserialize)]
struct Version {
    date: String,
    hash: String,
    version: String,
}

fn run_code(
    client: &Client,
    code: &str,
    flags: Flags,
    is_private: bool,
) -> impl Future<Item = String, Error = &'static str> {
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
    client
        .post("https://play.rust-lang.org/execute")
        .json(&req)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|mut resp| resp.json())
        .map(move |resp| generate_result_from_response(resp, channel, is_private))
        .map_err(|e| utils::map_reqwest_error(&e))
}

fn generate_code_to_send(code: &str, bare: bool) -> String {
    if bare {
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

    lazy_static! {
        static ref RE_ERROR: Regex = Regex::new(r"^error\[(E\d{4})\]:").unwrap();
        static ref RE_CODE: Regex = Regex::new(r"`(.+?)`").unwrap();
        static ref RE_ISSUE: Regex = Regex::new(r"\(see issue #(\d+)\)").unwrap();
    }
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
    use combine::parser::item::{item, none_of};
    use combine::parser::range::recognize;
    use combine::parser::repeat::{skip_many, skip_many1};
    use combine::parser::Parser;
    use std::iter::once;
    let spaces1 = || (space(), spaces());
    let attr_content = || (item('['), skip_many(none_of(once(']'))), item(']'));
    let outer_attr = (item('#'), spaces(), attr_content());
    let inner_attr = (item('#'), spaces(), item('!'), spaces(), attr_content());
    let extern_crate = (
        skip_many(outer_attr),
        spaces(),
        string("extern"),
        spaces1(),
        string("crate"),
        spaces1(),
        skip_many1(choice((alpha_num(), item('_')))),
        spaces(),
        item(';'),
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

lazy_static! {
    static ref PRELUDES: String = get_preludes();
}

fn get_preludes() -> String {
    const LIST: &[&str] = &[
        "std::{f32, f64}",
        "std::{i8, i16, i32, i64, i128, isize}",
        "std::{str, slice}",
        "std::{u8, u16, u32, u64, u128, usize}",
        "std::char",
        "std::collections::{HashMap, HashSet}",
        "std::ffi::{CStr, CString, OsStr, OsString}",
        "std::fmt::{self, Debug, Display, Formatter}",
        "std::fs::File",
        "std::io",
        "std::io::prelude::*",
        "std::marker::PhantomData",
        "std::mem::{MaybeUninit, replace, size_of, swap, transmute}",
        "std::ops::*",
        "std::path::{Path, PathBuf}",
        "std::ptr::NonNull",
        "std::rc::Rc",
        "std::sync::Arc",
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
