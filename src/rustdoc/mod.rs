use self::search::ItemType;
use crate::bot::Bot;
use crate::utils::encode_with_code;
use futures::future::{self, Either, TryFutureExt as _};
use itertools::Itertools;
use log::{info, warn};
use rustdoc_seeker::DocItem;
use sha2::{Digest, Sha256};
use std::future::Future;
use telegram_types::bot::inline_mode::{
    InlineQueryResult, InlineQueryResultArticle, InputMessageContent, InputTextMessageContent,
    ResultId,
};
use telegram_types::bot::types::{ParseMode, Update, UpdateContent};

mod search;

pub use self::search::init;

pub struct RustdocBot {
    bot: Bot,
}

impl RustdocBot {
    pub fn new(bot: Bot) -> Self {
        info!("RustdocBot authorized as @{}", bot.username);
        RustdocBot { bot }
    }

    pub fn handle_update(&self, update: Update) -> impl Future<Output = Result<(), ()>> {
        let query = match update.content {
            UpdateContent::InlineQuery(query) => query,
            _ => return Either::Left(future::ok(())),
        };
        let result = search::query(&query.query)
            .into_iter()
            .take(50)
            .map(doc_item_to_result)
            .collect_vec();
        let future = self
            .bot
            .answer_inline_query(query.id, &result)
            .execute()
            .map_ok(|_| ())
            .map_err(|e| warn!("failed to answer query: {:?}", e));
        Either::Right(future)
    }
}

fn doc_item_to_result(item: &DocItem) -> InlineQueryResult<'static> {
    let url = {
        let mut result = "https://doc.rust-lang.org/".to_string();
        item.fmt_url(&mut result).unwrap();
        result
    };
    let item_type = ItemType::from(&item.name);
    let path = {
        let mut result = String::new();
        if !item_type.is_keyword_or_primitive() {
            let is_parent_keyword_or_primitive = item
                .parent
                .as_ref()
                .map_or(false, |p| ItemType::from(p).is_keyword_or_primitive());
            if !is_parent_keyword_or_primitive {
                result.push_str(item.path.as_ref());
                result.push_str("::");
            }
        }
        if let Some(parent) = &item.parent {
            result.push_str(parent.as_ref());
            result.push_str("::");
        }
        result.push_str(item.name.as_ref());
        if item_type.is_macro() {
            result.push('!');
        }
        result
    };
    let type_str = match item_type {
        ItemType::Keyword => " (keyword)",
        ItemType::Primitive => " (primitive type)",
        _ => "",
    };
    let title = format!("{}{}", path, type_str);
    let description = item.desc.as_ref().to_string();
    // We don't escape path assuming they don't contain any HTML special
    // characters. This is checked in debug assertions in the lazy_static
    // block in `search` mod.
    let mut message = format!(r#"<a href="{}">{}</a>{}"#, url, path, type_str);
    if !description.is_empty() {
        message.push_str(" - ");
        encode_with_code(&mut message, &description);
    }

    let id = format!("{:x}", Sha256::digest(url.as_bytes()));
    InlineQueryResult::Article(InlineQueryResultArticle {
        id: ResultId(id),
        title: title.into(),
        input_message_content: InputMessageContent::Text(InputTextMessageContent {
            message_text: message.into(),
            parse_mode: Some(ParseMode::HTML),
            disable_web_page_preview: Some(true),
        }),
        reply_markup: None,
        url: None,
        hide_url: None,
        description: if description.is_empty() {
            None
        } else {
            Some(description.into())
        },
        thumb_url: None,
        thumb_width: None,
        thumb_height: None,
    })
}
