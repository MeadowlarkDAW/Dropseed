use dropseed::engine::DSEngineMainThread;
use dropseed::plugin_scanner::ScannedPluginInfo;
use egui_glow::egui_winit::egui;

use super::{ActivatedState, DSTestHostGUI};

pub fn scan_external_plugins(
    ds_engine: &mut DSEngineMainThread,
    activated_state: &mut ActivatedState,
) {
    let scanned_plugins_info = ds_engine.scan_external_plugins();

    let scanned_plugin_list: Vec<(ScannedPluginInfo, String)> = scanned_plugins_info
        .scanned_plugins
        .iter()
        .map(|plugin| {
            let dropdown_text = format!("{} ({})", &plugin.description.name, &plugin.format);

            (plugin.clone(), dropdown_text)
        })
        .collect();

    let scanned_failed_list: Vec<(String, String)> = scanned_plugins_info
        .failed_plugins
        .iter()
        .map(|(path, error)| (path.to_string_lossy().to_string(), error.clone()))
        .collect();

    activated_state.scanned_plugin_list = scanned_plugin_list;
    activated_state.scanned_failed_list = scanned_failed_list;
}

pub(crate) fn show(app: &mut DSTestHostGUI, ui: &mut egui::Ui) {
    // TODO: Add/remove plugin paths.

    if app.activated_state.is_none() {
        ui.label("Engine is deactivated");
        return;
    }

    if ui.button("Rescan all plugin directories").clicked() {
        scan_external_plugins(&mut app.ds_engine, &mut app.activated_state.as_mut().unwrap());
    }

    ui.separator();

    let activated_state = app.activated_state.as_ref().unwrap();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Available Plugins");
        egui::ScrollArea::horizontal().id_source("available_plugs_hscroll").show(ui, |ui| {
            egui::Grid::new("available_plugs").num_columns(10).striped(true).show(ui, |ui| {
                ui.label("NAME");
                ui.label("VERSION");
                ui.label("VENDOR");
                ui.label("FORMAT");
                ui.label("FORMAT VERSION");
                ui.label("DESCRIPTION");
                ui.label("RDN");
                ui.label("URL");
                ui.label("MANUAL URL");
                ui.label("SUPPORT URL");
                ui.end_row();

                for plugin in activated_state.scanned_plugin_list.iter() {
                    ui.label(&plugin.0.description.name);
                    ui.label(&plugin.0.description.version);
                    ui.label(&plugin.0.description.vendor);
                    ui.label(format!("{}", plugin.0.format));
                    ui.label(&plugin.0.format_version);
                    ui.label(&plugin.0.description.description);
                    ui.label(&plugin.0.description.id);
                    ui.label(&plugin.0.description.url);
                    ui.label(&plugin.0.description.manual_url);
                    ui.label(&plugin.0.description.support_url);
                    ui.end_row();
                }
            });
        });

        ui.separator();

        ui.heading("Failed Plugin Errors");
        egui::ScrollArea::horizontal().id_source("failed_plugs_hscroll").show(ui, |ui| {
            egui::Grid::new("failed_plugs").num_columns(2).striped(true).show(ui, |ui| {
                ui.label("PATH");
                ui.label("ERROR");
                ui.end_row();

                for (path, error) in activated_state.scanned_failed_list.iter() {
                    ui.label(path);
                    ui.label(error);
                    ui.end_row();
                }
            });
        });
    });
}
