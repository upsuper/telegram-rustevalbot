use reqwest;
use std::borrow::Cow;
use std::fmt;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Copy, Debug)]
pub enum Void {}

impl fmt::Display for Void {
    fn fmt(&self, _: &mut fmt::Formatter) -> fmt::Result {
        Ok(())
    }
}

pub fn is_separator(c: char) -> bool {
    c.is_whitespace() || c.is_control()
}

pub fn map_reqwest_error(error: &reqwest::Error) -> &'static str {
    if error.is_http() || error.is_redirect() {
        "failed to request"
    } else if error.is_serialization() {
        "failed to parse result"
    } else if error.is_client_error() {
        "client error"
    } else if error.is_server_error() {
        "server error"
    } else {
        "unknown error"
    }
}

pub fn truncate_output(output: &str, max_lines: usize, max_total_columns: usize) -> Cow<str> {
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

/// A `Write` type which counts the total width written into the inner.
pub struct WidthCountingWriter<W: fmt::Write> {
    inner: W,
    width: usize,
}

impl<W: fmt::Write> WidthCountingWriter<W> {
    /// Create a `WidthCountWriter` with an inner `Write`.
    pub fn new(inner: W) -> Self {
        WidthCountingWriter {
            inner,
            width: 0,
        }
    }

    /// Get the total unicode char width written into inner.
    pub fn total_width(&self) -> usize {
        self.width
    }
}

impl<W: fmt::Write> fmt::Write for WidthCountingWriter<W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.width += s.width_cjk();
        self.inner.write_str(s)
    }
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
        const TESTCASES: &[Testcase] = &[
            Testcase {
                input: &[("a", 300)],
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
