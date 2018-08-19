mod about;
mod crate_;
mod eval;
mod version;

use futures::unsync::oneshot;
use futures::{Future, IntoFuture};
use reqwest::header::{Headers, UserAgent};
use reqwest::unstable::async::Client;
use std::borrow::Cow;
use std::cell::Cell;
use std::fmt::Display;
use tokio_core::reactor::Handle;
use utils::{is_separator, Void};

/// Command executor.
pub struct Executor<'a> {
    /// Reqwest client
    client: Client,
    /// Telegram username of the bot
    username: &'a str,
    /// A field to indicate that shutdown.
    shutdown: Cell<Option<oneshot::Sender<i64>>>,
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

struct CommandInfo<'a> {
    name: &'a str,
    args: &'a str,
    at_self: bool,
}

struct ExecutionContext<'a> {
    client: &'a Client,
    is_private: bool,
    args: &'a str,
}

impl<'a> Executor<'a> {
    /// Create new command executor.
    pub fn new(handle: &Handle, username: &'a str, shutdown: oneshot::Sender<i64>) -> Self {
        let mut headers = Headers::new();
        headers.set(UserAgent::new(super::USER_AGENT));
        let client = Client::builder()
            .default_headers(headers)
            .build(handle)
            .unwrap();
        let shutdown = Cell::new(Some(shutdown));
        Executor {
            client,
            username,
            shutdown,
        }
    }

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
        macro_rules! execute {
            ($future:expr) => {
                return Box::new($future.then(reply));
            };
        }
        if let Some(info) = self.parse_command(cmd.command) {
            let context = ExecutionContext {
                client: &self.client,
                is_private: cmd.is_private,
                args: info.args,
            };
            match info.name {
                "/crate" => execute!(crate_::run(context)),
                "/eval" => execute!(eval::run(context)),
                "/rustc_version" => execute!(version::run(context)),
                _ => {}
            }
            if cmd.is_private || info.at_self {
                match info.name {
                    "/version" => execute!(version::run(context)),
                    "/about" => execute!(about::run(context)),
                    _ => {}
                }
            }
            if cmd.is_private && cmd.is_admin {
                match info.name {
                    "/shutdown" => execute!(self.shutdown(cmd.id)),
                    _ => {}
                }
            }
        }
        Box::new(Err(()).into_future())
    }

    fn parse_command<'s>(&self, s: &'s str) -> Option<CommandInfo<'s>> {
        let (name, args) = match s.find(is_separator) {
            Some(pos) => (&s[..pos], &s[pos + 1..]),
            None => (s, ""),
        };
        let (name, at_self) = match name.find('@') {
            Some(pos) => {
                if &name[pos + 1..] != self.username {
                    return None;
                }
                (&name[..pos], true)
            }
            None => (name, false),
        };
        Some(CommandInfo {
            name,
            args,
            at_self,
        })
    }

    fn shutdown(&self, id: i64) -> impl Future<Item = Cow<'static, str>, Error = Void> {
        self.shutdown.replace(None).unwrap().send(id).unwrap();
        Ok("start shutting down...".into()).into_future()
    }
}
