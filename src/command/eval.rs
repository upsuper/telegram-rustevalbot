use futures::Future;
use htmlescape::{encode_attribute, encode_minimal};
use regex::{Captures, Regex};

use super::{CommandImpl, ExecutionContext};
use utils;

lazy_static! {
    static ref RE_ERROR: Regex = Regex::new(r"^error\[(E\d{4})\]:").unwrap();
    static ref RE_CODE: Regex = Regex::new(r"`(.+?)`").unwrap();
    static ref RE_ISSUE: Regex = Regex::new(r"\(see issue #(\d+)\)").unwrap();
}

pub struct EvalCommand;

#[derive(Debug, Default)]
pub struct Flags {
    channel: Option<Channel>,
    edition: Option<&'static str>,
    mode: Option<Mode>,
    bare: bool,
}

impl CommandImpl for EvalCommand {
    impl_command_methods! {
        (flags: Flags) {
            ("--stable", "use stable channel") {
                flags.channel = Some(Channel::Stable);
            }
            ("--beta", "use beta channel") {
                flags.channel = Some(Channel::Beta);
            }
            ("--nightly", "use nightly channel") {
                flags.channel = Some(Channel::Nightly);
            }
            ("--2015", "use 2015 edition") {
                flags.edition = Some("2015");
            }
            ("--2018", "use 2018 edition") {
                flags.edition = Some("2018");
            }
            ("--debug", "do debug build") {
                flags.mode = Some(Mode::Debug);
            }
            ("--release", "do release build") {
                flags.mode = Some(Mode::Release);
            }
            ("--bare", "don't add any wrapping code") {
                flags.bare = true;
            }
        }
    }

    fn run(
        ctx: &ExecutionContext,
        flags: &Flags,
        arg: &str,
    ) -> Box<dyn Future<Item = String, Error = &'static str>> {
        let is_private = ctx.is_private;
        let code = if flags.bare {
            arg.to_string()
        } else {
            format!(include_str!("eval_template.rs"), code = arg)
        };
        let channel = flags.channel.unwrap_or(Channel::Stable);
        let req = Request {
            channel,
            edition: flags.edition,
            mode: flags.mode.unwrap_or(Mode::Debug),
            crate_type: CrateType::Bin,
            tests: false,
            backtrace: false,
            code,
        };
        let future = ctx
            .client
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
                        const MAX_LINES: usize = 3;
                        const MAX_TOTAL_COLUMNS: usize = MAX_LINES * 72;
                        utils::truncate_output(output, MAX_LINES, MAX_TOTAL_COLUMNS)
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
            }).map_err(|e| utils::map_reqwest_error(&e));
        Box::new(future)
    }
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
