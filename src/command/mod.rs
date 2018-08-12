mod crate_;
mod eval;
mod meta;

use futures::unsync::oneshot;
use futures::{Future, IntoFuture};
use reqwest::unstable::async::Client;
use std::borrow::Cow;
use std::cell::Cell;
use std::fmt::Display;
use utils::is_separator;

/// Command executor.
pub struct Executor<'a> {
    /// Reqwest client
    pub client: &'a Client,
    /// A field to indicate that shutdown.
    pub shutdown: Cell<Option<oneshot::Sender<i64>>>,
}

pub struct Command<'a> {
    /// Update id of the command
    pub id: i64,
    /// The command text
    pub command: &'a str,
    /// Whether this command is from an admin
    pub is_admin: bool,
    /// Whether this command is in private chat
    pub is_private: bool,
}

impl<'a> Executor<'a> {
    /// Execute a command.
    ///
    /// Future resolves to a message to send back. If nothing can be
    /// replied, it rejects.
    pub fn execute(&self, cmd: Command) -> Box<Future<Item = Cow<'static, str>, Error = ()>> {
        fn reply(
            reply: Result<impl Into<Cow<'static, str>>, impl Display>,
        ) -> Result<Cow<'static, str>, ()> {
            Ok(match reply {
                Ok(reply) => reply.into(),
                Err(err) => format!("error: {}", err).into(),
            })
        }
        match split_command(cmd.command) {
            ("/crate", param) => Box::new(crate_::run(self.client, param).then(reply)),
            ("/eval", param) => Box::new(eval::run(self.client, param).then(reply)),
            ("/meta", param) => Box::new(meta::run(self.client, param).then(reply)),
            ("/version", "") => Box::new(version()),
            ("/shutdown", "") if cmd.is_admin => Box::new(self.shutdown(cmd.id)),
            _ => Box::new(Err(()).into_future()),
        }
    }

    fn shutdown(&self, id: i64) -> impl Future<Item = Cow<'static, str>, Error = ()> {
        self.shutdown.replace(None).unwrap().send(id).unwrap();
        Ok("start shutting down...".into()).into_future()
    }
}

fn split_command<'a>(s: &'a str) -> (&'a str, &'a str) {
    match s.find(is_separator) {
        Some(pos) => (&s[..pos], &s[pos + 1..]),
        None => (s, ""),
    }
}

fn version() -> impl Future<Item = Cow<'static, str>, Error = ()> {
    Ok(format!("version: {}", super::VERSION).into()).into_future()
}
