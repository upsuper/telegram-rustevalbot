use crate::shutdown::Shutdown;
use log::info;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::sync::Arc;
use std::thread;

pub fn init(shutdown: Arc<Shutdown>) {
    let mut signals = Signals::new(&[SIGINT, SIGTERM]).expect("failed to init signal handler");
    thread::spawn(move || {
        for signal in signals.forever() {
            info!("signal: {}", signal);
            match signal {
                SIGINT | SIGTERM => {
                    shutdown.shutdown();
                    break;
                }
                _ => unreachable!(),
            }
        }
    });
}
