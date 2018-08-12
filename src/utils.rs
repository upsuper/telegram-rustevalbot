use reqwest;
use std::fmt;

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

pub fn map_reqwest_error(error: reqwest::Error) -> &'static str {
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
