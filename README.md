# Telegram Rust eval bot

[![Build Status](https://api.travis-ci.org/upsuper/telegram-rustevalbot.svg?branch=master)](https://travis-ci.org/upsuper/telegram-rustevalbot)

This is a Telegram bot for simple query of Rust-related stuff,
inspired by [Playbot NG](https://github.com/panicbit/playbot_ng) on IRC.

The official instance of this bot is [@rustevalbot](https://t.me/rustevalbot),
and it has [a testing channel](https://t.me/rustevalbot_test).

## Usage

Interaction with eval bot is done either
via commands on Telegram channel that it joins,
or by talking directly to it.

All command which recognized by it can be edited.
The reply of such command would be updated automatically as well.
If the edited command is no longer recognized,
the reply from the bot will be removed.
(But removing the original command wouldn't remove the reply,
because Telegram doesn't send update for such event.)

### Code evaluation

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

### Crate query

Eval bot can display information about a crate on crates.io
via `/crate <crate>` command, e.g. `/crate itertools`.

This command supports two other modes via `--keyword` and `--query`.
With one of the flag specified,
it would use the argument as keyword or general query string,
and list the first several items returned from crates.io.

### Document query

Eval can can display document item from official document
via `/doc <query>` command, e.g. `/crate hashmap`.

### Other commands

You can use `/help` command when talking to it directly to query other
commands available.

All command also support `--help` flag to display help information.

## Configuration

This bot uses environment variable for config,
and also recognize `.env` file in the working directory.

`TELEGRAM_TOKEN` is required to run the bot.
It can be created with [@BotFather](https://t.me/BotFather).

In addition, it accepts `BOT_ADMIN_ID`, the User ID of admin account.
(Note: You can get your own User ID
via [@userinfobot](https://t.me/userinfobot)
or [@JsonDumpBot](https://t.me/JsonDumpBot).)
With this specified,
the bot will send message to this account when it starts and stops.
Admin would also be able to use `/shutdown` command to stop it.

This bot requires write permission to `record_list.json`
in the current directory in order to persist command information
across reboot for command editing.

### Upgrade

This bot listens on `upgrade` file in the current directory,
and when the file gets written,
the bot will quit itself as if a `/shutdown` command is sent.
