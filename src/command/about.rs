use futures::{Future, IntoFuture};

use super::ExecutionContext;
use utils::Void;

pub(super) fn run(_ctx: &ExecutionContext) -> impl Future<Item = String, Error = Void> {
    Ok(format!(
        "{} {}\n{}",
        env!("CARGO_PKG_NAME"),
        ::VERSION,
        env!("CARGO_PKG_HOMEPAGE")
    )).into_future()
}
