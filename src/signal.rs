use crate::shutdown::Shutdown;
use log::info;
use signal_hook::iterator::Signals;
use signal_hook::SIGTERM;
use std::sync::Arc;
use std::thread;

pub fn init(shutdown: Arc<Shutdown>) {
    let signals = Signals::new(&[SIGTERM]).expect("failed to init signal handler");
    thread::spawn(move || {
        for signal in signals.forever() {
            match signal {
                SIGTERM => {
                    info!("SIGTERM");
                    shutdown.shutdown();
                    break;
                }
                _ => unreachable!(),
            }
        }
    });
}
