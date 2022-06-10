use basedrop::Owned;
use rtrb_basedrop::{Consumer, Producer};
use rusty_daw_core::SampleRate;
use std::time::Duration;
use std::{
    mem::MaybeUninit,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
};

use crate::graph::schedule::SharedSchedule;

static PROCESS_THREAD_POLL_INTERVAL: Duration = Duration::from_micros(5);

pub(crate) struct DAWEngineProcessThread {
    to_audio_thread_audio_out_tx: Producer<f32>,
    from_audio_thread_audio_in_rx: Consumer<f32>,

    /// In case there are no inputs, use this to let the engine know when there
    /// are frames to render.
    num_frames_wanted: Option<Arc<AtomicU32>>,

    in_temp_buffer: Owned<Vec<f32>>,
    out_temp_buffer: Owned<Vec<f32>>,

    in_channels: usize,
    out_channels: usize,

    schedule: SharedSchedule,
}

impl DAWEngineProcessThread {
    pub fn new(
        to_audio_thread_audio_out_tx: Producer<f32>,
        from_audio_thread_audio_in_rx: Consumer<f32>,
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

    pub fn run(&mut self, run: Arc<AtomicBool>, max_block_size: u32, sample_rate: SampleRate) {
        /*
        let _rt_priority_handle = match audio_thread_priority::get_current_thread_info() {
            Ok(thread_info) => {
                match audio_thread_priority::promote_thread_to_real_time(
                    thread_info,
                    max_block_size * 16,
                    sample_rate.as_u32(),
                ) {
                    Ok(h) => {
                        log::info!("Successfully promoted process thread to real-time");
                        Some(h)
                    }
                    Err(e) => {
                        log::warn!("Failed to set realtime priority for process thread: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to set realtime priority for process thread: {}", e);
                None
            }
        };
        */

        while run.load(Ordering::Relaxed) {
            let num_frames = if let Some(num_frames_wanted) = &self.num_frames_wanted {
                let num_frames = num_frames_wanted.load(Ordering::SeqCst);

                if num_frames == 0 {
                    std::thread::sleep(PROCESS_THREAD_POLL_INTERVAL);
                    continue;
                }

                num_frames as usize
            } else {
                let num_samples = self.from_audio_thread_audio_in_rx.slots();

                if num_samples == 0 {
                    std::thread::sleep(PROCESS_THREAD_POLL_INTERVAL);
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

            // This is safe because:
            // - self.out_temp_buffer has an allocated length of
            //   (ALLOCATED_FRAMES_PER_CHANNEL * self.out_channels), and `num_frames` will
            //   never be larger than ALLOCATED_FRAMES_PER_CHANNEL
            // - The schedule will always completely fill the buffer.
            unsafe {
                self.out_temp_buffer.set_len(num_frames * self.out_channels);
            }

            self.schedule.process_interleaved(
                &*self.in_temp_buffer,
                self.in_channels,
                &mut *self.out_temp_buffer,
                self.out_channels,
            );

            match self
                .to_audio_thread_audio_out_tx
                .write_chunk_uninit(num_frames * self.in_channels)
            {
                Ok(mut chunk) => {
                    let (slice_1, slice_2) = chunk.as_mut_slices();

                    let out_part = &self.out_temp_buffer[0..slice_1.len()];
                    for i in 0..slice_1.len() {
                        slice_1[i] = MaybeUninit::new(out_part[i]);
                    }

                    let out_part =
                        &self.out_temp_buffer[slice_1.len()..slice_1.len() + slice_2.len()];
                    for i in 0..slice_2.len() {
                        slice_2[i] = MaybeUninit::new(out_part[i]);
                    }

                    unsafe {
                        chunk.commit_all();
                    }
                }
                Err(_) => {
                    log::error!("Ran out of space in engine_to_audio_thread_audio_out buffer");
                    return;
                }
            }
        }
    }
}
