use basedrop::Owned;
use rtrb::{Consumer, Producer};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use crate::graph::shared_pools::SharedSchedule;

use super::audio_thread::AUDIO_THREAD_POLL_INTERVAL;

pub(crate) struct DSEngineProcessThread {
    to_audio_thread_audio_out_tx: Owned<Producer<f32>>,
    from_audio_thread_audio_in_rx: Owned<Consumer<f32>>,

    /// In case there are no inputs, use this to let the engine know when there
    /// are frames to render.
    num_frames_wanted: Option<Arc<AtomicU32>>,

    in_temp_buffer: Owned<Vec<f32>>,
    out_temp_buffer: Owned<Vec<f32>>,

    in_channels: usize,
    out_channels: usize,

    schedule: SharedSchedule,
}

impl DSEngineProcessThread {
    pub fn new(
        to_audio_thread_audio_out_tx: Owned<Producer<f32>>,
        from_audio_thread_audio_in_rx: Owned<Consumer<f32>>,
        num_frames_wanted: Option<Arc<AtomicU32>>,
        in_temp_buffer: Owned<Vec<f32>>,
        out_temp_buffer: Owned<Vec<f32>>,
        in_channels: usize,
        out_channels: usize,
        schedule: SharedSchedule,
    ) -> Self {
        Self {
            to_audio_thread_audio_out_tx,
            from_audio_thread_audio_in_rx,
            num_frames_wanted,
            in_temp_buffer,
            out_temp_buffer,
            in_channels,
            out_channels,
            schedule,
        }
    }

    pub fn run(&mut self, run: Arc<AtomicBool>) {
        #[cfg(target_os = "windows")]
        let spin_sleeper = spin_sleep::SpinSleeper::default();

        while run.load(Ordering::Relaxed) {
            let num_frames = if let Some(num_frames_wanted) = &self.num_frames_wanted {
                let num_frames = num_frames_wanted.load(Ordering::SeqCst);

                if num_frames == 0 {
                    #[cfg(not(target_os = "windows"))]
                    std::thread::sleep(AUDIO_THREAD_POLL_INTERVAL);

                    #[cfg(target_os = "windows")]
                    spin_sleeper.sleep(AUDIO_THREAD_POLL_INTERVAL);

                    continue;
                }

                num_frames as usize
            } else {
                let num_samples = self.from_audio_thread_audio_in_rx.slots();

                if num_samples == 0 {
                    #[cfg(not(target_os = "windows"))]
                    std::thread::sleep(AUDIO_THREAD_POLL_INTERVAL);

                    #[cfg(target_os = "windows")]
                    spin_sleeper.sleep(AUDIO_THREAD_POLL_INTERVAL);

                    continue;
                }

                let chunk = self.from_audio_thread_audio_in_rx.read_chunk(num_samples).unwrap();

                let (slice_1, slice_2) = chunk.as_slices();

                self.in_temp_buffer.clear();
                self.in_temp_buffer.extend_from_slice(slice_1);
                self.in_temp_buffer.extend_from_slice(slice_2);

                chunk.commit_all();

                num_samples / self.in_channels
            };

            self.out_temp_buffer.clear();
            self.out_temp_buffer.resize(num_frames * self.out_channels, 0.0);

            self.schedule.process_interleaved(
                &*self.in_temp_buffer,
                self.in_channels,
                &mut *self.out_temp_buffer,
                self.out_channels,
            );

            match self.to_audio_thread_audio_out_tx.write_chunk(num_frames * self.out_channels) {
                Ok(mut chunk) => {
                    let (slice_1, slice_2) = chunk.as_mut_slices();

                    let out_part = &self.out_temp_buffer[0..slice_1.len()];
                    slice_1.copy_from_slice(&out_part[..slice_1.len()]);

                    let out_part =
                        &self.out_temp_buffer[slice_1.len()..slice_1.len() + slice_2.len()];
                    slice_2.copy_from_slice(&out_part[..slice_2.len()]);

                    chunk.commit_all();
                }
                Err(_) => {
                    log::error!("Ran out of space in engine_to_audio_thread_audio_out buffer");
                    return;
                }
            }
        }
    }
}
