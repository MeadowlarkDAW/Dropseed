use dropseed_plugin_api::{ext::timer::TimerID, PluginInstanceID};
use fnv::FnvHashMap;
use hierarchical_hash_wheel_timer::wheels::cancellable::{
    CancellableTimerEntry, QuadWheelWithOverflow,
};
use hierarchical_hash_wheel_timer::wheels::Skip;
use std::hash::Hash;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TimerEntryKey {
    pub plugin_id: u64,
    pub timer_id: TimerID,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TimerEntry {
    pub key: TimerEntryKey,
    interval: Duration,
}

impl CancellableTimerEntry for TimerEntry {
    type Id = TimerEntryKey;
    fn id(&self) -> &Self::Id {
        &self.key
    }
}

pub(crate) struct HostTimer {
    entries: FnvHashMap<TimerEntryKey, Rc<TimerEntry>>,
    wheel: QuadWheelWithOverflow<TimerEntry>,
    next_expected_tick_instant: Option<Instant>,
}

impl HostTimer {
    pub fn new() -> Self {
        Self {
            entries: FnvHashMap::default(),
            wheel: QuadWheelWithOverflow::new(),
            next_expected_tick_instant: None,
        }
    }

    pub fn insert(&mut self, plugin_id: u64, timer_id: TimerID, interval_ms: u32) {
        let key = TimerEntryKey { plugin_id, timer_id };

        let interval = Duration::from_millis(u64::from(interval_ms));

        let new_entry = Rc::new(TimerEntry { key, interval });

        if self.entries.insert(key, Rc::clone(&new_entry)).is_some() {
            if let Err(e) = self.wheel.cancel(&key) {
                log::error!("Unexpected error while cancelling timer: {:?}", e);
            }
        }

        if let Err(e) = self.wheel.insert_ref_with_delay(new_entry, interval) {
            self.entries.remove(&key);
            log::error!("Unexpected error while inserting entry into timer: {:?}", e);
        }
    }

    pub fn remove(&mut self, plugin_id: u64, timer_id: TimerID) {
        let key = TimerEntryKey { plugin_id, timer_id };

        if self.entries.remove(&key).is_some() {
            if let Err(e) = self.wheel.cancel(&key) {
                log::error!("Unexpected error while cancelling timer: {:?}", e);
            }
        }
    }

    pub fn remove_all_with_plugin(&mut self, plugin_id: u64) {
        let mut entries_to_remove: Vec<TimerEntryKey> = self
            .entries
            .iter()
            .filter_map(|(key, _)| if key.plugin_id == plugin_id { Some(*key) } else { None })
            .collect();

        for key in entries_to_remove.drain(..) {
            self.remove(key.plugin_id, key.timer_id);
        }
    }

    pub fn advance(&mut self) -> Option<(Vec<Rc<TimerEntry>>, Instant)> {
        if self.entries.is_empty() {
            self.next_expected_tick_instant = None;
            return None;
        }

        // Calculate much time has passed since the expected instant of the next tick.
        let expected_tick_instant =
            self.next_expected_tick_instant.unwrap_or_else(|| Instant::now());
        let time_elapsed = Instant::now().duration_since(expected_tick_instant);

        // Calculate how many ticks we need to run (how many milliseconds have passed since
        // the last time we ticked/skipped).
        let num_ticks = 1 + ((time_elapsed.as_secs_f64() * 1_000.0).floor() as u64);

        // Tick through the timer wheel and collect all the the entries that have elapsed.
        let mut elapsed_entries: Vec<Rc<TimerEntry>> = Vec::new();
        for _ in 0..num_ticks {
            elapsed_entries.append(&mut self.wheel.tick());
        }

        // Re-schedule the entries which have elapsed so they are periodic (as apposed to one-shot).
        for entry in elapsed_entries.iter() {
            if let Err(e) = self.wheel.insert_ref_with_delay(Rc::clone(entry), entry.interval) {
                log::error!("Unexpected error while re-scheduling event in timer: {:?}", e);
            }
        }

        // Calculate how many ticks until the next event.
        let mut num_ticks_to_next_event = num_ticks;
        if let Skip::Millis(ms) = self.wheel.can_skip() {
            // The timer wheel has this many milliseconds (ticks) it can skip without
            // triggering an event, so advance the timer by that many ticks.
            self.wheel.skip(ms);
            num_ticks_to_next_event += u64::from(ms);
        }

        self.next_expected_tick_instant =
            Some(expected_tick_instant + Duration::from_millis(num_ticks_to_next_event));

        Some((elapsed_entries, self.next_expected_tick_instant.unwrap()))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
