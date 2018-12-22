use futures::future::Either;
use futures::sync::oneshot::Receiver;
use futures::{Async, Future, IntoFuture, Poll, Stream};
use log::debug;
use reqwest;
use reqwest::r#async::{Client, Request};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt;
use std::marker::PhantomData;
use std::time::Duration;
use telegram_types::bot::inline_mode::{AnswerInlineQuery, InlineQueryId, InlineQueryResult};
use telegram_types::bot::methods::{
    ApiError, ChatTarget, DeleteMessage, EditMessageText, GetMe, GetUpdates, Method, SendMessage,
    TelegramResult,
};
use telegram_types::bot::types::{ChatId, Message, MessageId, ParseMode, Update, UpdateId};
use tokio_timer::timeout::{self, Timeout};
use tokio_timer::Error as TimerError;

/// Telegram bot
#[derive(Clone, Debug)]
pub struct Bot {
    client: Client,
    token: &'static str,
    /// Telegram username of the bot
    pub username: &'static str,
}

impl Bot {
    pub fn create(client: Client, token: &'static str) -> impl Future<Item = Self, Error = Error> {
        let bot = Bot {
            client,
            token,
            username: "",
        };
        bot.build_request(&GetMe).execute().map(move |user| {
            let username = Box::leak(user.username.expect("No username?").into_boxed_str());
            Bot { username, ..bot }
        })
    }

    pub fn with_client(self, client: Client) -> Self {
        Bot { client, ..self }
    }

    pub fn get_updates(&self, stop_signal: Receiver<()>) -> UpdateStream {
        UpdateStream {
            bot: self.clone(),
            update_id: None,
            buffer: VecDeque::new(),
            current_request: None,
            stop_signal,
        }
    }

    pub fn confirm_update(&self, update_id: UpdateId) -> impl Future<Item = (), Error = Error> {
        let mut get_updates = GetUpdates::new();
        get_updates.offset(UpdateId(update_id.0 + 1));
        self.build_request(&get_updates).execute().map(|_| ())
    }

    pub fn send_message<'a>(
        &self,
        chat_id: ChatId,
        text: impl Into<Cow<'a, str>>,
    ) -> BotRequest<'_, Message> {
        let mut send_message =
            SendMessage::new(ChatTarget::id(chat_id.0), text).parse_mode(ParseMode::HTML);
        send_message.disable_web_page_preview = Some(true);
        self.build_request(&send_message)
    }

    pub fn edit_message<'a>(
        &self,
        chat_id: ChatId,
        message_id: MessageId,
        text: impl Into<Cow<'a, str>>,
    ) -> BotRequest<'_, Message> {
        let edit_message = EditMessageText::new(ChatTarget::id(chat_id.0), message_id, text)
            .parse_mode(ParseMode::HTML)
            .disable_preview();
        self.build_request(&edit_message)
    }

    pub fn delete_message(&self, chat_id: ChatId, message_id: MessageId) -> BotRequest<'_, bool> {
        let delete_message = DeleteMessage {
            chat_id: ChatTarget::id(chat_id.0),
            message_id,
        };
        self.build_request(&delete_message)
    }

    pub fn answer_inline_query(
        &self,
        inline_query_id: InlineQueryId,
        results: &[InlineQueryResult<'_>],
    ) -> BotRequest<'_, bool> {
        let answer = AnswerInlineQuery {
            inline_query_id,
            results: results.into(),
            cache_time: None,
            is_personal: None,
            next_offset: None,
            switch_pm_text: None,
            switch_pm_parameter: None,
        };
        self.build_request(&answer)
    }

    fn build_request<'s, R>(&'s self, request: &R) -> BotRequest<'s, R::Item>
    where
        R: Method + Serialize,
    {
        let request = self.client.post(&R::url(self.token)).json(&request).build();
        BotRequest {
            client: &self.client,
            request,
            phantom: PhantomData,
        }
    }
}

pub struct BotRequest<'a, T> {
    client: &'a Client,
    request: Result<Request, reqwest::Error>,
    phantom: PhantomData<T>,
}

impl<'a, T> BotRequest<'a, T>
where
    T: Send,
    for<'de> T: Deserialize<'de>,
{
    pub fn execute(self) -> impl Future<Item = T, Error = Error> + Send {
        let req = match self.request {
            Ok(req) => req,
            Err(err) => return Either::B(Err(err.into()).into_future()),
        };
        let future = self
            .client
            .execute(req)
            .and_then(|resp| resp.into_body().concat2())
            .map_err(Error::from)
            .and_then(|data| {
                match serde_json::from_slice::<TelegramResult<T>>(&data) {
                    Ok(result) => Ok(Into::<Result<_, _>>::into(result)?),
                    Err(error) => Err(Error::Parse(ParseError {
                        data: data.into_iter().collect(),
                        error,
                    })),
                }
            });
        Either::A(future)
    }
}

#[derive(Debug)]
pub enum Error {
    Request(reqwest::Error),
    Api(ApiError),
    Timer(TimerError),
    Parse(ParseError),
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Request(err)
    }
}

impl From<ApiError> for Error {
    fn from(err: ApiError) -> Self {
        Error::Api(err)
    }
}

impl From<TimerError> for Error {
    fn from(err: TimerError) -> Self {
        Error::Timer(err)
    }
}

pub struct ParseError {
    data: Vec<u8>,
    error: serde_json::Error,
}

impl fmt::Debug for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "ParseError: {}", self.error)
    }
}

pub struct UpdateStream {
    bot: Bot,
    update_id: Option<UpdateId>,
    buffer: VecDeque<Update>,
    current_request: Option<PendingFuture>,
    stop_signal: Receiver<()>,
}

impl UpdateStream {
    pub fn bot(&self) -> &Bot {
        &self.bot
    }
}

type PendingFuture = Box<dyn Future<Item = Vec<Update>, Error = timeout::Error<Error>> + Send>;

const TELEGRAM_TIMEOUT_SECS: u16 = 5;

impl Stream for UpdateStream {
    type Item = Update;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.stop_signal.poll() {
            Ok(Async::Ready(())) => return Ok(Async::Ready(None)),
            Ok(Async::NotReady) => {}
            Err(err) => unreachable!("Shutdown singal dies: {:?}", err),
        }
        loop {
            if let Some(update) = self.buffer.pop_front() {
                debug!("{}: {:?}", self.bot.username, update);
                break Ok(Async::Ready(Some(update)));
            }
            let mut request = self.current_request.take().unwrap_or_else(|| {
                let mut get_updates = GetUpdates::new();
                if let Some(update_id) = self.update_id {
                    get_updates.offset(update_id);
                }
                get_updates.timeout = Some(i32::from(TELEGRAM_TIMEOUT_SECS));
                Box::new(Timeout::new(
                    self.bot.build_request(&get_updates).execute(),
                    Duration::from_secs(u64::from(TELEGRAM_TIMEOUT_SECS)),
                ))
            });
            match request.poll() {
                Ok(Async::Ready(updates)) => {
                    if let Some(last_update) = updates.last() {
                        self.bump_update_id(last_update.update_id);
                    }
                    self.buffer = VecDeque::from(updates);
                }
                Ok(Async::NotReady) => {
                    self.current_request = Some(request);
                    break Ok(Async::NotReady);
                }
                Err(err) => {
                    if err.is_inner() {
                        let error = err.into_inner().unwrap();
                        self.may_recover_from_error(&error);
                        break Err(error);
                    }
                    if err.is_timer() {
                        break Err(err.into_timer().unwrap().into());
                    }
                    // Timeout, loop back and do a new one.
                    debug_assert!(err.is_elapsed());
                }
            }
        }
    }
}

impl UpdateStream {
    fn bump_update_id(&mut self, update_id: UpdateId) {
        self.update_id = Some(UpdateId(update_id.0 + 1));
    }

    fn may_recover_from_error(&mut self, error: &Error) {
        // XXX We should be able to simplify this function once if-let-chain
        // gets stable. See RFC 2497.
        let data = match error {
            Error::Parse(ParseError { data, .. }) => data,
            _ => return,
        };
        let value = match serde_json::from_slice::<JsonValue>(&data) {
            Ok(value) => value,
            Err(_) => return,
        };
        let map = match value {
            JsonValue::Object(map) => map,
            _ => return,
        };
        let ok = map.get("ok").and_then(|v| v.as_bool());
        if !ok.unwrap_or(false) {
            return;
        }
        map.get("result")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.last())
            .and_then(|item| item.as_object())
            .and_then(|map| map.get("update_id"))
            .and_then(|v| v.as_i64())
            .map(|v| self.bump_update_id(UpdateId(v)));
    }
}
