use super::eval::Channel;
use super::{BoxCommandFuture, CommandImpl, ExecutionContext};
use crate::utils;
use futures::Future;
use serde::Deserialize;

pub struct VersionCommand;

impl CommandImpl for VersionCommand {
    impl_command_methods! {
        (channel: Option<Channel>) {
            ("--stable", "check stable channel") {
                *channel = Some(Channel::Stable);
            }
            ("--beta", "check beta channel") {
                *channel = Some(Channel::Beta);
            }
            ("--nightly", "check nightly channel") {
                *channel = Some(Channel::Nightly);
            }
        }
    }

    fn run(ctx: &ExecutionContext<'_>, channel: &Option<Channel>, _arg: &str) -> BoxCommandFuture {
        let url = format!(
            "https://play.rust-lang.org/meta/version/{}",
            channel.unwrap_or(Channel::Stable).as_str(),
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
}

#[derive(Deserialize)]
struct Version {
    date: String,
    hash: String,
    version: String,
}
