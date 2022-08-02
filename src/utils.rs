use htmlescape::encode_minimal;
use phf::phf_map;
use std::borrow::Cow;
use std::fmt;
use telegram_types::bot::types::{ChatType, Message};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, Debug)]
pub enum Void {}

impl fmt::Display for Void {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

pub fn truncate_output(output: &str, max_lines: usize, max_total_columns: usize) -> Cow<'_, str> {
    let mut line_count = 0;
    let mut column_count = 0;
    for (pos, c) in output.char_indices() {
        column_count += c.width_cjk().unwrap_or(1);
        if column_count > max_total_columns {
            let mut truncate_width = 0;
            for (pos, c) in output[..pos].char_indices().rev() {
                truncate_width += c.width_cjk().unwrap_or(1);
                if truncate_width >= 3 {
                    return format!("{}...", &output[..pos]).into();
                }
            }
        }
        if c == '\n' {
            line_count += 1;
            if line_count == max_lines {
                return format!("{}...", &output[..pos]).into();
            }
        }
    }
    output.into()
}

pub fn is_message_from_private_chat(message: &Message) -> bool {
    matches!(message.chat.kind, ChatType::Private { .. })
}

pub fn encode_with_code(output: &mut String, text: &str) {
    let mut is_code = false;
    for chunk in encode_minimal(text).split('`') {
        if !is_code {
            output.push_str(chunk);
        } else {
            output.push_str("<code>");
            output.push_str(chunk);
            output.push_str("</code>");
        }
        is_code = !is_code;
    }
}

static UNICODE_CHARS_MAP: phf::Map<char, &str> = phf_map! {
    '“' => "\"",
    '”' => "\"",
    '‘' => "\'",
    '’' => "\'",
    '—' => "--"
};

/// Normalize the mistakenly inputted Unicode character
/// to the corresponding ASCII character.
/// 
/// For the table what characters this function will convert,
/// you can refer to [`UNICODE_CHARS_MAP`].
/// 
/// Time complexity of this is `O(n)`.
pub fn normalize_unicode_chars(inputs: &str) -> String {
    let mut output = String::with_capacity(inputs.len());

    for c in inputs.chars() {
        if let Some(replacement) = UNICODE_CHARS_MAP.get(&c) {
            output.push_str(replacement);
        } else {
            output.push(c);
        }
    }

    output
}

#[cfg(test)]
mod test {
    use super::*;

    fn construct_string(parts: &[(&str, usize)]) -> String {
        let len = parts.iter().map(|(s, n)| s.len() * n).sum();
        let mut result = String::with_capacity(len);
        for &(s, n) in parts.iter() {
            for _ in 0..n {
                result.push_str(s);
            }
        }
        result
    }

    #[test]
    fn test_truncate_output() {
        const MAX_LINES: usize = 3;
        const MAX_TOTAL_COLUMNS: usize = MAX_LINES * 72;
        struct Testcase<'a> {
            input: &'a [(&'a str, usize)],
            expected: &'a [(&'a str, usize)],
        }
        const TESTCASES: &[Testcase<'_>] = &[
            Testcase {
                input: &[("a", 216)],
                expected: &[("a", 216)],
            },
            Testcase {
                input: &[("a", 217)],
                expected: &[("a", 213), ("...", 1)],
            },
            Testcase {
                input: &[("啊", 300)],
                expected: &[("啊", 106), ("...", 1)],
            },
            Testcase {
                input: &[("啊", 107), ("a", 5)],
                expected: &[("啊", 106), ("...", 1)],
            },
            Testcase {
                input: &[("a\n", 10)],
                expected: &[("a\n", 2), ("a...", 1)],
            },
        ];
        for Testcase { input, expected } in TESTCASES.iter() {
            assert_eq!(
                truncate_output(&construct_string(input), MAX_LINES, MAX_TOTAL_COLUMNS),
                construct_string(expected)
            );
        }
    }
}
