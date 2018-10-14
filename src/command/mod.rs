mod about;
mod crate_;
mod doc;
mod eval;
mod version;

use futures::{Future, IntoFuture};
use reqwest::async::Client;
use reqwest::header::{HeaderMap, USER_AGENT};
use std::borrow::Cow;
use utils::Void;

/// Command executor.
pub struct Executor {
    /// Reqwest client
    client: Client,
    /// Telegram username of the bot
    username: &'static str,
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

impl Executor {
    /// Create new command executor.
    pub fn new(username: &'static str) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, super::USER_AGENT.parse().unwrap());
        let client = Client::builder().default_headers(headers).build().unwrap();
        Executor { client, username }
    }

    /// Execute a command.
    ///
    /// Future resolves to a message to send back. If nothing can be
    /// replied, it rejects.
    pub fn execute(&self, cmd: &Command) -> Option<BoxFutureStr> {
        if let Some(info) = self.parse_command(cmd.command) {
            let context = ExecutionContext {
                client: &self.client,
                is_private: cmd.is_private,
                is_specific: cmd.is_private || info.at_self,
                args: info.args,
            };
            if let Some(result) = execute_command(info.name, &context) {
                return Some(result);
            }
            if cmd.is_private && cmd.is_admin && info.name == "/shutdown" {
                super::SHUTDOWN.shutdown(Some(cmd.id));
                return Some(str_to_box_future("start shutting down..."));
            }
        }
        None
    }

    fn parse_command<'s>(&self, s: &'s str) -> Option<CommandInfo<'s>> {
        use combine::parser::{
            char::{alpha_num, spaces, string},
            choice::optional,
            item::{any, eof, item},
            range::recognize,
            repeat::{skip_many, skip_many1},
            Parser,
        };
        (
            recognize((item('/'), skip_many1(alpha_num()))),
            optional((item('@'), string(self.username))),
            optional((spaces(), recognize(skip_many(any())))),
            eof(),
        )
            .map(|(name, at_self, args, _)| CommandInfo {
                name,
                args: args.map(|(_, s)| s).unwrap_or(""),
                at_self: at_self.is_some(),
            }).parse(s)
            .map(|(ci, _)| ci)
            .ok()
    }
}

fn str_to_box_future(s: &'static str) -> BoxFutureStr {
    Box::new(Ok(s.into()).into_future())
}

trait CommandImpl {
    #[inline]
    fn init() {}

    // XXX Trait functions cannot return `impl Trait`.
    // Hopefully in the future we can use `async fn` here.
    fn run(ctx: &ExecutionContext) -> Box<dyn Future<Item = String, Error = &'static str>>;
}

macro_rules! commands {
    {
        general: [
            $($cmd_g:expr => $ty_g:ty: $desc_g:expr,)+
        ];
        specific: [
            $($cmd_s:expr => $ty_s:ty: $desc_s:expr,)+
        ];
    } => {
        pub(super) fn init() {
            $(<$ty_g>::init();)+
            $(<$ty_s>::init();)+
        }

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

        fn execute_command(name: &str, ctx: &ExecutionContext) -> Option<BoxFutureStr> {
            macro_rules! execute_mod {
                ($ty:ty) => {{
                    Some(Box::new(<$ty>::run(&ctx).then(|reply| {
                        Ok(match reply {
                            Ok(reply) => reply.into(),
                            Err(err) => format!("error: {}", err).into(),
                        })
                    })))
                }}
            }
            match name {
                $($cmd_g => execute_mod!($ty_g),)+
                $($cmd_s if ctx.is_specific => execute_mod!($ty_s),)+
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
        "/crate" => crate_::CrateCommand: "query crate information",
        "/doc" => doc::DocCommand: "query document of Rust's standard library",
        "/eval" => eval::EvalCommand: "evaluate a piece of Rust code",
        "/rustc_version" => version::VersionCommand: "display rustc version being used",
    ];
    specific: [
        "/version" => version::VersionCommand: "display rustc version being used",
        "/about" => about::AboutCommand: "display information about this bot",
    ];
}
