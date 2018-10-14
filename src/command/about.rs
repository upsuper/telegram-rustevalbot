use futures::{Future, IntoFuture};

use super::{CommandImpl, ExecutionContext};

pub struct AboutCommand;

impl CommandImpl for AboutCommand {
    fn run(_ctx: &ExecutionContext) -> Box<dyn Future<Item = String, Error = &'static str>> {
        Box::new(
            Ok(format!(
                "{} {}\n{}",
                env!("CARGO_PKG_NAME"),
                ::VERSION,
                env!("CARGO_PKG_HOMEPAGE")
            )).into_future(),
        )
    }
}
