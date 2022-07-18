use crate::utils::thread_id::SharedThreadIDs;
use basedrop::Shared;
use clack_extensions::audio_ports::HostAudioPorts;
use clack_extensions::gui::PluginGui;
use clack_extensions::log::Log;
use clack_extensions::params::{HostParams, ParamRescanFlags, PluginParams};
use clack_extensions::state::PluginState;
use clack_extensions::thread_check::ThreadCheck;
use clack_host::events::io::{EventBuffer, InputEvents, OutputEvents};
use clack_host::extensions::HostExtensions;
use clack_host::host::{Host, HostAudioProcessor, HostMainThread, HostShared};
use clack_host::plugin::{PluginAudioProcessorHandle, PluginMainThreadHandle, PluginSharedHandle};
use dropseed_core::plugin::host_request_channel::HostRequestChannelSender;
use dropseed_core::plugin::{HostRequestFlags, PluginInstanceID};

pub struct ClapHost;

impl<'a> Host<'a> for ClapHost {
    type AudioProcessor = ClapHostAudioProcessor<'a>;
    type Shared = ClapHostShared<'a>;
    type MainThread = ClapHostMainThread<'a>;

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared) {
        builder
            .register::<Log>()
            .register::<ThreadCheck>()
            .register::<HostAudioPorts>()
            .register::<HostParams>();
    }
}

pub struct ClapHostMainThread<'a> {
    pub shared: &'a ClapHostShared<'a>,
    pub instance: Option<PluginMainThreadHandle<'a>>,
    pub gui_visible: bool,

    rescan_requested: Option<ParamRescanFlags>,
    clear_requested: bool,
    flush_requested: bool,
}

impl<'a> ClapHostMainThread<'a> {
    pub fn new(shared: &'a ClapHostShared<'a>) -> Self {
        Self {
            shared,
            instance: None,
            gui_visible: false,
            rescan_requested: None,
            clear_requested: false,
            flush_requested: false,
        }
    }

    #[allow(unused)]
    fn param_flush(&mut self, in_events: &EventBuffer, out_events: &mut EventBuffer) {
        let params_ext = match self.shared.params_ext {
            None => return,
            Some(p) => p,
        };

        let clap_in_events = InputEvents::from_buffer(in_events);
        let mut clap_out_events = OutputEvents::from_buffer(out_events);

        params_ext.flush(self.instance.as_mut().unwrap(), &clap_in_events, &mut clap_out_events);
    }
}

impl<'a> HostMainThread<'a> for ClapHostMainThread<'a> {
    fn instantiated(&mut self, instance: PluginMainThreadHandle<'a>) {
        self.instance = Some(instance);
    }
}

pub struct ClapHostAudioProcessor<'a> {
    shared: &'a ClapHostShared<'a>,
    plugin: PluginAudioProcessorHandle<'a>,
}

impl<'a> HostAudioProcessor<'a> for ClapHostAudioProcessor<'a> {}

impl<'a> ClapHostAudioProcessor<'a> {
    pub fn new(plugin: PluginAudioProcessorHandle<'a>, shared: &'a ClapHostShared) -> Self {
        Self { shared, plugin }
    }

    pub fn param_flush(&mut self, in_events: &EventBuffer, out_events: &mut EventBuffer) {
        let params_ext = match self.shared.params_ext {
            None => return,
            Some(p) => p,
        };

        let clap_in_events = InputEvents::from_buffer(in_events);
        let mut clap_out_events = OutputEvents::from_buffer(out_events);

        params_ext.flush_active(&mut self.plugin, &clap_in_events, &mut clap_out_events);
    }
}

pub struct ClapHostShared<'a> {
    pub id: Shared<String>,

    pub params_ext: Option<&'a PluginParams>,
    pub state_ext: Option<&'a PluginState>,
    pub gui_ext: Option<&'a PluginGui>,

    host_request: HostRequestChannelSender,
    plugin_log_name: Shared<String>,
    thread_ids: SharedThreadIDs,
}

impl<'a> ClapHostShared<'a> {
    pub(crate) fn new(
        id: Shared<String>,
        host_request: HostRequestChannelSender,
        thread_ids: SharedThreadIDs,
        plugin_id: PluginInstanceID,
        coll_handle: &basedrop::Handle,
    ) -> Self {
        let plugin_log_name = Shared::new(coll_handle, format!("{:?}", &plugin_id));

        Self {
            id,
            host_request,
            params_ext: None,
            state_ext: None,
            gui_ext: None,
            plugin_log_name,
            thread_ids,
        }
    }
}

impl<'a> HostShared<'a> for ClapHostShared<'a> {
    fn instantiated(&mut self, instance: PluginSharedHandle<'a>) {
        self.params_ext = instance.get_extension();
        self.state_ext = instance.get_extension();
        self.gui_ext = instance.get_extension();
    }

    fn request_restart(&self) {
        self.host_request.request(HostRequestFlags::RESTART)
    }

    fn request_process(&self) {
        self.host_request.request(HostRequestFlags::PROCESS)
    }

    fn request_callback(&self) {
        self.host_request.request(HostRequestFlags::CALLBACK)
    }
}

mod extensions;
