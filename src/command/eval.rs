use futures::Future;
use htmlescape::{encode_attribute, encode_minimal};
use regex::{Captures, Regex};
use std::borrow::Cow;
use unicode_width::UnicodeWidthChar;

use super::ExecutionContext;
use utils;

lazy_static! {
    static ref RE_ERROR: Regex = Regex::new(r"^error\[(E\d{4})\]:").unwrap();
    static ref RE_CODE: Regex = Regex::new(r"`(.+?)`").unwrap();
    static ref RE_ISSUE: Regex = Regex::new(r"\(see issue #(\d+)\)").unwrap();
}

pub(super) fn run(ctx: &ExecutionContext) -> impl Future<Item = String, Error = &'static str> {
    let mut body = ctx.args;
    let mut channel = None;
    let mut edition = None;
    let mut mode = None;
    let mut bare = false;
    loop {
        body = body.trim_left_matches(utils::is_separator);
        let flag = body.split(utils::is_separator).next().unwrap_or("");
        match flag {
            "--stable" => channel = Some(Channel::Stable),
            "--beta" => channel = Some(Channel::Beta),
            "--nightly" => channel = Some(Channel::Nightly),
            "--2015" => edition = Some("2015"),
            "--2018" => edition = Some("2018"),
            "--debug" => mode = Some(Mode::Debug),
            "--release" => mode = Some(Mode::Release),
            "--bare" => bare = true,
            _ => break,
        }
        body = &body[flag.len()..];
    }

    let is_private = ctx.is_private;
    let code = if bare {
        body.to_string()
    } else {
        format!(include_str!("eval_template.rs"), code = body)
    };
    let channel = channel.unwrap_or(Channel::Stable);
    let req = Request {
        channel,
        edition,
        mode: mode.unwrap_or(Mode::Debug),
        crate_type: CrateType::Bin,
        tests: false,
        backtrace: false,
        code,
    };
    ctx.client
        .post("https://play.rust-lang.org/execute")
        .json(&req)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|mut resp| resp.json())
        .map(move |resp: Response| {
            if resp.success {
                let output = resp.stdout.trim();
                let output = if is_private {
                    output.into()
                } else {
                    truncate_output(output)
                };
                if output.is_empty() {
                    return "(no output)".to_string();
                }
                return format!("<pre>{}</pre>", encode_minimal(&output));
            }
            let mut return_line: Option<&str> = None;
            for line in resp.stderr.split('\n') {
                let line = line.trim();
                if line.starts_with("Compiling")
                    || line.starts_with("Finished")
                    || line.starts_with("Running")
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
                let line = RE_ERROR.replacen(&line, 1, |captures: &Captures| {
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
                let line = RE_CODE.replace_all(&line, |captures: &Captures| {
                    format!("<code>{}</code>", captures.get(1).unwrap().as_str())
                });
                let line = RE_ISSUE.replacen(&line, 1, |captures: &Captures| {
                    let issue_num = captures.get(1).unwrap().as_str();
                    let url = format!("https://github.com/rust-lang/rust/issues/{}", issue_num);
                    format!(r#"(see issue <a href="{}">#{}</a>)"#, url, issue_num)
                });
                format!("{}", line)
            } else {
                "(nothing??)".to_string()
            }
        }).map_err(|e| utils::map_reqwest_error(&e))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    channel: Channel,
    #[serde(skip_serializing_if = "Option::is_none")]
    edition: Option<&'static str>,
    mode: Mode,
    crate_type: CrateType,
    tests: bool,
    backtrace: bool,
    code: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Debug,
    Release,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum CrateType {
    Bin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Nightly => "nightly",
        }
    }
}

#[derive(Debug, Deserialize)]
struct Response {
    stderr: String,
    stdout: String,
    success: bool,
}

fn truncate_output(output: &str) -> Cow<str> {
    const MAX_LINES: usize = 3;
    const MAX_TOTAL_COLUMNS: usize = MAX_LINES * 72;
    let mut line_count = 0;
    let mut column_count = 0;
    for (pos, c) in output.char_indices() {
        column_count += c.width_cjk().unwrap_or(1);
        if column_count > MAX_TOTAL_COLUMNS {
            let mut truncate_width = 0;
            for (pos, c) in output[..pos].char_indices().rev() {
                truncate_width += c.width_cjk().unwrap_or(1);
                if truncate_width >= 3 {
                    return format!("{}...", &output[..pos]).into();
                }
            }
        }
        if c == '\n' {
            line_count += 1;
            if line_count == MAX_LINES {
                return format!("{}...", &output[..pos]).into();
            }
        }
    }
    output.into()
}

#[cfg(test)]
mod test {
    use super::*;

    fn construct_string(parts: &[(&str, usize)]) -> String {
        let len = parts.iter().map(|(s, n)| s.len() * n).sum();
        let mut result = String::with_capacity(len);
        for &(s, n) in parts.iter() {
            for _ in 0..n {
                result.push_str(s);
            }
        }
        result
    }

    #[test]
    fn test_truncate_output() {
        struct Testcase<'a> {
            input: &'a [(&'a str, usize)],
            expected: &'a [(&'a str, usize)],
        }
        const TESTCASES: &[Testcase] = &[
            Testcase {
                input: &[("a", 300)],
                expected: &[("a", 213), ("...", 1)],
            },
            Testcase {
                input: &[("啊", 300)],
                expected: &[("啊", 106), ("...", 1)],
            },
            Testcase {
                input: &[("啊", 107), ("a", 5)],
                expected: &[("啊", 106), ("...", 1)],
            },
            Testcase {
                input: &[("a\n", 10)],
                expected: &[("a\n", 2), ("a...", 1)],
            },
        ];
        for Testcase { input, expected } in TESTCASES.iter() {
            assert_eq!(
                truncate_output(&construct_string(input)),
                construct_string(expected)
            );
        }
    }
}
