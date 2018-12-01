use crate::shutdown::Shutdown;
use crate::utils::Void;
use futures::{Future, IntoFuture};
use log::debug;
use parking_lot::Mutex;
use reqwest::r#async::Client;
use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use telegram_types::bot::types::UpdateId;

/// Command executor.
pub struct Executor {
    /// Reqwest client
    client: Client,
    /// Telegram username of the bot
    username: &'static str,
    /// Channel to trigger shutdown
    shutdown: Arc<Shutdown>,
    /// Update ID of the shutdown message
    shutdown_id: Arc<Mutex<Option<UpdateId>>>,
}

pub struct Command<'a> {
    /// Update id of the command
    pub id: UpdateId,
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
}

type BoxFutureStr = Box<dyn Future<Item = Cow<'static, str>, Error = Void> + Send>;

impl Executor {
    /// Create new command executor.
    pub fn new(
        client: Client,
        username: &'static str,
        shutdown: Arc<Shutdown>,
        shutdown_id: Arc<Mutex<Option<UpdateId>>>,
    ) -> Self {
        Executor {
            client,
            username,
            shutdown,
            shutdown_id,
        }
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
            };
            if let Some(result) = execute_command_with_name(info.name, &context, info.args) {
                return Some(result);
            }
            if cmd.is_private && cmd.is_admin && info.name == "/shutdown" {
                *self.shutdown_id.lock() = Some(cmd.id);
                self.shutdown.shutdown();
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
            optional((item('@'), string(&self.username))),
            optional((spaces(), recognize(skip_many(any())))),
            eof(),
        )
            .map(|(name, at_self, args, _)| CommandInfo {
                name,
                args: args.map(|(_, s)| s).unwrap_or(""),
                at_self: at_self.is_some(),
            })
            .parse(s)
            .map(|(ci, _)| ci)
            .ok()
    }
}

fn str_to_box_future(s: &'static str) -> BoxFutureStr {
    Box::new(Ok(s.into()).into_future())
}

type BoxCommandFuture = Box<dyn Future<Item = String, Error = &'static str> + Send>;

trait CommandImpl {
    type Flags: Debug + Default;

    /// Returns the help message.
    fn help() -> &'static str {
        ""
    }

    /// Parses the given flag and add into flags. Returns whether the flag is
    /// recognized.
    fn add_flag(_flags: &mut Self::Flags, _flag: &str) -> bool {
        false
    }

    // XXX Trait functions cannot return `impl Trait`.
    // Hopefully in the future we can use `async fn` here.
    fn run(ctx: &ExecutionContext, flags: &Self::Flags, arg: &str) -> BoxCommandFuture;
}

macro_rules! impl_command_methods {
    (($flags:ident: $flags_ty:ty) {
        $(($flag:expr, $help:expr) $code:block)*
    }) => {
        type Flags = $flags_ty;

        fn help() -> &'static str {
            concat!(
                $("<code>", $flag, "</code> - ", $help, "\n",)*
                "<code>--help</code> - show this information",
            )
        }

        fn add_flag($flags: &mut $flags_ty, flag: &str) -> bool {
            match flag {
                $($flag => $code,)+
                _ => return false,
            }
            true
        }
    }
}

mod about;
mod crate_;
mod doc;
mod eval;
mod version;

pub use self::doc::init;

macro_rules! commands {
    {
        general: [
            $($cmd_g:expr => $ty_g:ty: $desc_g:expr,)+
        ];
        specific: [
            $($cmd_s:expr => $ty_s:ty: $desc_s:expr,)+
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

        fn execute_command_with_name(
            name: &str,
            ctx: &ExecutionContext,
            args: &str,
        ) -> Option<BoxFutureStr> {
            macro_rules! execute {
                ($ty:ty) => { Some(execute_command::<$ty>(ctx, args)) }
            }
            match name {
                $($cmd_g => execute!($ty_g),)+
                $($cmd_s if ctx.is_specific => execute!($ty_s),)+
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

struct FlagsBuilder<Impl: CommandImpl> {
    flags: Impl::Flags,
    help: bool,
    error: bool,
}

impl<Impl: CommandImpl> Debug for FlagsBuilder<Impl> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        f.write_str("FlagsBuilder(")?;
        if self.error {
            f.write_str("error")?;
        } else if self.help {
            f.write_str("help")?;
        } else {
            write!(f, "{:?}", self.flags)?;
        }
        f.write_str(")")
    }
}

impl<Impl: CommandImpl> Default for FlagsBuilder<Impl> {
    fn default() -> Self {
        FlagsBuilder {
            flags: Default::default(),
            help: false,
            error: false,
        }
    }
}

impl<'a, Impl: CommandImpl> Extend<&'a str> for FlagsBuilder<Impl> {
    fn extend<T>(&mut self, iter: T)
    where
        T: IntoIterator<Item = &'a str>,
    {
        if !self.error {
            self.error = !iter.into_iter().all(|flag| match flag {
                "--help" => {
                    self.help = true;
                    true
                }
                _ => Impl::add_flag(&mut self.flags, flag),
            });
        }
    }
}

#[derive(Debug, PartialEq)]
enum CommandParseResult<'a, Flags> {
    Normal(Flags, &'a str),
    Help,
    Error,
}

fn parse_command_flags<Impl: CommandImpl>(args: &str) -> CommandParseResult<Impl::Flags> {
    use combine::parser::{
        char::{alpha_num, spaces, string},
        range::recognize,
        repeat::{many, skip_many1},
        Parser,
    };
    let flag_parser = recognize((string("--"), skip_many1(alpha_num())));
    let parsing_result =
        many::<FlagsBuilder<Impl>, _>((flag_parser, spaces()).map(|(f, _)| f)).parse(args);
    debug!("parsed command: {:?}", parsing_result);
    match parsing_result {
        Ok((builder, remaining)) => {
            if builder.error {
                CommandParseResult::Error
            } else if builder.help {
                CommandParseResult::Help
            } else {
                CommandParseResult::Normal(builder.flags, remaining)
            }
        }
        _ => CommandParseResult::Error,
    }
}

fn execute_command<Impl: CommandImpl>(ctx: &ExecutionContext, args: &str) -> BoxFutureStr {
    let (flags, remaining) = match parse_command_flags::<Impl>(args) {
        CommandParseResult::Normal(flags, remaining) => (flags, remaining),
        CommandParseResult::Help => return str_to_box_future(Impl::help()),
        CommandParseResult::Error => {
            return str_to_box_future("error: unable to parse the command");
        }
    };
    Box::new(Impl::run(&ctx, &flags, remaining).then(|reply| {
        Ok(match reply {
            Ok(reply) => reply.into(),
            Err(err) => format!("error: {}", err).into(),
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCommand;

    impl CommandImpl for TestCommand {
        type Flags = Vec<&'static str>;

        fn add_flag(flags: &mut Self::Flags, flag: &str) -> bool {
            for f in &["--chars", "--12345", "--chars12345"] {
                if flag == *f {
                    flags.push(f);
                    return true;
                }
            }
            false
        }

        fn run(_ctx: &ExecutionContext, _flags: &Self::Flags, _arg: &str) -> BoxCommandFuture {
            unreachable!()
        }
    }

    fn parse(arg: &str) -> CommandParseResult<Vec<&'static str>> {
        parse_command_flags::<TestCommand>(arg)
    }

    #[test]
    fn test_flag_parsing() {
        use super::CommandParseResult::*;
        assert_eq!(
            parse("--chars --12345 --chars12345 xxx"),
            Normal(vec!["--chars", "--12345", "--chars12345"], "xxx"),
        );
        assert_eq!(
            parse("--12345 --chars"),
            Normal(vec!["--12345", "--chars"], ""),
        );
        assert_eq!(
            parse("--12345 --12345 --chars --chars"),
            Normal(vec!["--12345", "--12345", "--chars", "--chars"], ""),
        );
        assert_eq!(parse("--help"), Help);
        assert_eq!(parse("--help xxxxx"), Help);
        assert_eq!(parse("--12345 --help --chars xxx"), Help);
        assert_eq!(parse("--unknown"), Error);
        assert_eq!(parse("--help --unknown"), Error);
        assert_eq!(parse("--12345 --unknown"), Error);
        assert_eq!(parse("--unknown --12345"), Error);
    }
}
