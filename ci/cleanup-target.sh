#!/bin/sh

rm -rf target/{debug,release}/{,lib}telegram-rustevalbot*
rm -rf target/{debug,release}/telegram_rustevalbot-*
rm -rf target/{debug,release}/{build,.fingerprint}/telegram-rustevalbot-*
rm -rf target/{debug,release}/deps/telegram_rustevalbot-*
rm -rf target/*/{release,debug}/.fingerprint/telegram-rustevalbot-*
rm -rf target/debug/incremental
