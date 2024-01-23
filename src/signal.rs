use crate::shutdown::Shutdown;
use log::info;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::sync::Arc;
use std::thread;

pub fn init(shutdown: Arc<Shutdown>) {
    let mut signals = Signals::new([SIGINT, SIGTERM]).expect("failed to init signal handler");
    thread::spawn(move || {
        if let Some(signal) = signals.forever().next() {
            info!("signal: {}", signal);
            assert!(matches!(signal, SIGINT | SIGTERM));
            shutdown.shutdown();
        }
    });
}
