use basedrop::Collector;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

static WAIT_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) fn run_garbage_collector_thread(
    mut collector: Collector,
    interval: Duration,
    run: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut last_collect = Instant::now();

        while run.load(Ordering::Relaxed) {
            if last_collect.elapsed() >= interval {
                collector.collect();

                last_collect = Instant::now();

                log::trace!("Garbage collected");
            }

            std::thread::sleep(WAIT_INTERVAL);
        }
    })
}
