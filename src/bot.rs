use futures::{Async, Future, IntoFuture, Poll, Stream};
use reqwest;
use reqwest::r#async::Client;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;
use telegram_types::bot::methods::{
    ApiError, ChatTarget, DeleteMessage, EditMessageText, GetMe, GetUpdates, Method, SendMessage,
    TelegramResult,
};
use telegram_types::bot::types::{ChatId, MessageId, ParseMode, Update, UpdateId};
use tokio_timer::timeout::{self, Timeout};
use tokio_timer::Error as TimerError;

/// Telegram bot
#[derive(Clone)]
pub struct Bot {
    client: Client,
    token: &'static str,
    /// Telegram username of the bot
    pub username: Rc<str>,
}

impl Bot {
    pub fn create(
        client: Client,
        token: &'static str,
    ) -> impl Future<Item = Self, Error = Error> {
        request(&client, token, &GetMe).map(move |user| {
            let username = Rc::from(user.username.expect("No username?"));
            Bot {
                client,
                token,
                username,
            }
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
        request(&self.client, self.token, &get_updates).map(|_| ())
    }

    pub fn send_message_to_admin<'a>(
        &self,
        text: impl Into<Cow<'a, str>>,
    ) -> Box<dyn Future<Item = (), Error = Error>> {
        if let Some(id) = *crate::ADMIN_ID {
            let send_message = SendMessage::new(ChatTarget::id(id.0), text);
            Box::new(request(&self.client, self.token, &send_message).map(|_| ()))
        } else {
            Box::new(Ok(()).into_future())
        }
    }

    pub fn send_message<'a>(
        &self,
        chat_id: ChatId,
        text: impl Into<Cow<'a, str>>,
    ) -> Box<dyn Future<Item = MessageId, Error = Error>> {
        let mut send_message =
            SendMessage::new(ChatTarget::id(chat_id.0), text).parse_mode(ParseMode::HTML);
        send_message.disable_web_page_preview = Some(true);
        Box::new(request(&self.client, self.token, &send_message).map(|msg| msg.message_id))
    }

    pub fn edit_message<'a>(
        &self,
        chat_id: ChatId,
        message_id: MessageId,
        text: impl Into<Cow<'a, str>>,
    ) -> Box<dyn Future<Item = (), Error = Error>> {
        let edit_message = EditMessageText::new(ChatTarget::id(chat_id.0), message_id, text)
            .parse_mode(ParseMode::HTML)
            .disable_preview();
        Box::new(request(&self.client, self.token, &edit_message).map(|_| ()))
    }

    pub fn delete_message(
        &self,
        chat_id: ChatId,
        message_id: MessageId,
    ) -> impl Future<Item = (), Error = Error> {
        request(
            &self.client,
            self.token,
            &DeleteMessage {
                chat_id: ChatTarget::id(chat_id.0),
                message_id,
            },
        ).map(|_| ())
    }
}

fn request<R>(
    client: &Client,
    token: &str,
    request: &R,
) -> Box<dyn Future<Item = R::Item, Error = Error>>
where
    R: Method + Serialize,
{
    Box::new(
        client
            .post(&R::url(token))
            .json(&request)
            .send()
            .and_then(|resp| resp.error_for_status())
            .and_then(|mut resp| resp.json())
            .map_err(Error::from)
            .and_then(|result: TelegramResult<R::Item>| {
                Into::<Result<_, _>>::into(result).map_err(Error::from)
            }),
    )
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
                    request(&self.bot.client, self.bot.token, &get_updates),
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
