use crate::shutdown::Shutdown;
use log::{debug, info};
use notify::{self, Event, EventKind, RecommendedWatcher, RecursiveMode, Result, Watcher};
use std::path::Path;
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

fn init_watcher(tx: Sender<Result<Event>>) -> Result<impl Watcher> {
    let mut watcher = RecommendedWatcher::new(tx, Default::default())?;
    watcher.watch(Path::new(NOTIFY_FILE), RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

fn watch_notify_file(_watcher: &impl Watcher, rx: &Receiver<Result<Event>>, shutdown: &Shutdown) {
    for event in rx.iter() {
        debug!("notify: {:?}", event);
        if let Ok(Event {
            kind: EventKind::Modify(_),
            ..
        }) = event
        {
            info!("notify detected");
            shutdown.shutdown();
            break;
        }
    }
}
