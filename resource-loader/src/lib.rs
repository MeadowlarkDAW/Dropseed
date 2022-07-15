use basedrop::Collector;

pub mod pcm;
pub use pcm::{PcmKey, PcmLoadError, PcmLoader, PcmRAM, PcmRAMType, ResampleQuality};

pub struct ResourceLoader {
    pub pcm_loader: PcmLoader,
    collector: Collector,
}

impl ResourceLoader {
    pub fn new(project_sample_rate: u32) -> Self {
        let collector = Collector::new();

        let pcm_loader = PcmLoader::new(collector.handle(), project_sample_rate);

        Self { pcm_loader, collector }
    }

    pub fn collect(&mut self) {
        self.pcm_loader.collect();
        self.collector.collect();
    }
}
