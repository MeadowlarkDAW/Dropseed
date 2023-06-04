use std::error::Error;

use dropseed_core::RtGCHandle;

use super::descriptor::NodeType;
use super::node_host_audio_thread::{NodeHostAudioThr, SharedNodeHostAudioThr};
use super::{NodeMainThr, NodeRequestAudioThr, NodeRequestReceiver};

pub(crate) struct NodeHostMainThr {
    node: Box<dyn NodeMainThr>,

    node_type: NodeType,

    node_request_at: NodeRequestAudioThr,
    node_request_receiver: NodeRequestReceiver,

    activated_processor: Option<SharedNodeHostAudioThr>,
}

impl NodeHostMainThr {
    pub fn new(mut node: Box<dyn NodeMainThr>, node_type: NodeType, gc: RtGCHandle) -> Self {
        #[cfg(feature = "external-plugin-guis")]
        let supports_gui = node._supports_gui();

        let supports_timers = node.supports_timers();

        let (node_request_receiver, node_request_mt, node_request_at) = super::node_request_channel(
            #[cfg(feature = "external-plugin-guis")]
            supports_gui,
            supports_timers,
        );

        node.init(node_request_mt, gc);

        Self { node, node_type, node_request_at, node_request_receiver, activated_processor: None }
    }

    pub fn activate(
        &mut self,
        sample_rate: f64,
        min_frames: u32,
        max_frames: u32,
        gc: &RtGCHandle,
    ) -> Result<(), Box<dyn Error>> {
        if self.activated_processor.is_some() {
            return Err("Node is already active".into());
        }

        let node = self.node.activate(sample_rate, min_frames, max_frames)?;

        let shared_processor =
            SharedNodeHostAudioThr::new(NodeHostAudioThr::new(node, self.node_type), gc);

        self.activated_processor = Some(shared_processor);

        Ok(())
    }
}
