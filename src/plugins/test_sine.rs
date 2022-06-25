use meadowlark_core_types::SampleRate;

use crate::plugin::{
    ext, PluginActivatedInfo, PluginAudioThread, PluginDescriptor, PluginFactory, PluginMainThread,
};
use crate::{EventBuffer, HostRequest, PluginInstanceID, ProcBuffers, ProcInfo, ProcessStatus};

pub static TEST_SINE_STEREO_RDN: &str = "app.meadowlark.test-sine-stereo";

pub struct TestSineStereoFactory;

impl PluginFactory for TestSineStereoFactory {
    fn description(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: TEST_SINE_STEREO_RDN.into(),
            version: "0.1".into(),
            name: "Test Sine Stereo".into(),
            vendor: "Meadowlark".into(),
            description: "A simple plugin used for testing the audio graph. Plays 440Hz in left channel and 880Hz in right channel.".into(),
            url: String::new(),
            manual_url: String::new(),
            support_url: String::new(),
            features: String::new()
        }
    }

    fn new(
        &mut self,
        _host_request: HostRequest,
        _plugin_id: PluginInstanceID,
        _coll_handle: &basedrop::Handle,
    ) -> Result<Box<dyn PluginMainThread>, String> {
        Ok(Box::new(TestSineStereoMainThread {}))
    }
}

pub struct TestSineStereoMainThread {}

impl PluginMainThread for TestSineStereoMainThread {
    fn activate(
        &mut self,
        sample_rate: SampleRate,
        _min_frames: u32,
        _max_frames: u32,
        _coll_handle: &basedrop::Handle,
    ) -> Result<PluginActivatedInfo, String> {
        Ok(PluginActivatedInfo {
            audio_thread: Box::new(TestSineStereoAudioThread::new(sample_rate)),
            internal_handle: None,
        })
    }

    fn audio_ports_ext(&mut self) -> Result<ext::audio_ports::PluginAudioPortsExt, String> {
        Ok(ext::audio_ports::PluginAudioPortsExt::stereo_out())
    }
}

pub struct TestSineStereoAudioThread {
    left_inc: f32,
    right_inc: f32,

    left_phase: f32,
    right_phase: f32,
}

impl TestSineStereoAudioThread {
    fn new(sample_rate: SampleRate) -> Self {
        Self {
            left_inc: 440.0 / sample_rate.as_f32(),
            right_inc: 880.0 / sample_rate.as_f32(),

            left_phase: 0.0,
            right_phase: 0.0,
        }
    }
}

impl PluginAudioThread for TestSineStereoAudioThread {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        buffers: &mut ProcBuffers,
        _in_events: &EventBuffer,
        _out_events: &mut EventBuffer,
    ) -> ProcessStatus {
        let (mut buf_l, mut buf_r) = buffers.audio_out[0].stereo_f32_mut().unwrap();

        let frames = proc_info.frames.min(buf_l.len()).min(buf_r.len());

        for i in 0..frames {
            buf_l[i] = (self.left_phase * std::f32::consts::TAU).sin() * 0.25;
            buf_r[i] = (self.right_phase * std::f32::consts::TAU).sin() * 0.25;

            self.left_phase = (self.left_phase + self.left_inc).fract();
            self.right_phase = (self.right_phase + self.right_inc).fract();
        }

        ProcessStatus::Continue
    }
}
