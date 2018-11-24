use futures::future::Either;
use futures::{Async, Future, IntoFuture, Poll, Stream};
use reqwest;
use reqwest::r#async::{Client, Request};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::time::Duration;
use telegram_types::bot::methods::{
    ApiError, ChatTarget, DeleteMessage, EditMessageText, GetMe, GetUpdates, Method, SendMessage,
    TelegramResult,
};
use telegram_types::bot::types::{ChatId, Message, MessageId, ParseMode, Update, UpdateId};
use tokio_timer::timeout::{self, Timeout};
use tokio_timer::Error as TimerError;

/// Telegram bot
#[derive(Clone)]
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

    pub fn get_updates<'s>(&'s self) -> impl Stream<Item = Update, Error = Error> + 's {
        UpdateStream {
            bot: self,
            update_id: None,
            buffer: VecDeque::new(),
            current_request: None,
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
    ) -> BotRequest<Message> {
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
    ) -> BotRequest<Message> {
        let edit_message = EditMessageText::new(ChatTarget::id(chat_id.0), message_id, text)
            .parse_mode(ParseMode::HTML)
            .disable_preview();
        self.build_request(&edit_message)
    }

    pub fn delete_message(&self, chat_id: ChatId, message_id: MessageId) -> BotRequest<bool> {
        let delete_message = DeleteMessage {
            chat_id: ChatTarget::id(chat_id.0),
            message_id,
        };
        self.build_request(&delete_message)
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
    for<'de> T: Deserialize<'de>,
{
    pub fn execute(self) -> impl Future<Item = T, Error = Error> {
        let req = match self.request {
            Ok(req) => req,
            Err(err) => return Either::B(Err(err.into()).into_future()),
        };
        let future = self
            .client
            .execute(req)
            .and_then(|resp| resp.error_for_status())
            .and_then(|mut resp| resp.json())
            .map_err(Error::from)
            .and_then(|result: TelegramResult<T>| Ok(Into::<Result<_, _>>::into(result)?));
        Either::A(future)
    }
}

#[derive(Debug)]
pub enum Error {
    Request(reqwest::Error),
    Api(ApiError),
    Timer(TimerError),
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

struct UpdateStream<'a> {
    bot: &'a Bot,
    update_id: Option<UpdateId>,
    buffer: VecDeque<Update>,
    current_request: Option<PendingFuture>,
}

type PendingFuture = Box<dyn Future<Item = Vec<Update>, Error = timeout::Error<Error>>>;

const TELEGRAM_TIMEOUT_SECS: u16 = 5;

impl<'a> Stream for UpdateStream<'a> {
    type Item = Update;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            if let Some(update) = self.buffer.pop_front() {
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
                        self.update_id = Some(UpdateId(last_update.update_id.0 + 1));
                    }
                    self.buffer = VecDeque::from(updates);
                }
                Ok(Async::NotReady) => {
                    self.current_request = Some(request);
                    break Ok(Async::NotReady);
                }
                Err(err) => {
                    if err.is_inner() {
                        break Err(err.into_inner().unwrap());
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
