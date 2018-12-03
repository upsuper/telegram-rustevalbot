if [[ "$TRAVIS_OS_NAME" == "linux" && "$TRAVIS_RUST_VERSION" == "stable" ]]
then
    strip target/release/telegram-rustevalbot
    cp target/release/telegram-rustevalbot /tmp/
fi
