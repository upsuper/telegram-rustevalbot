use crate::shutdown::Shutdown;
use log::{debug, info};
use notify::{self, DebouncedEvent, RecursiveMode, Watcher};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

const NOTIFY_FILE: &str = "upgrade";

pub fn init(shutdown: Arc<Shutdown>) {
    let (tx, rx) = mpsc::channel();
    let watcher = init_watcher(tx).expect("failed to init upgrade watcher");
    thread::spawn(move || {
        watch_notify_file(&watcher, &rx, &shutdown);
    });
}

fn init_watcher(tx: Sender<DebouncedEvent>) -> notify::Result<impl Watcher> {
    let mut watcher = notify::watcher(tx, Default::default())?;
    watcher.watch(NOTIFY_FILE, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

fn watch_notify_file(
    _watcher: &impl Watcher,
    rx: &Receiver<DebouncedEvent>,
    shutdown: &Shutdown,
) {
    for event in rx.iter() {
        debug!("notify: {:?}", event);
        if let DebouncedEvent::NoticeWrite(_) = event {
            info!("notify detected");
            shutdown.shutdown();
            break;
        }
    }
}
