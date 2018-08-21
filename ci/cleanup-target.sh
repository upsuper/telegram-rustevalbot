#!/bin/sh

cd $TRAVIS_BUILD_DIR/target
rm -rf {debug,release}/{,lib}telegram-rustevalbot*
rm -rf {debug,release}/telegram_rustevalbot-*
rm -rf {debug,release}/{build,.fingerprint}/telegram-rustevalbot-*
rm -rf {debug,release}/deps/telegram_rustevalbot-*
rm -rf */{debug,release}/.fingerprint/telegram-rustevalbot-*
rm -rf debug/incremental
