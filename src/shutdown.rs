use futures::sync::oneshot::{channel, Receiver, Sender};
use std::sync::Mutex;

#[derive(Default)]
pub struct Shutdown(Mutex<Option<Sender<Option<i64>>>>);

impl Shutdown {
    pub fn renew(&self) -> Receiver<Option<i64>> {
        let (sender, receiver) = channel();
        *self.0.lock().unwrap() = Some(sender);
        receiver
    }

    pub fn shutdown(&self, id: Option<i64>) {
        self.0.lock().unwrap().take().unwrap().send(id).unwrap();
    }
}
