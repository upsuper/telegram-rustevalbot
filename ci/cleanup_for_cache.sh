if [[ "$TRAVIS_RUST_VERSION" == "nightly" ]]
then
    # Don't cache any build output for nightly.
    rm -rf target
else
    rm -rf target/{debug,release}/telegram-rustevalbot*
    rm -rf target/{debug,release}/telegram_rustevalbot-*
    rm -rf target/{debug,release}/{build,.fingerprint}/telegram-rustevalbot-*
    rm -rf target/{debug,release}/deps/telegram_rustevalbot-*
    rm -rf target/debug/incremental
fi
