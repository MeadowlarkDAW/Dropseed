use basedrop::Owned;
use meadowlark_core_types::SampleRate;
use rtrb::{Consumer, Producer, RingBuffer};
use std::fmt::Debug;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use super::process_thread::DSEngineProcessThread;
use crate::graph::schedule::SharedSchedule;

// Allocate enough for at-least 3 seconds of buffer time.
static ALLOCATED_FRAMES_PER_CHANNEL: usize = 192_000 * 3;

/// Make sure we have a bit of time to copy the engine's output buffer to the
/// audio thread's output buffer.
static COPY_OUT_TIME_WINDOW: f64 = 0.9;

pub struct DSEngineAudioThread {
    to_engine_audio_in_tx: Owned<Producer<f32>>,
    from_engine_audio_out_rx: Owned<Consumer<f32>>,

    in_channels: usize,
    out_channels: usize,

    sample_rate: SampleRate,
    sample_rate_recip: f64,

    /// In case there are no inputs, use this to let the engine know when there
    /// are frames to render.
    num_frames_wanted: Option<Arc<AtomicU32>>,
}

impl Debug for DSEngineAudioThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("DSEngineAudioThread");

        f.field("in_channels", &self.in_channels);
        f.field("out_channels", &self.out_channels);
        f.field("sample_rate", &self.sample_rate);
        f.field("sample_rate_recip", &self.sample_rate_recip);

        f.finish()
    }
}

impl DSEngineAudioThread {
    pub(crate) fn new(
        in_channels: usize,
        out_channels: usize,
        coll_handle: &basedrop::Handle,
        schedule: SharedSchedule,
        sample_rate: SampleRate,
    ) -> (Self, DSEngineProcessThread) {
        let (to_engine_audio_in_tx, from_audio_thread_audio_in_rx) =
            RingBuffer::<f32>::new(in_channels * ALLOCATED_FRAMES_PER_CHANNEL);
        let (to_audio_thread_audio_out_tx, from_engine_audio_out_rx) =
            RingBuffer::<f32>::new(out_channels * ALLOCATED_FRAMES_PER_CHANNEL);

        let to_engine_audio_in_tx = Owned::new(coll_handle, to_engine_audio_in_tx);
        let from_audio_thread_audio_in_rx = Owned::new(coll_handle, from_audio_thread_audio_in_rx);
        let to_audio_thread_audio_out_tx = Owned::new(coll_handle, to_audio_thread_audio_out_tx);
        let from_engine_audio_out_rx = Owned::new(coll_handle, from_engine_audio_out_rx);

        let in_temp_buffer =
            Owned::new(coll_handle, vec![0.0; in_channels * ALLOCATED_FRAMES_PER_CHANNEL]);
        let out_temp_buffer =
            Owned::new(coll_handle, vec![0.0; out_channels * ALLOCATED_FRAMES_PER_CHANNEL]);

        let num_frames_wanted =
            if in_channels == 0 { Some(Arc::new(AtomicU32::new(0))) } else { None };

        let num_frames_wanted_clone = num_frames_wanted.as_ref().map(|n| Arc::clone(n));

        let sample_rate_recip = 1.0 / sample_rate.as_f64();

        (
            Self {
                to_engine_audio_in_tx,
                from_engine_audio_out_rx,
                in_channels,
                out_channels,
                sample_rate,
                sample_rate_recip,
                num_frames_wanted,
            },
            DSEngineProcessThread::new(
                to_audio_thread_audio_out_tx,
                from_audio_thread_audio_in_rx,
                num_frames_wanted_clone,
                in_temp_buffer,
                out_temp_buffer,
                in_channels,
                out_channels,
                schedule,
            ),
        )
    }

    #[cfg(feature = "cpal-backend")]
    pub fn process_cpal_interleaved_output_only<T: cpal::Sample>(
        &mut self,
        cpal_out_channels: usize,
        out: &mut [T],
    ) {
        // TODO: Use some kind of interrupt to activate the thread as apposed
        // to potentially pinning a whole CPU core just waiting for the process
        // thread to finish?
        //
        // Note that I already tried the method of calling `thread::sleep()` for
        // a short amount of time while the process thread is still running, but
        // apparently Windows does not like it when you call `thread::sleep()` on
        // a high-priority thread (underruns galore).

        let clear_output = |out: &mut [T]| {
            for s in out.iter_mut() {
                *s = T::from(&0.0);
            }
        };

        let proc_start_time = Instant::now();

        if out.len() < self.out_channels || cpal_out_channels == 0 {
            clear_output(out);
            return;
        }

        let total_frames = out.len() / cpal_out_channels;

        // Discard any output from previous cycles that failed to render on time.
        if !self.from_engine_audio_out_rx.is_empty() {
            let num_slots = self.from_engine_audio_out_rx.slots();

            let chunks = self.from_engine_audio_out_rx.read_chunk(num_slots).unwrap();
            chunks.commit_all();
        }

        if let Some(num_frames_wanted) = &self.num_frames_wanted {
            num_frames_wanted.store(total_frames as u32, Ordering::SeqCst);
        } else {
            match self.to_engine_audio_in_tx.write_chunk(total_frames * self.in_channels) {
                Ok(mut chunk) => {
                    let (slice_1, slice_2) = chunk.as_mut_slices();
                    slice_1.fill(0.0);
                    slice_2.fill(0.0);

                    chunk.commit_all();
                }
                Err(_) => {
                    log::error!("Ran out of space in audio_thread_to_engine_audio_in buffer");
                    clear_output(out);
                    return;
                }
            }
        }

        let num_out_samples = total_frames * self.out_channels;
        if num_out_samples == 0 {
            return;
        }

        let max_proc_time = Duration::from_secs_f64(
            total_frames as f64 * self.sample_rate_recip * COPY_OUT_TIME_WINDOW,
        );

        while proc_start_time.elapsed() < max_proc_time {
            if let Ok(chunk) = self.from_engine_audio_out_rx.read_chunk(num_out_samples) {
                if cpal_out_channels == self.out_channels {
                    // We can simply just convert the interlaced buffer over.

                    let (slice_1, slice_2) = chunk.as_slices();

                    let out_part = &mut out[0..slice_1.len()];
                    for i in 0..slice_1.len() {
                        out_part[i] = T::from(&slice_1[i]);
                    }

                    let out_part = &mut out[slice_1.len()..slice_1.len() + slice_2.len()];
                    for i in 0..slice_2.len() {
                        out_part[i] = T::from(&slice_2[i]);
                    }
                } else {
                    let (slice_1, slice_2) = chunk.as_slices();

                    for ch_i in 0..cpal_out_channels {
                        if ch_i < self.out_channels {
                            for i in 0..total_frames {
                                let i2 = (i * self.out_channels) + ch_i;

                                let s = if i2 < slice_1.len() {
                                    slice_1[i2]
                                } else {
                                    slice_2[i2 - slice_1.len()]
                                };

                                out[(i * cpal_out_channels) + ch_i] = T::from(&s);
                            }
                        } else {
                            for i in 0..total_frames {
                                out[(i * cpal_out_channels) + ch_i] = T::from(&0.0);
                            }
                        }
                    }
                }

                chunk.commit_all();
                return;
            }
        }

        log::trace!("underrun");

        // The engine took too long to process.
        clear_output(out);
    }
}
