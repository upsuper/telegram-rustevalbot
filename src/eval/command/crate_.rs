use super::{BoxCommandFuture, CommandImpl, ExecutionContext};
use crate::utils;
use futures::Future;
use htmlescape::{encode_attribute, encode_minimal};
use itertools::Itertools;
use log::warn;
use percent_encoding::{utf8_percent_encode, PATH_SEGMENT_ENCODE_SET};
use reqwest::StatusCode;
use serde::Deserialize;
use std::borrow::Cow;
use std::fmt::{self, Display, Formatter, Write};
use url::Url;

pub struct CrateCommand;

#[derive(Copy, Clone, Debug)]
enum Mode {
    Keyword,
    Query,
}

#[derive(Debug, Default)]
pub struct Flags {
    mode: Option<Mode>,
}

impl CommandImpl for CrateCommand {
    impl_command_methods! {
        (flags: Flags) {
            ("--keyword", "query by keyword") {
                flags.mode = Some(Mode::Keyword);
            }
            ("--query", "general query") {
                flags.mode = Some(Mode::Query);
            }
        }
    }

    fn run(ctx: &ExecutionContext, flags: &Flags, arg: &str) -> BoxCommandFuture {
        let query = arg.trim().to_string();
        let mode = match flags.mode {
            Some(mode) => mode,
            None => {
                let future = ctx
                    .client
                    .get(&format!(
                        "https://crates.io/api/v1/crates/{}",
                        encode_for_url(&query)
                    )).send()
                    .and_then(|resp| resp.error_for_status())
                    .and_then(|mut resp| resp.json())
                    .map(move |resp: Info| format!("{}", resp.crate_))
                    .or_else(move |err| match err.status() {
                        Some(StatusCode::NOT_FOUND) => Ok(format!("<b>{}</b> - not found", query)),
                        _ => {
                            warn!("{:?}", err);
                            Err(utils::map_reqwest_error(&err))
                        }
                    });
                return Box::new(future);
            }
        };

        let mut url = Url::parse("https://crates.io/api/v1/crates").unwrap();
        let (query_name, sort) = match mode {
            Mode::Keyword => ("keyword", "recent-downloads"),
            Mode::Query => ("q", "relevance"),
        };
        url.query_pairs_mut()
            .append_pair(query_name, &query)
            .append_pair("sort", sort)
            .append_pair("per_page", if ctx.is_private { "10" } else { "3" });
        let future = ctx
            .client
            .get(url)
            .send()
            .and_then(|resp| resp.error_for_status())
            .and_then(|mut resp| resp.json())
            .map(move |resp: Crates| {
                let count = resp.crates.len();
                if count == 0 {
                    return "(none)".to_string();
                }
                let mut result = String::new();
                for c in resp.crates {
                    writeln!(result, "{}", c).unwrap();
                }
                if count < resp.meta.total {
                    let query = encode_for_url(&query);
                    let url = match mode {
                        Mode::Keyword => format!("https://crates.io/keywords/{}", query),
                        Mode::Query => format!("https://crates.io/search?q={}", query),
                    };
                    write!(
                        result,
                        r#"<a href="{}">More...</a>"#,
                        encode_attribute(&url)
                    ).unwrap();
                }
                result
            }).or_else(|err| {
                warn!("{:?}", err);
                Err(utils::map_reqwest_error(&err))
            });
        Box::new(future)
    }
}

fn encode_for_url(s: &str) -> String {
    utf8_percent_encode(s, PATH_SEGMENT_ENCODE_SET).collect()
}

#[derive(Debug, Deserialize)]
struct Info {
    #[serde(rename = "crate")]
    crate_: Crate,
}

#[derive(Debug, Deserialize)]
struct Crates {
    crates: Vec<Crate>,
    meta: CratesMeta,
}

#[derive(Debug, Deserialize)]
struct CratesMeta {
    total: usize,
}

#[derive(Debug, Deserialize)]
struct Crate {
    id: String,
    name: String,
    description: Option<String>,
    max_version: String,
    documentation: Option<String>,
    repository: Option<String>,
}

impl Display for Crate {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let name_url = encode_for_url(&self.name);
        let crate_url = format!("https://crates.io/crates/{}", name_url);
        let doc_url = self
            .documentation
            .as_ref()
            .map(|s| Cow::Borrowed(s.as_str()))
            .unwrap_or_else(|| Cow::from(format!("https://docs.rs/crate/{}", name_url)));
        write!(
            f,
            concat!(
                "<b>{}</b> ({})",
                r#" - <a href="{}">info</a>"#,
                r#" - <a href="{}">doc</a>"#
            ),
            encode_minimal(&self.name),
            encode_minimal(&self.max_version),
            encode_attribute(&crate_url),
            encode_attribute(&doc_url),
        )?;
        if let Some(repo) = &self.repository {
            write!(f, r#" - <a href="{}">repo</a>"#, encode_attribute(&repo))?;
        }
        if let Some(description) = &self.description {
            f.write_str(" - ")?;
            let description = description.split_whitespace().join(" ");
            f.write_str(&encode_minimal(&description))?;
        }
        Ok(())
    }
}
