use eframe::egui;

use super::BasicDawExampleGUI;

pub(crate) fn show(app: &mut BasicDawExampleGUI, ui: &mut egui::Ui) {
    // TODO: Add/remove plugin paths.

    if ui.button("Rescan all plugin directories").clicked() {
        app.engine.rescan_plugin_directories();
    }

    ui.separator();

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

                for plugin in app.plugin_list.iter() {
                    ui.label(
                        plugin.description.name.as_ref().map(|v| v.as_str()).unwrap_or("(none)"),
                    );
                    ui.label(
                        plugin.description.version.as_ref().map(|v| v.as_str()).unwrap_or("(none)"),
                    );
                    ui.label(
                        plugin
                            .description
                            .vendor
                            .as_ref()
                            .map(|v| v.as_str())
                            .unwrap_or("(unkown)"),
                    );
                    ui.label(format!("{}", plugin.format));
                    ui.label(
                        plugin.format_version.as_ref().map(|v| v.as_str()).unwrap_or("(unkown)"),
                    );
                    ui.label(
                        plugin
                            .description
                            .description
                            .as_ref()
                            .map(|v| v.as_str())
                            .unwrap_or("(none)"),
                    );
                    ui.label(&plugin.description.id);
                    ui.label(
                        plugin.description.url.as_ref().map(|v| v.as_str()).unwrap_or("(none)"),
                    );
                    ui.label(
                        plugin
                            .description
                            .manual_url
                            .as_ref()
                            .map(|v| v.as_str())
                            .unwrap_or("(none)"),
                    );
                    ui.label(
                        plugin
                            .description
                            .support_url
                            .as_ref()
                            .map(|v| v.as_str())
                            .unwrap_or("(none)"),
                    );
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

                for (path, error) in app.failed_plugins_text.iter() {
                    ui.label(path);
                    ui.label(error);
                    ui.end_row();
                }
            });
        });
    });
}
