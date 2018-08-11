use futures::Future;
use htmlescape::{encode_attribute, encode_minimal};
use regex::{Captures, Regex};
use reqwest::unstable::async::Client;

use utils;

lazy_static! {
    static ref RE_ERROR: Regex = Regex::new(r"^error\[(E\d{4})\]:").unwrap();
    static ref RE_CODE: Regex = Regex::new(r"`(.+?)`").unwrap();
}

pub fn run(client: &Client, param: &str) -> impl Future<Item = String, Error = &'static str> {
    let mut body = param;
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
    client
        .post("https://play.rust-lang.org/execute")
        .json(&req)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|mut resp| resp.json())
        .and_then(move |resp: Response| {
            if resp.success {
                return Ok(format!("<pre>{}</pre>", encode_minimal(resp.stdout.trim()),));
            }
            for line in resp.stderr.split('\n') {
                let line = line.trim();
                if line.starts_with("Compiling")
                    || line.starts_with("Finished")
                    || line.starts_with("Running")
                {
                    continue;
                }
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
                return Ok(format!("{}", line));
            }
            Ok("(nothing??)".to_string())
        })
        .map_err(utils::map_reqwest_error)
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
    pub fn as_str(&self) -> &'static str {
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
