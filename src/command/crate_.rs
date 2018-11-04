use super::{CommandImpl, ExecutionContext};
use crate::utils;
use futures::Future;
use htmlescape::{encode_attribute, encode_minimal};
use itertools::Itertools;
use log::warn;
use percent_encoding::{utf8_percent_encode, PATH_SEGMENT_ENCODE_SET};
use reqwest::StatusCode;
use serde::Deserialize;
use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};

pub struct CrateCommand;

impl CommandImpl for CrateCommand {
    type Flags = ();

    fn run(
        ctx: &ExecutionContext,
        _flags: &(),
        arg: &str,
    ) -> Box<dyn Future<Item = String, Error = &'static str>> {
        let name = arg.trim().to_string();
        let future = ctx
            .client
            .get(&format!(
                "https://crates.io/api/v1/crates/{}",
                encode_name(&name)
            )).send()
            .and_then(|resp| resp.error_for_status())
            .and_then(|mut resp| resp.json())
            .map(move |resp: Info| format!("{}", resp.crate_))
            .or_else(move |err| match err.status() {
                Some(StatusCode::NOT_FOUND) => Ok(format!("<b>{}</b> - not found", name)),
                _ => {
                    warn!("{:?}", err);
                    Err(utils::map_reqwest_error(&err))
                }
            });
        Box::new(future)
    }
}

fn encode_name(name: &str) -> String {
    utf8_percent_encode(name, PATH_SEGMENT_ENCODE_SET).collect()
}

#[derive(Debug, Deserialize)]
struct Info {
    #[serde(rename = "crate")]
    crate_: Crate,
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
        let name_url = encode_name(&self.name);
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
