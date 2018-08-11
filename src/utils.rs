use reqwest;

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
