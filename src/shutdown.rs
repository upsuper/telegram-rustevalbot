use futures::sync::oneshot::{channel, Receiver, Sender};
use parking_lot::Mutex;
use std::sync::Arc;

pub struct Shutdown {
    /// Queue of senders for shutdown notification. None if the shutdown
    /// is already notified, and no new senders should be enqueued.
    queue: Mutex<Option<Vec<Sender<()>>>>,
}

impl Shutdown {
    pub fn create() -> Arc<Self> {
        Arc::new(Shutdown {
            queue: Mutex::new(Some(Vec::new())),
        })
    }

    pub fn register(&self) -> Receiver<()> {
        let (sender, receiver) = channel();
        match &mut *self.queue.lock() {
            Some(queue) => queue.push(sender),
            None => sender.send(()).unwrap(),
        }
        receiver
    }

    pub fn shutdown(&self) {
        if let Some(queue) = self.queue.lock().take() {
            for sender in queue {
                // We don't care if the receiver has gone.
                let _ = sender.send(());
            }
        }
    }
}
