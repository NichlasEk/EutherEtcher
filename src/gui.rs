use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
};

use eframe::egui::{
    self, pos2, vec2, Align, Color32, FontId, Layout, RichText, Sense, Stroke, StrokeKind, Vec2,
};

use crate::{
    config::default_chunk_size_mib,
    device::{find_device, flatten_visible_devices, list_devices, BlockDevice},
    error::Result,
    image::inspect_image,
    music::AudioEngine,
    safety::run_safety_checks,
    verify, writer,
};

enum FlashEvent {
    Status(String),
    Progress(u64),
    Finished(std::result::Result<(), String>),
}

pub struct EutherGui {
    devices: Vec<BlockDevice>,
    selected_device: Option<String>,
    image_path: String,
    confirm_path: String,
    chunk_size_mib: u64,
    verify_after_write: bool,
    force: bool,
    status: String,
    last_error: Option<String>,
    progress: f32,
    running: bool,
    receiver: Option<Receiver<FlashEvent>>,
    wave_phase: f32,
    show_internal_drives: bool,
    music_enabled: bool,
    music: Option<AudioEngine>,
    music_error: Option<String>,
}

impl Default for EutherGui {
    fn default() -> Self {
        let mut app = Self {
            devices: Vec::new(),
            selected_device: None,
            image_path: String::new(),
            confirm_path: String::new(),
            chunk_size_mib: default_chunk_size_mib(),
            verify_after_write: true,
            force: false,
            status: "Ready".to_string(),
            last_error: None,
            progress: 0.0,
            running: false,
            receiver: None,
            wave_phase: 0.0,
            show_internal_drives: false,
            music_enabled: true,
            music: None,
            music_error: None,
        };
        app.refresh_devices();
        app.start_music();
        app
    }
}

pub fn run_gui() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 760.0]),
        ..Default::default()
    };

    eframe::run_native(
        "EutherEtcher",
        options,
        Box::new(|_cc| Ok(Box::<EutherGui>::default())),
    )
    .map_err(|err| std::io::Error::other(err.to_string()))?;

    Ok(())
}

impl eframe::App for EutherGui {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_flash_events();
        self.wave_phase = (self.wave_phase + 0.018) % std::f32::consts::TAU;
        ctx.request_repaint();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Frame::default()
            .fill(Color32::from_rgb(12, 13, 18))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.add_space(14.0);
                    self.header(ui);
                    ui.add_space(16.0);

                    ui.columns(2, |columns| {
                        columns[0].set_width(390.0);
                        self.device_panel(&mut columns[0]);
                        self.flash_panel(&mut columns[1]);
                    });
                });
            });
    }
}

impl EutherGui {
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("EutherEtcher")
                        .font(FontId::proportional(36.0))
                        .strong()
                        .color(Color32::from_rgb(236, 241, 235)),
                );
                ui.label(
                    RichText::new("Linux image writer")
                        .font(FontId::proportional(15.0))
                        .color(Color32::from_rgb(142, 155, 152)),
                );
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!self.running, egui::Button::new("Refresh devices"))
                    .clicked()
                {
                    self.refresh_devices();
                }

                if ui.button("Next loop").clicked() {
                    self.next_music_track();
                }

                let music_label = if self.music_enabled {
                    "Music on"
                } else {
                    "Music off"
                };
                if ui.button(music_label).clicked() {
                    self.toggle_music();
                }
            });
        });
    }

    fn device_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style())
            .fill(Color32::from_rgb(24, 28, 30))
            .stroke(Stroke::new(1.0, Color32::from_rgb(59, 68, 70)))
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.heading("2. Target");
                ui.checkbox(&mut self.show_internal_drives, "Show internal drives");
                if self.show_internal_drives {
                    ui.label(
                        RichText::new("Internal SATA/NVMe drives are marked DANGER.")
                            .color(Color32::from_rgb(245, 119, 98)),
                    );
                }
                ui.add_space(8.0);

                let flat = {
                    let mut refs = Vec::new();
                    flatten_visible_devices(
                        &self.devices,
                        &mut refs,
                        self.show_internal_drives,
                        false,
                    );
                    refs.into_iter().cloned().collect::<Vec<_>>()
                };

                for device in &flat {
                    self.device_row(ui, device);
                    ui.add_space(6.0);
                }
            });
    }

    fn device_row(&mut self, ui: &mut egui::Ui, device: &BlockDevice) {
        let selected = self.selected_device.as_deref() == Some(device.path.as_str());
        let color = if device.is_dangerous_internal() {
            Color32::from_rgb(245, 83, 70)
        } else if device.is_removable_target() {
            Color32::from_rgb(53, 180, 156)
        } else {
            Color32::from_rgb(128, 133, 134)
        };

        let response = egui::Frame::default()
            .fill(if selected {
                if device.is_dangerous_internal() {
                    Color32::from_rgb(76, 35, 35)
                } else {
                    Color32::from_rgb(38, 64, 59)
                }
            } else {
                Color32::from_rgb(29, 33, 35)
            })
            .stroke(Stroke::new(
                1.0,
                if selected {
                    if device.is_dangerous_internal() {
                        Color32::from_rgb(245, 83, 70)
                    } else {
                        Color32::from_rgb(75, 220, 190)
                    }
                } else {
                    Color32::from_rgb(48, 55, 57)
                },
            ))
            .corner_radius(6.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(color, "■");
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&device.path)
                                .font(FontId::monospace(15.0))
                                .color(Color32::from_rgb(235, 239, 236)),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{}  {}  {}  {}",
                                device.risk_label(),
                                format_size(device.size_bytes),
                                device.transport.as_deref().unwrap_or("unknown"),
                                device.model.as_deref().unwrap_or("unknown")
                            ))
                            .font(FontId::proportional(13.0))
                            .color(Color32::from_rgb(148, 156, 154)),
                        );
                    });
                });
            })
            .response;

        if response.interact(Sense::click()).clicked() {
            self.selected_device = Some(device.path.clone());
            self.confirm_path.clear();
        }
    }

    fn flash_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style())
            .fill(Color32::from_rgb(22, 24, 27))
            .stroke(Stroke::new(1.0, Color32::from_rgb(59, 62, 68)))
            .inner_margin(18.0)
            .show(ui, |ui| {
                self.signal_canvas(ui);
                ui.add_space(16.0);
                self.step_strip(ui);
                ui.add_space(12.0);

                ui.label(RichText::new("1. Image").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.image_path)
                        .hint_text("./archlinux.iso")
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.verify_after_write, "Verify after write");
                    ui.checkbox(&mut self.force, "Force");
                    ui.add(
                        egui::DragValue::new(&mut self.chunk_size_mib)
                            .range(1..=64)
                            .suffix(" MiB"),
                    );
                });

                ui.add_space(10.0);
                ui.label(RichText::new("3. Arm").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.confirm_path)
                        .hint_text(self.selected_device.as_deref().unwrap_or("/dev/sdX"))
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.running, egui::Button::new("Dry run"))
                        .clicked()
                    {
                        self.dry_run();
                    }

                    let flash_enabled = !self.running
                        && self.selected_device.is_some()
                        && !self.image_path.trim().is_empty();
                    if ui
                        .add_enabled(flash_enabled, egui::Button::new("Flash image"))
                        .clicked()
                    {
                        self.start_flash();
                    }
                });

                ui.add_space(14.0);
                ui.add(egui::ProgressBar::new(self.progress).desired_width(f32::INFINITY));
                ui.label(RichText::new(&self.status).color(Color32::from_rgb(213, 219, 215)));

                if let Some(music) = &self.music {
                    ui.label(
                        RichText::new(format!("Loop: {}", music.track_name()))
                            .color(Color32::from_rgb(245, 177, 66)),
                    );
                } else if let Some(error) = &self.music_error {
                    ui.label(
                        RichText::new(format!("Music unavailable: {error}"))
                            .color(Color32::from_rgb(148, 156, 154)),
                    );
                }

                if let Some(error) = &self.last_error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(error).color(Color32::from_rgb(245, 119, 98)));
                }
            });
    }

    fn signal_canvas(&mut self, ui: &mut egui::Ui) {
        let desired = Vec2::new(ui.available_width(), 220.0);
        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 8.0, Color32::from_rgb(13, 17, 19));
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(1.0, Color32::from_rgb(52, 62, 63)),
            StrokeKind::Outside,
        );

        let center = rect.center();
        let radius = rect.height().min(rect.width()) * 0.28;
        for ring in 0..4 {
            let t = self.wave_phase + ring as f32 * 0.65;
            let pulse = radius + t.sin().abs() * 18.0 + ring as f32 * 16.0;
            let alpha = (95.0 - ring as f32 * 16.0).max(24.0) as u8;
            painter.circle_stroke(
                center,
                pulse,
                Stroke::new(2.0, Color32::from_rgba_premultiplied(53, 180, 156, alpha)),
            );
        }

        for index in 0..18 {
            let x = rect.left() + 24.0 + index as f32 * (rect.width() - 48.0) / 17.0;
            let y = center.y + ((index as f32 * 0.7 + self.wave_phase * 3.0).sin() * 36.0);
            let height = 22.0 + ((index as f32 + self.wave_phase * 4.0).cos().abs() * 54.0);
            let color = if index % 3 == 0 {
                Color32::from_rgb(245, 177, 66)
            } else {
                Color32::from_rgb(53, 180, 156)
            };
            painter.line_segment(
                [pos2(x, y - height / 2.0), pos2(x, y + height / 2.0)],
                Stroke::new(3.0, color),
            );
        }

        painter.text(
            center + vec2(0.0, -8.0),
            egui::Align2::CENTER_CENTER,
            "NEON IMAGE STREAM",
            FontId::monospace(18.0),
            Color32::from_rgb(232, 238, 233),
        );
        painter.text(
            center + vec2(0.0, 20.0),
            egui::Align2::CENTER_CENTER,
            self.selected_device
                .as_deref()
                .unwrap_or("NO TARGET SELECTED"),
            FontId::monospace(14.0),
            Color32::from_rgb(146, 158, 155),
        );
    }

    fn step_strip(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.step_badge(ui, "1", "Image", !self.image_path.trim().is_empty());
            self.step_badge(ui, "2", "Target", self.selected_device.is_some());
            let armed = self
                .selected_device
                .as_deref()
                .is_some_and(|path| self.confirm_path.trim() == path);
            self.step_badge(ui, "3", "Flash", armed);
        });
    }

    fn step_badge(&self, ui: &mut egui::Ui, number: &str, label: &str, active: bool) {
        let fill = if active {
            Color32::from_rgb(25, 77, 67)
        } else {
            Color32::from_rgb(31, 34, 39)
        };
        let stroke = if active {
            Color32::from_rgb(53, 224, 182)
        } else {
            Color32::from_rgb(72, 75, 82)
        };

        egui::Frame::default()
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(number)
                            .font(FontId::monospace(15.0))
                            .color(Color32::from_rgb(245, 177, 66)),
                    );
                    ui.label(
                        RichText::new(label)
                            .font(FontId::proportional(15.0))
                            .color(Color32::from_rgb(232, 238, 233)),
                    );
                });
            });
    }

    fn refresh_devices(&mut self) {
        match list_devices() {
            Ok(devices) => {
                self.devices = devices;
                self.status = "Device list refreshed".to_string();
                self.last_error = None;
            }
            Err(err) => {
                self.last_error = Some(err.to_string());
                self.status = "Could not refresh devices".to_string();
            }
        }
    }

    fn start_music(&mut self) {
        match AudioEngine::start_random() {
            Ok(engine) => {
                self.music = Some(engine);
                self.music_enabled = true;
                self.music_error = None;
            }
            Err(err) => {
                self.music = None;
                self.music_enabled = false;
                self.music_error = Some(err.to_string());
            }
        }
    }

    fn toggle_music(&mut self) {
        if self.music_enabled {
            self.music = None;
            self.music_enabled = false;
        } else {
            self.music_enabled = true;
            self.start_music();
        }
    }

    fn next_music_track(&mut self) {
        if let Some(music) = &mut self.music {
            music.next_track();
        } else if self.music_enabled {
            self.start_music();
        }
    }

    fn selected_block_device(&self) -> std::result::Result<BlockDevice, String> {
        let path = self
            .selected_device
            .as_deref()
            .ok_or_else(|| "No device selected".to_string())?;
        find_device(&self.devices, path)
            .cloned()
            .ok_or_else(|| format!("Device no longer present: {path}"))
    }

    fn dry_run(&mut self) {
        self.progress = 0.0;
        self.last_error = None;

        let result = self.validate_selection();
        match result {
            Ok((image_size, device_path)) => {
                self.status =
                    format!("Dry-run passed: {image_size} bytes can be written to {device_path}");
            }
            Err(err) => {
                self.status = "Dry-run failed".to_string();
                self.last_error = Some(err);
            }
        }
    }

    fn start_flash(&mut self) {
        self.last_error = None;

        let device = match self.selected_block_device() {
            Ok(device) => device,
            Err(err) => {
                self.last_error = Some(err);
                return;
            }
        };

        if self.confirm_path.trim() != device.path {
            self.last_error = Some(format!("Type {} before flashing", device.path));
            return;
        }

        let image = match inspect_image(PathBuf::from(self.image_path.trim()).as_path()) {
            Ok(image) => image,
            Err(err) => {
                self.last_error = Some(err.to_string());
                return;
            }
        };

        if let Err(err) = run_safety_checks(&device, &image, &Default::default(), self.force) {
            self.last_error = Some(err.to_string());
            return;
        }

        let device_path = PathBuf::from(device.path);
        let image_path = image.path;
        let image_size = image.size_bytes;
        let chunk_size_mib = self.chunk_size_mib;
        let verify_after_write = self.verify_after_write;
        let (sender, receiver) = mpsc::channel();

        self.receiver = Some(receiver);
        self.running = true;
        self.progress = 0.0;
        self.status = "Writing image".to_string();

        thread::spawn(move || {
            let write_result = writer::write_image_with_progress(
                &image_path,
                &device_path,
                chunk_size_mib,
                |written| {
                    let _ = sender.send(FlashEvent::Progress(written));
                },
            );

            if let Err(err) = write_result {
                let _ = sender.send(FlashEvent::Finished(Err(err.to_string())));
                return;
            }

            if verify_after_write {
                let _ = sender.send(FlashEvent::Status("Verifying image".to_string()));
                if let Err(err) = verify::verify_image(
                    &image_path,
                    &device_path,
                    image_size,
                    chunk_size_mib,
                    false,
                ) {
                    let _ = sender.send(FlashEvent::Finished(Err(err.to_string())));
                    return;
                }
            }

            let _ = sender.send(FlashEvent::Finished(Ok(())));
        });
    }

    fn validate_selection(&self) -> std::result::Result<(u64, String), String> {
        let device = self.selected_block_device()?;
        let image = inspect_image(PathBuf::from(self.image_path.trim()).as_path())
            .map_err(|err| err.to_string())?;
        run_safety_checks(&device, &image, &Default::default(), self.force)
            .map_err(|err| err.to_string())?;

        Ok((image.size_bytes, device.path))
    }

    fn poll_flash_events(&mut self) {
        let Some(receiver) = self.receiver.take() else {
            return;
        };

        let mut keep_receiver = true;
        while let Ok(event) = receiver.try_recv() {
            match event {
                FlashEvent::Status(status) => {
                    self.status = status;
                }
                FlashEvent::Progress(written) => {
                    if let Ok((image_size, _)) = self.validate_selection() {
                        self.progress = (written as f32 / image_size as f32).clamp(0.0, 1.0);
                    }
                }
                FlashEvent::Finished(result) => {
                    self.running = false;
                    self.progress = if result.is_ok() { 1.0 } else { self.progress };
                    match result {
                        Ok(()) => {
                            self.status = "Flash complete".to_string();
                            self.last_error = None;
                        }
                        Err(err) => {
                            self.status = "Flash failed".to_string();
                            self.last_error = Some(err);
                        }
                    }
                    keep_receiver = false;
                }
            }
        }

        if keep_receiver {
            self.receiver = Some(receiver);
        }
    }
}

fn format_size(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return "unknown".to_string();
    };

    let gib = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    if gib >= 1.0 {
        format!("{gib:.1} GiB")
    } else {
        let mib = bytes as f64 / 1024.0 / 1024.0;
        format!("{mib:.1} MiB")
    }
}
