use basedrop::Collector;
use meadowlark_core_types::SampleRate;

pub mod pcm;
pub use pcm::{PcmKey, PcmLoadError, PcmLoader, PcmResource, PcmResourceType};

pub struct ResourceLoader {
    pub pcm_loader: PcmLoader,
    collector: Collector,
}

impl ResourceLoader {
    pub fn new(default_sample_rate: SampleRate) -> Self {
        let collector = Collector::new();

        let pcm_loader = PcmLoader::new(collector.handle(), default_sample_rate);

        Self { pcm_loader, collector }
    }

    pub fn collect(&mut self) {
        self.pcm_loader.collect();
        self.collector.collect();
    }
}
