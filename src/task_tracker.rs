use log::debug;
use std::future::Future;
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

pub fn create(runtime: &Runtime) -> (Arc<TaskSpawner>, TaskWaiter) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = runtime.handle().clone();
    (
        Arc::new(TaskSpawner { handle, sender }),
        TaskWaiter { receiver },
    )
}

enum TaskState {
    Starting,
    Ended,
}

pub struct TaskSpawner {
    handle: Handle,
    sender: UnboundedSender<TaskState>,
}

impl TaskSpawner {
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let sender = self.sender.clone();
        match sender.send(TaskState::Starting) {
            Ok(()) => {}
            Err(_) => unreachable!("waiter has been dropped"),
        }
        self.handle.spawn(async move {
            future.await;
            match sender.send(TaskState::Ended) {
                Ok(()) => {}
                Err(_) => unreachable!("waiter is dropped before task finishes"),
            }
        });
    }
}

pub struct TaskWaiter {
    receiver: UnboundedReceiver<TaskState>,
}

impl TaskWaiter {
    pub async fn wait(mut self) {
        let mut counter = 0usize;
        loop {
            match self.receiver.recv().await {
                Some(TaskState::Starting) => counter += 1,
                Some(TaskState::Ended) => {
                    counter -= 1;
                    if counter == 0 {
                        debug!("all tasks done");
                        break;
                    }
                }
                None => unreachable!("remaining {} unfinished tasks", counter),
            }
        }
    }
}
