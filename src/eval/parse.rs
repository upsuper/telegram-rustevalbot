use combine::error::StringStreamError;
use combine::parser::char::{alpha_num, space, spaces, string};
use combine::parser::choice::choice;
use combine::parser::combinator::attempt;
use combine::parser::range::recognize;
use combine::parser::repeat::{many, skip_many1};
use combine::parser::token::eof;
use combine::parser::Parser;
use serde::Serialize;
use std::fmt::Write as _;

pub fn parse_command(command: &str) -> Option<(Flags, &str)> {
    let spaces1 = || (space(), spaces()).map(|_| ());
    let flag_name = recognize(skip_many1(alpha_num()));
    let flag = (spaces1(), string("--"), flag_name).map(|(_, _, name)| name);
    let mut parser = string("/eval")
        .with(many::<FlagsBuilder, _, _>(attempt(flag)))
        .skip(choice((spaces1(), eof())))
        .and_then(|builder| {
            if builder.error {
                Err(StringStreamError::UnexpectedParse)
            } else {
                Ok(builder.flags)
            }
        });
    parser.parse(command).ok()
}

pub fn get_help_message() -> String {
    let mut result = String::new();
    for info in FLAG_INFO.iter() {
        writeln!(
            result,
            "<code>--{}</code> - {}",
            info.name, info.description
        )
        .unwrap();
    }
    result
}

#[derive(Default)]
struct FlagsBuilder {
    flags: Flags,
    error: bool,
}

impl<'a> Extend<&'a str> for FlagsBuilder {
    fn extend<T: IntoIterator<Item = &'a str>>(&mut self, iter: T) {
        for name in iter {
            match FLAG_INFO.iter().find(|info| info.name == name) {
                Some(info) => (info.setter)(&mut self.flags),
                None => self.error = true,
            }
        }
    }
}

struct FlagInfo {
    name: &'static str,
    description: &'static str,
    setter: fn(&mut Flags),
}

const FLAG_INFO: &[FlagInfo] = &[
    FlagInfo {
        name: "stable",
        description: "use stable channel",
        setter: |flags| flags.channel = Some(Channel::Stable),
    },
    FlagInfo {
        name: "beta",
        description: "use beta channel",
        setter: |flags| flags.channel = Some(Channel::Beta),
    },
    FlagInfo {
        name: "nightly",
        description: "use nightly channel",
        setter: |flags| flags.channel = Some(Channel::Nightly),
    },
    FlagInfo {
        name: "2015",
        description: "use 2015 edition",
        setter: |flags| flags.edition = Some("2015"),
    },
    FlagInfo {
        name: "2018",
        description: "use 2018 edition",
        setter: |flags| flags.edition = Some("2018"),
    },
    FlagInfo {
        name: "debug",
        description: "do debug build",
        setter: |flags| flags.mode = Some(Mode::Debug),
    },
    FlagInfo {
        name: "release",
        description: "do release build",
        setter: |flags| flags.mode = Some(Mode::Release),
    },
    FlagInfo {
        name: "bare",
        description: "don't add any wrapping code",
        setter: |flags| flags.bare = true,
    },
    FlagInfo {
        name: "version",
        description: "show version instead of running code",
        setter: |flags| flags.version = true,
    },
    FlagInfo {
        name: "help",
        description: "show this help information",
        setter: |flags| flags.help = true,
    },
];

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Flags {
    pub channel: Option<Channel>,
    pub edition: Option<&'static str>,
    pub mode: Option<Mode>,
    pub bare: bool,
    pub version: bool,
    pub help: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Debug,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Nightly => "nightly",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_command, Channel, Flags, Mode};

    #[test]
    fn unknown_command() {
        assert_eq!(parse_command("/unknown"), None);
    }

    #[test]
    fn command_with_nothing() {
        assert_eq!(parse_command("/eval"), Some((Flags::default(), "")));
    }

    #[test]
    fn command_with_content() {
        assert_eq!(
            parse_command("/eval something after"),
            Some((Flags::default(), "something after"))
        );
    }

    #[test]
    fn command_with_content_newline() {
        assert_eq!(
            parse_command("/eval\nsome content"),
            Some((Flags::default(), "some content")),
        );
    }

    #[test]
    fn unknown_flag() {
        assert_eq!(parse_command("/eval --unknown"), None);
    }

    #[test]
    fn channel_flags() {
        const CHANNELS: &[(&str, Channel)] = &[
            ("stable", Channel::Stable),
            ("beta", Channel::Beta),
            ("nightly", Channel::Nightly),
        ];
        for (name, channel) in CHANNELS.iter() {
            let expected_flags = Flags {
                channel: Some(*channel),
                ..Flags::default()
            };
            assert_eq!(
                parse_command(&format!("/eval --{}", name)),
                Some((expected_flags, "")),
            );
        }
    }

    #[test]
    fn edition_flags() {
        const EDITIONS: &[&str] = &["2015", "2018"];
        for edition in EDITIONS.iter() {
            let expected_flags = Flags {
                edition: Some(*edition),
                ..Flags::default()
            };
            assert_eq!(
                parse_command(&format!("/eval --{}", edition)),
                Some((expected_flags, "")),
            );
        }
    }

    #[test]
    fn mode_flags() {
        const MODES: &[(&str, Mode)] = &[("debug", Mode::Debug), ("release", Mode::Release)];
        for (name, mode) in MODES.iter() {
            let expected_flags = Flags {
                mode: Some(*mode),
                ..Flags::default()
            };
            assert_eq!(
                parse_command(&format!("/eval --{}", name)),
                Some((expected_flags, "")),
            );
        }
    }

    #[test]
    fn bare_flag() {
        let expected_flags = Flags {
            bare: true,
            ..Flags::default()
        };
        assert_eq!(parse_command("/eval --bare"), Some((expected_flags, "")),);
    }

    #[test]
    fn version_flag() {
        let expected_flags = Flags {
            version: true,
            ..Flags::default()
        };
        assert_eq!(parse_command("/eval --version"), Some((expected_flags, "")));
    }

    #[test]
    fn help_flag() {
        let expected_flags = Flags {
            help: true,
            ..Flags::default()
        };
        assert_eq!(parse_command("/eval --help"), Some((expected_flags, "")));
    }

    #[test]
    fn flags_without_sep() {
        assert_eq!(parse_command("/eval --stable--2015"), None);
    }

    #[test]
    fn content_and_multiple_flags() {
        let input = "/eval\n--stable --bare\n--version --nightly --debug --2015\nrest\ncontent";
        let expected_flags = Flags {
            channel: Some(Channel::Nightly),
            mode: Some(Mode::Debug),
            edition: Some("2015"),
            bare: true,
            version: true,
            help: false,
        };
        assert_eq!(
            parse_command(input),
            Some((expected_flags, "rest\ncontent"))
        );
    }
}
