use futures::{Future, IntoFuture};

use super::eval::Channel;
use super::ExecutionContext;
use utils;

pub(super) fn run(ctx: &ExecutionContext) -> Box<Future<Item = String, Error = &'static str>> {
    let mut channel = Channel::Stable;
    match ctx.args.trim_matches(utils::is_separator) {
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
    let future = ctx
        .client
        .get(&url)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|mut resp| resp.json())
        .map(|resp: Version| format!("rustc {} ({:.9} {})", resp.version, resp.hash, resp.date))
        .map_err(|e| utils::map_reqwest_error(&e));
    Box::new(future)
}

#[derive(Deserialize)]
struct Version {
    date: String,
    hash: String,
    version: String,
}
