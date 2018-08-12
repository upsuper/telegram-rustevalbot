use futures::{Future, IntoFuture};
use reqwest::unstable::async::Client;

use super::eval::Channel;
use utils;

pub fn run(client: &Client, param: &str) -> Box<Future<Item = String, Error = &'static str>> {
    let mut channel = Channel::Stable;
    match param.trim_matches(utils::is_separator) {
        "" => {}
        "--stable" => channel = Channel::Stable,
        "--beta" => channel = Channel::Beta,
        "--nightly" => channel = Channel::Nightly,
        _ => return Box::new(Err("unknown argument").into_future()),
    }
    let url = format!(
        "https://play.rust-lang.org/meta/version/{}",
        channel.as_str(),
    );
    let future = client
        .get(&url)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|mut resp| resp.json())
        .and_then(|resp: Version| {
            Ok(format!(
                "rustc {} ({:.9} {})",
                resp.version, resp.hash, resp.date
            ))
        })
        .map_err(utils::map_reqwest_error);
    Box::new(future)
}

#[derive(Deserialize)]
struct Version {
    date: String,
    hash: String,
    version: String,
}
