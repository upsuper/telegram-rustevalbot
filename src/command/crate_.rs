use futures::Future;
use htmlescape::{encode_attribute, encode_minimal};
use itertools::Itertools;
use percent_encoding::{utf8_percent_encode, PATH_SEGMENT_ENCODE_SET};
use reqwest::StatusCode;

use super::ExecutionContext;
use utils;

pub(super) fn run(ctx: &ExecutionContext) -> impl Future<Item = String, Error = &'static str> {
    let name = ctx.args.trim().to_string();
    let name_url = utf8_percent_encode(&name, PATH_SEGMENT_ENCODE_SET).collect::<String>();
    let url = format!("https://crates.io/api/v1/crates/{}", name_url);
    ctx.client
        .get(&url)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|mut resp| resp.json())
        .map(move |resp: Info| {
            let info = resp.crate_;
            let crate_url = format!("https://crates.io/crates/{}", name_url);
            let doc_url = info
                .documentation
                .unwrap_or_else(|| format!("https://docs.rs/crate/{}", name_url));
            let mut message = format!(
                concat!(
                    "<b>{}</b> ({})",
                    r#" - <a href="{}">info</a>"#,
                    r#" - <a href="{}">doc</a>"#
                ),
                encode_minimal(&info.name),
                encode_minimal(&info.max_version),
                encode_attribute(&crate_url),
                encode_attribute(&doc_url),
            );
            if let Some(repo) = info.repository {
                message.push_str(&format!(
                    r#" - <a href="{}">repo</a>"#,
                    encode_attribute(&repo),
                ));
            }
            if let Some(description) = info.description {
                message.push_str(" - ");
                let description = description.split_whitespace().join(" ");
                message.push_str(&encode_minimal(&description));
            }
            message
        })
        .or_else(move |err| match err.status() {
            Some(StatusCode::NotFound) => Ok(format!("<b>{}</b> - not found", name)),
            _ => {
                warn!("{:?}", err);
                Err(utils::map_reqwest_error(&err))
            }
        })
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
