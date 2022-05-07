use rusty_daw_core::SampleRate;
use rusty_daw_engine::{HostInfo, RustyDAWEngine};
use std::time::Duration;

#[test]
fn clap_host_test() {
    mowl::init().unwrap();

    let (mut engine, mut engine_rx, internal_scan_res) = RustyDAWEngine::new(
        Duration::from_secs(3),
        HostInfo::new(String::from("RustyDAW integration test"), String::from("0.1.0"), None, None),
        Vec::new(),
    );

    dbg!(internal_scan_res);

    engine.rescan_plugin_directories();

    for msg in engine_rx.try_iter() {
        dbg!(msg);
    }

    let (shared_schedule, graph_in_node_id, graph_out_node_id) =
        engine.activate_engine(SampleRate::default(), 1, 256, 2, 2).unwrap();
    
    for msg in engine_rx.try_iter() {
        dbg!(msg);
    }

    //engine.insert_new_plugin_between_main_ports(save_state, src_plugin_id, dst_plugin_id)
}
