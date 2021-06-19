#![recursion_limit = "128"]

mod bot;
mod bot_runner;
mod cratesio;
mod eval;
mod rustdoc;
mod shutdown;
#[cfg(unix)]
mod signal;
mod task_tracker;
mod upgrade;
mod utils;

use crate::bot::{Bot, Error};
use crate::bot_runner::BotRunner;
use crate::cratesio::CratesioBot;
use crate::eval::EvalBot;
use crate::rustdoc::RustdocBot;
use crate::shutdown::Shutdown;
use futures::channel::oneshot::Receiver;
use futures::future::{self, TryFutureExt as _};
use itertools::Itertools;
use log::{error, info};
use once_cell::sync::Lazy;
use reqwest::Client;
use std::env;
use std::fmt::Write as FmtWrite;
use std::future::Future;
use std::io::Write as IOWrite;
use telegram_types::bot::types::{ChatId, UserId};
use tokio::runtime::Runtime;

static ADMIN_ID: Lazy<UserId> = Lazy::new(|| {
    env::var("BOT_ADMIN_ID")
        .ok()
        .and_then(|s| str::parse(&s).map(UserId).ok())
        .expect("BOT_ADMIN_ID must be a valid user id")
});
static ABOUT_MESSAGE: Lazy<String> = Lazy::new(|| {
    format!(
        "{} {}\n{}",
        env!("CARGO_PKG_NAME"),
        env!("VERSION"),
        env!("CARGO_PKG_HOMEPAGE")
    )
});

fn main() {
    // We don't care if we fail to load .env file.
    let _ = dotenv::from_path(std::env::current_dir().unwrap().join(".env"));
    init_logger();

    let shutdown = Shutdown::create();
    #[cfg(unix)]
    signal::init(shutdown.clone());
    upgrade::init(shutdown.clone());
    rustdoc::init();

    info!("Running as `{}`", env!("USER_AGENT"));

    let runtime = Runtime::new().unwrap();
    let (spawner, waiter) = task_tracker::create(&runtime);
    let client = build_client();
    let bot_runner = BotRunner {
        client: &client,
        spawner: &spawner,
        shutdown: &shutdown,
        report_error: report_error_to_admin,
    };

    // Kick off eval bot.
    let client_clone = client.clone();
    let eval_receiver = bot_runner.run(
        "eval",
        "EVAL_TELEGRAM_TOKEN",
        move |bot| EvalBot::new(client_clone, bot),
        EvalBot::handle_update,
    );

    // Kick off cratesio bot.
    let client_clone = client.clone();
    let cratesio_receiver = bot_runner.run(
        "cratesio",
        "CRATESIO_TELEGRAM_TOKEN",
        move |bot| CratesioBot::new(client_clone, bot),
        CratesioBot::handle_update,
    );

    // Kick off rustdoc bot.
    let rustdoc_receiver = bot_runner.run(
        "rustdoc",
        "RUSTDOC_TELEGRAM_TOKEN",
        RustdocBot::new,
        RustdocBot::handle_update,
    );

    async fn bind_name(
        receiver: Receiver<Result<Option<Bot>, ()>>,
        name: &'static str,
    ) -> Result<Option<(&'static str, Bot)>, ()> {
        let b = receiver.await.map_err(|_| ())?;
        Ok(b?.map(|b| (name, b)))
    }

    let bot = runtime.block_on(async {
        let bots = future::try_join_all(vec![
            bind_name(eval_receiver, "eval"),
            bind_name(cratesio_receiver, "cratesio"),
            bind_name(rustdoc_receiver, "rustdoc"),
        ])
        .await
        .unwrap();
        let bots = bots.into_iter().flatten().collect_vec();
        let mut start_msg = format!("Start version: {}", env!("VERSION"));
        for (name, bot) in bots.iter() {
            write!(&mut start_msg, "\nbot {} @{}", name, bot.username).unwrap();
        }
        let (_, first_bot) = bots.into_iter().next().expect("no bot configured?");
        send_message_to_admin(&first_bot, start_msg).await.unwrap();
        first_bot
    });

    runtime.block_on(async move {
        waiter.wait().await;
        // Send the final message.
        send_message_to_admin(&bot, "bye".to_string())
            .await
            .unwrap();
    });
}

fn init_logger() {
    let env = env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info");
    env_logger::Builder::from_env(env)
        .format(|buf, record| {
            let timestamp = buf.timestamp();
            let level = record.level();
            let level_style = buf.default_level_style(level);
            let write_header = write!(buf, "{:>5} {}: ", level_style.value(level), timestamp);
            let write_module_path = match record.module_path() {
                None => Ok(()),
                Some(mut module_path) => {
                    const THIS_MODULE: &str = module_path!();
                    if module_path.starts_with(THIS_MODULE) {
                        let stripped = &module_path[THIS_MODULE.len()..];
                        if stripped.is_empty() || stripped.starts_with("::") {
                            module_path = stripped;
                        }
                    }
                    if module_path.is_empty() {
                        Ok(())
                    } else {
                        write!(buf, "{}: ", module_path)
                    }
                }
            };
            let write_args = writeln!(buf, "{}", record.args());
            write_header.and(write_module_path).and(write_args)
        })
        .init();
}

fn build_client() -> Client {
    use reqwest::header::{HeaderMap, USER_AGENT};
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, env!("USER_AGENT").parse().unwrap());
    Client::builder().default_headers(headers).build().unwrap()
}

fn report_error_to_admin(bot: &Bot, error: &Error) {
    use htmlescape::encode_minimal;
    let message = match error {
        Error::Parse(bot::ParseError { data, error }) => format!(
            "parse failed: {:?}\n<pre>{}</pre>",
            encode_minimal(&format!("{:?}", error)),
            encode_minimal(&String::from_utf8_lossy(data)),
        ),
        _ => encode_minimal(&format!("{:?}", error)),
    };
    tokio::spawn(send_message_to_admin(bot, message));
}

fn send_message_to_admin(bot: &Bot, msg: String) -> impl Future<Output = Result<(), ()>> {
    let chat_id = ChatId(ADMIN_ID.0);
    bot.send_message(chat_id, msg)
        .execute()
        .map_ok(|_| ())
        .map_err(|e| error!("failed to send message to admin: {:?}", e))
}
