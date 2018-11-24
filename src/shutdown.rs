use futures::sync::oneshot::{channel, Receiver, Sender};
use std::sync::Mutex;
use telegram_types::bot::types::UpdateId;

#[derive(Default)]
pub struct Shutdown(Mutex<Option<Sender<Option<UpdateId>>>>);

impl Shutdown {
    pub fn renew(&self) -> Receiver<Option<UpdateId>> {
        let (sender, receiver) = channel();
        *self.0.lock().unwrap() = Some(sender);
        receiver
    }

    pub fn shutdown(&self, id: Option<UpdateId>) {
        self.0.lock().unwrap().take().unwrap().send(id).unwrap();
    }
}
