use crate::bot::Bot;
use futures::future::Either;
use futures::{Future, IntoFuture};
use htmlescape::encode_minimal;
use itertools::Itertools;
use log::warn;
use percent_encoding::{utf8_percent_encode, PATH_SEGMENT_ENCODE_SET};
use reqwest::r#async::Client;
use reqwest::IntoUrl;
use serde::Deserialize;
use std::borrow::Cow;
use telegram_types::bot::inline_mode::{
    InlineQueryResult, InlineQueryResultArticle, InputMessageContent, InputTextMessageContent,
    ResultId,
};
use telegram_types::bot::types::{
    InlineKeyboardButton, InlineKeyboardButtonPressed, InlineKeyboardMarkup, ParseMode, Update,
    UpdateContent,
};
use url::Url;

pub struct CratesioBot {
    client: Client,
    bot: Bot,
}

impl CratesioBot {
    pub fn new(client: Client, bot: Bot) -> Self {
        info!("CratesioBot authorized as @{}", bot.username);
        CratesioBot { client, bot }
    }

    pub fn handle_update(&self, update: Update) -> impl Future<Item = (), Error = ()> {
        let query = match update.content {
            UpdateContent::InlineQuery(query) => query,
            _ => return Either::A(Ok(()).into_future()),
        };
        let result = if query.query.is_empty() {
            Either::A(
                self.generate_results("https://crates.io/api/v1/summary", |resp: Summary| {
                    resp.most_recently_downloaded
                }),
            )
        } else {
            let mut url = Url::parse("https://crates.io/api/v1/crates").unwrap();
            url.query_pairs_mut()
                .append_pair("q", &query.query)
                .append_pair("sort", "relevance")
                .append_pair("per_page", "50");
            Either::B(self.generate_results(url, |resp: Crates| resp.crates))
        };
        let bot = self.bot.clone();
        let future = result
            .map_err(|e| warn!("failed to get results: {:?}", e))
            .and_then(move |r| {
                bot.answer_inline_query(query.id, &r)
                    .execute()
                    .map(|_| ())
                    .map_err(|e| warn!("failed to answer query: {:?}", e))
            });
        Either::B(future)
    }

    fn generate_results<T>(
        &self,
        url: impl IntoUrl,
        get_crates: impl FnOnce(T) -> Vec<Crate>,
    ) -> impl Future<Item = Vec<InlineQueryResult<'static>>, Error = reqwest::Error>
    where
        for<'de> T: Deserialize<'de>,
    {
        self.client
            .get(url)
            .send()
            .and_then(|resp| resp.error_for_status())
            .and_then(|mut resp| resp.json())
            .map(move |resp: T| {
                get_crates(resp)
                    .into_iter()
                    .map(|c| c.into_inline_query_result())
                    .collect()
            })
    }
}

#[derive(Debug, Deserialize)]
struct Summary {
    most_recently_downloaded: Vec<Crate>,
}

#[derive(Debug, Deserialize)]
struct Crates {
    crates: Vec<Crate>,
}

#[derive(Debug, Deserialize)]
struct Crate {
    id: String,
    name: String,
    description: Option<String>,
    max_version: String,
    documentation: Option<String>,
    repository: Option<String>,
}

impl Crate {
    fn into_inline_query_result(self) -> InlineQueryResult<'static> {
        let description: Option<Cow<'_, str>> = self
            .description
            .map(|d| d.split_whitespace().join(" ").into());
        let title = format!("{} {}", self.name, self.max_version);
        let mut message = format!(
            "<b>{}</b> ({})",
            encode_minimal(&self.name),
            encode_minimal(&self.max_version)
        );
        if let Some(description) = &description {
            message.push_str(" - ");
            message.push_str(&description);
        }

        let name_url = encode_for_url(&self.name);
        let crate_url = format!("https://crates.io/crates/{}", name_url);
        let doc_url = self
            .documentation
            .unwrap_or_else(|| format!("https://docs.rs/crate/{}", name_url));
        let mut buttons = vec![
            InlineKeyboardButton {
                text: "info".to_string(),
                pressed: InlineKeyboardButtonPressed::Url(crate_url),
            },
            InlineKeyboardButton {
                text: "doc".to_string(),
                pressed: InlineKeyboardButtonPressed::Url(doc_url),
            },
        ];
        if let Some(repo) = self.repository {
            buttons.push(InlineKeyboardButton {
                text: "repo".to_string(),
                pressed: InlineKeyboardButtonPressed::Url(repo),
            });
        }

        InlineQueryResult::Article(InlineQueryResultArticle {
            id: ResultId(self.id),
            title: title.into(),
            input_message_content: InputMessageContent::Text(InputTextMessageContent {
                message_text: message.into(),
                parse_mode: Some(ParseMode::HTML),
                disable_web_page_preview: Some(true),
            }),
            reply_markup: Some(InlineKeyboardMarkup {
                inline_keyboard: vec![buttons],
            }),
            url: None,
            hide_url: None,
            description,
            thumb_url: None,
            thumb_width: None,
            thumb_height: None,
        })
    }
}

fn encode_for_url(s: &str) -> String {
    utf8_percent_encode(s, PATH_SEGMENT_ENCODE_SET).collect()
}
