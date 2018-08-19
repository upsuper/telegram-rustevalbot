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
    is_specific: bool,
    args: &'a str,
}

type BoxFutureStr = Box<dyn Future<Item = Cow<'static, str>, Error = Void>>;

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
    pub fn execute(&self, cmd: Command) -> Option<BoxFutureStr> {
        if let Some(info) = self.parse_command(cmd.command) {
            let context = ExecutionContext {
                client: &self.client,
                is_private: cmd.is_private,
                is_specific: cmd.is_private || info.at_self,
                args: info.args,
            };
            match execute_command(info.name, context) {
                Some(result) => return Some(result),
                None => {}
            }
            if cmd.is_private && cmd.is_admin {
                match info.name {
                    "/shutdown" => {
                        self.shutdown.replace(None).unwrap().send(cmd.id).unwrap();
                        return Some(str_to_box_future("start shutting down..."));
                    }
                    _ => {}
                }
            }
        }
        None
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
}

fn str_to_box_future(s: &'static str) -> BoxFutureStr {
    Box::new(Ok(s.into()).into_future())
}

macro_rules! commands {
    {
        general: [
            $($cmd_g:expr => $mod_g:ident / $desc_g:expr,)+
        ];
        specific: [
            $($cmd_s:expr => $mod_s:ident / $desc_s:expr,)+
        ];
    } => {
        fn display_help(is_private: bool) -> &'static str {
            if is_private {
                concat!(
                    $("<code>", $cmd_g, "</code> - ", $desc_g, "\n",)+
                    $("<code>", $cmd_s, "</code> - ", $desc_s, "\n",)+
                    "<code>/help</code> - show this information",
                )
            } else {
                concat!(
                    $("<code>", $cmd_g, "</code> - ", $desc_g, "\n",)+
                )
            }
        }

        fn execute_command(name: &str, ctx: ExecutionContext) -> Option<BoxFutureStr> {
            macro_rules! execute_mod {
                ($mod:ident) => {{
                    Some(Box::new($mod::run(ctx).then(|reply| {
                        Ok(match reply {
                            Ok(reply) => reply.into(),
                            Err(err) => format!("error: {}", err).into(),
                        })
                    })))
                }}
            }
            match name {
                $($cmd_g => execute_mod!($mod_g),)+
                $($cmd_s if ctx.is_specific => execute_mod!($mod_s),)+
                "/help" if ctx.is_specific => {
                    Some(str_to_box_future(display_help(ctx.is_private)))
                }
                _ => None
            }
        }
    }
}

commands! {
    general: [
        "/crate" => crate_ / "query crate information",
        "/eval" => eval / "evaluate a piece of Rust code",
        "/rustc_version" => version / "display rustc version being used",
    ];
    specific: [
        "/version" => version / "display rustc version being used",
        "/about" => about / "display information about this bot",
    ];
}
