# Telegram Rust eval bot

[![Build Status](https://api.travis-ci.org/upsuper/telegram-rustevalbot.svg?branch=master)](https://travis-ci.org/upsuper/telegram-rustevalbot)
[![Coverage Status](https://coveralls.io/repos/github/upsuper/telegram-rustevalbot/badge.svg?branch=master)](https://coveralls.io/github/upsuper/telegram-rustevalbot?branch=master)

This bot program handles Eval bot and Crates.io bot.

## Eval bot

This is a Telegram bot for simple query of Rust-related stuff,
inspired by [Playbot NG](https://github.com/panicbit/playbot_ng) on IRC.

The official instance of this bot is [@rustevalbot](https://t.me/rustevalbot),
and it has [a testing channel](https://t.me/rustevalbot_test).

### Usage

Interaction with eval bot is done either
via commands on Telegram channel that it joins,
or by talking directly to it.

All command which recognized by it can be edited.
The reply of such command would be updated automatically as well.
If the edited command is no longer recognized,
the reply from the bot will be removed.
(But removing the original command wouldn't remove the reply,
because Telegram doesn't send update for such event.)

#### Code evaluation

Eval bot evaluates Rust code for command `/eval`,
e.g.  `/eval Vec::<usize>::new().as_ptr()`.
This is backed by [Rust Playground](https://play.rust-lang.org/).

By default, the code is wrapped in a template that prints
the result of the expression via its `Debug` impl.
This behavior can be changed via `--bare` flag.

There are a few flags that can modify the behavior of the evaluation.
You can select the channel using
`--stable` (default), `--beta`, or `--nightly`.
You can pick the build profile using `--debug` (default) or `--release`.
To make the bot evaluate code as is, use `--bare`.

For convenience, inner attributes and `extern crate`s
at the beginning of code are moved to the beginning of the wrapped code
when `--bare` is not used.

#### Other commands

You can use `/help` command when talking to it directly to query other
commands available.

All command also support `--help` flag to display help information.

## Crates.io bot

This is a Telegram bot running under inline mode
for helping people find and send details of crates on crates.io.

The official instance of this bot is [@cratesiobot](https://t.me/cratesiobot).

It can be used everywhere with a message in format `@cratesiobot <crate>`.
Telegram would should a menu for user to select crate from.

## Rust doc bot

This is a Telegram bot running under inline mode
for helping people find and send details of items in Rust official document.

The official instance of this bot is [@rustdocbot](https://t.me/rustdocbot).

It can be used everywhere with a message in format `@rustdocbot <crate>`.
Telegram would should a menu for user to select item from.

This replaces the `/doc` command previously available in the Eval bot.

## Configuration

This bot program uses environment variable for config,
and also recognize `.env` file in the working directory.

Bot tokens can be specified via:
* `EVAL_TELEGRAM_TOKEN`: the Eval bot
* `CRATESIO_TELEGRAM_TOKEN`: the Crates.io bot
* `RUSTDOC_TELEGRAM_TOKEN`: the Rust doc bot

At least one of the tokens must be provided.
The bot tokens can be created with [@BotFather](https://t.me/BotFather).

It also requires `BOT_ADMIN_ID` to be specified as the User ID of admin account.
The bot program will send message to the account when it starts and stops
via the first configured bot in the order of the list above.

If the Eval bot is enabled,
admin would be able to use `/shutdown` command on that bot to stop the program.

For the User ID, one can get their own User ID
via [@userinfobot](https://t.me/userinfobot)
or [@JsonDumpBot](https://t.me/JsonDumpBot).

The Eval bot requires write permission to `record_list.json`
in the current directory in order to persist command information
across reboot for command editing.

### Upgrade

This bot listens on `upgrade` file in the current directory,
and when the file gets written,
the bot will quit itself as if a `/shutdown` command is sent.
