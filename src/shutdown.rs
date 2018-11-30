use futures::sync::oneshot::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Shutdown(Mutex<Option<Sender<()>>>);

impl Shutdown {
    pub fn new() -> Arc<Self> {
        Arc::new(Shutdown(Default::default()))
    }

    pub fn renew(&self) -> Receiver<()> {
        let (sender, receiver) = channel();
        *self.0.lock().unwrap() = Some(sender);
        receiver
    }

    pub fn shutdown(&self) {
        self.0.lock().unwrap().take().unwrap().send(()).unwrap();
    }
}
