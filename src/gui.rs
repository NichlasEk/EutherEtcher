use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread,
    time::Instant,
};

use eframe::egui::{
    self, pos2, vec2, Align, Color32, FontId, Layout, RichText, Sense, Stroke, StrokeKind, Vec2,
};

use crate::{
    cancel::CancelFlag,
    config::default_chunk_size_mib,
    device::{find_device, flatten_visible_devices, list_devices, BlockDevice},
    error::Result,
    image::{
        checksum_sidecar_status, inspect_image, read_sha256_sidecar, sha256_file_with_progress,
        ChecksumStatus,
    },
    music::AudioEngine,
    safety::run_safety_checks,
    verify, writer,
};

enum FlashEvent {
    Status(String),
    Phase { name: String, total_bytes: u64 },
    Progress { done_bytes: u64, total_bytes: u64 },
    Finished(std::result::Result<(), String>),
}

enum ChecksumEvent {
    Progress {
        done_bytes: u64,
        total_bytes: u64,
    },
    Finished {
        path: String,
        result: std::result::Result<(Option<String>, ChecksumStatus), String>,
    },
}

pub struct EutherGui {
    devices: Vec<BlockDevice>,
    selected_device: Option<String>,
    selected_device_identity: Option<String>,
    image_path: String,
    chunk_size_mib: u64,
    verify_after_write: bool,
    verify_source_checksum: bool,
    force: bool,
    status: String,
    last_error: Option<String>,
    progress: f32,
    running: bool,
    receiver: Option<Receiver<FlashEvent>>,
    cancel_flag: Option<CancelFlag>,
    helper_child: Arc<Mutex<Option<Child>>>,
    wave_phase: f32,
    phase_name: String,
    phase_done_bytes: u64,
    phase_total_bytes: u64,
    phase_started_at: Option<Instant>,
    image_sha256: Option<String>,
    image_sha256_path: Option<String>,
    checksum_status: Option<ChecksumStatus>,
    checksum_receiver: Option<Receiver<ChecksumEvent>>,
    checksum_running: bool,
    checksum_done_bytes: u64,
    checksum_total_bytes: u64,
    checksum_started_at: Option<Instant>,
    show_internal_drives: bool,
    music_enabled: bool,
    music: Option<AudioEngine>,
    music_error: Option<String>,
    show_preflight: bool,
}

impl Default for EutherGui {
    fn default() -> Self {
        let mut app = Self {
            devices: Vec::new(),
            selected_device: None,
            selected_device_identity: None,
            image_path: String::new(),
            chunk_size_mib: default_chunk_size_mib(),
            verify_after_write: true,
            verify_source_checksum: false,
            force: false,
            status: "Ready".to_string(),
            last_error: None,
            progress: 0.0,
            running: false,
            receiver: None,
            cancel_flag: None,
            helper_child: Arc::new(Mutex::new(None)),
            wave_phase: 0.0,
            phase_name: "Idle".to_string(),
            phase_done_bytes: 0,
            phase_total_bytes: 0,
            phase_started_at: None,
            image_sha256: None,
            image_sha256_path: None,
            checksum_status: None,
            checksum_receiver: None,
            checksum_running: false,
            checksum_done_bytes: 0,
            checksum_total_bytes: 0,
            checksum_started_at: None,
            show_internal_drives: false,
            music_enabled: true,
            music: None,
            music_error: None,
            show_preflight: false,
        };
        app.refresh_devices();
        app.start_music();
        app
    }
}

pub fn run_gui() -> Result<()> {
    install_gui_panic_hook();

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

fn install_gui_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/eutheretcher-panic.log")
        {
            let _ = writeln!(
                file,
                "\nprocess={} thread={:?}\n{info}",
                std::process::id(),
                thread::current().id()
            );
        }
        previous(info);
    }));
}

fn log_gui_event(message: impl AsRef<str>) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/eutheretcher-gui.log")
    {
        let _ = writeln!(file, "{}", message.as_ref());
    }
}

impl eframe::App for EutherGui {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_flash_events();
        self.poll_checksum_events();
        self.handle_dropped_files(ctx);
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

                    if ui.available_width() < 900.0 {
                        self.device_panel(ui);
                        ui.add_space(12.0);
                        self.flash_panel(ui);
                    } else {
                        ui.columns(2, |columns| {
                            columns[0].set_width(390.0);
                            self.device_panel(&mut columns[0]);
                            self.flash_panel(&mut columns[1]);
                        });
                    }

                    if self.show_preflight {
                        self.preflight_window(ui.ctx());
                    }
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
                let targets = flat
                    .into_iter()
                    .filter(|device| device.kind == "disk")
                    .collect::<Vec<_>>();

                if targets.is_empty() {
                    ui.label(
                        RichText::new("No removable USB/SD targets detected.")
                            .color(Color32::from_rgb(148, 156, 154)),
                    );
                }

                for device in &targets {
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

                        if !device.children.is_empty() {
                            ui.add_space(4.0);
                            for child in &device.children {
                                ui.label(
                                    RichText::new(format!(
                                        "{}  {}  {}",
                                        child.path,
                                        format_size(child.size_bytes),
                                        format_mountpoints(&child.mountpoints)
                                    ))
                                    .font(FontId::monospace(12.0))
                                    .color(Color32::from_rgb(118, 128, 127)),
                                );
                            }
                        }
                    });
                });
            })
            .response;

        if response.interact(Sense::click()).clicked() {
            self.selected_device = Some(device.path.clone());
            self.selected_device_identity = Some(device.identity_fingerprint());
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
                ui.horizontal(|ui| {
                    if ui
                        .add_sized([118.0, 24.0], egui::Button::new("Select image"))
                        .clicked()
                    {
                        self.pick_image();
                    }

                    ui.add(
                        egui::TextEdit::singleline(&mut self.image_path)
                            .hint_text("./archlinux.iso")
                            .desired_width(finite_width(ui, 220.0) - 8.0),
                    );
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.verify_after_write, "Verify after write");
                    if ui
                        .checkbox(&mut self.verify_source_checksum, "Check SHA256 sidecar")
                        .changed()
                    {
                        self.reset_checksum_state();
                    }
                    ui.checkbox(&mut self.force, "Force");
                    ui.add(
                        egui::DragValue::new(&mut self.chunk_size_mib)
                            .range(1..=64)
                            .suffix(" MiB"),
                    );
                });

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
                        self.open_preflight();
                    }
                });

                ui.add_space(14.0);
                ui.add(
                    egui::ProgressBar::new(self.progress).desired_width(finite_width(ui, 220.0)),
                );
                ui.label(RichText::new(&self.status).color(Color32::from_rgb(213, 219, 215)));
                if self.running && ui.button("Cancel").clicked() {
                    self.cancel_flash();
                }
                if let Ok(device) = self.selected_block_device() {
                    if !self.running
                        && device.has_mountpoints_recursive()
                        && ui.button("Unmount target").clicked()
                    {
                        self.unmount_selected_target(device.path);
                    }
                }
                if self.phase_total_bytes > 0 {
                    ui.label(
                        RichText::new(self.phase_status_line())
                            .font(FontId::monospace(12.0))
                            .color(Color32::from_rgb(148, 156, 154)),
                    );
                }

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
        let desired = Vec2::new(finite_width(ui, 320.0), 220.0);
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
            self.step_badge(
                ui,
                "3",
                "Flash",
                self.selected_device.is_some() && !self.image_path.trim().is_empty(),
            );
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

    fn pick_image(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Disk images", &["iso", "img"])
            .pick_file()
        {
            self.image_path = path.display().to_string();
            self.reset_checksum_state();
            self.last_error = None;
            self.status = "Image selected".to_string();
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped_files = ctx.input(|input| input.raw.dropped_files.clone());
        for file in dropped_files {
            let Some(path) = file.path else {
                continue;
            };
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase);
            if matches!(extension.as_deref(), Some("iso" | "img")) {
                self.image_path = path.display().to_string();
                self.reset_checksum_state();
                self.last_error = None;
                self.status = "Image dropped".to_string();
                break;
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

    fn reset_checksum_state(&mut self) {
        self.image_sha256 = None;
        self.image_sha256_path = None;
        self.checksum_status = None;
        self.checksum_receiver = None;
        self.checksum_running = false;
        self.checksum_done_bytes = 0;
        self.checksum_total_bytes = 0;
        self.checksum_started_at = None;
    }

    fn verify_selected_device_identity(
        &self,
        selected: &BlockDevice,
    ) -> std::result::Result<(), String> {
        let expected = self
            .selected_device_identity
            .as_deref()
            .ok_or_else(|| "Selected device identity is missing".to_string())?;
        let devices = list_devices().map_err(|err| err.to_string())?;
        let current = find_device(&devices, &selected.path)
            .ok_or_else(|| format!("Device disappeared before flashing: {}", selected.path))?;

        if current.identity_fingerprint() == expected {
            Ok(())
        } else {
            Err(format!(
                "{} changed since it was selected; refresh devices and select it again",
                selected.path
            ))
        }
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

    fn open_preflight(&mut self) {
        self.last_error = None;

        match self.validate_selection() {
            Ok((image_size, _device_path)) => {
                self.show_preflight = true;
                self.start_checksum_if_needed(image_size);
            }
            Err(err) => {
                self.status = "Pre-flight failed".to_string();
                self.last_error = Some(err);
            }
        }
    }

    fn start_checksum_if_needed(&mut self, total_bytes: u64) {
        let path = self.image_path.trim().to_string();
        if !self.verify_source_checksum {
            self.image_sha256 = None;
            self.image_sha256_path = Some(path);
            self.checksum_status = Some(ChecksumStatus::Missing);
            self.checksum_receiver = None;
            self.checksum_running = false;
            self.checksum_done_bytes = 0;
            self.checksum_total_bytes = 0;
            self.checksum_started_at = None;
            self.progress = 0.0;
            self.status = "SHA256 sidecar check skipped".to_string();
            return;
        }

        if self.image_sha256_path.as_deref() == Some(path.as_str()) && self.image_sha256.is_some() {
            self.status = "Review pre-flight confirmation".to_string();
            return;
        }

        match read_sha256_sidecar(PathBuf::from(&path).as_path()) {
            Ok(None) => {
                self.image_sha256 = None;
                self.image_sha256_path = Some(path);
                self.checksum_status = Some(ChecksumStatus::Missing);
                self.checksum_receiver = None;
                self.checksum_running = false;
                self.checksum_done_bytes = 0;
                self.checksum_total_bytes = 0;
                self.checksum_started_at = None;
                self.progress = 0.0;
                self.status = "No SHA256 sidecar found; pre-flight checksum skipped".to_string();
                return;
            }
            Ok(Some(_expected)) => {}
            Err(err) => {
                self.image_sha256 = None;
                self.image_sha256_path = None;
                self.checksum_status = None;
                self.checksum_receiver = None;
                self.checksum_running = false;
                self.status = "Pre-flight failed".to_string();
                self.last_error = Some(err.to_string());
                return;
            }
        }

        self.image_sha256 = None;
        self.image_sha256_path = None;
        self.checksum_status = None;
        self.checksum_running = true;
        self.checksum_done_bytes = 0;
        self.checksum_total_bytes = total_bytes;
        self.checksum_started_at = Some(Instant::now());
        self.progress = 0.0;
        self.status = "Calculating image SHA256 for pre-flight".to_string();

        let (sender, receiver) = mpsc::channel();
        self.checksum_receiver = Some(receiver);

        thread::spawn(move || {
            let result = sha256_file_with_progress(PathBuf::from(&path).as_path(), |done| {
                let _ = sender.send(ChecksumEvent::Progress {
                    done_bytes: done,
                    total_bytes,
                });
            })
            .map_err(|err| err.to_string())
            .and_then(|hash| {
                checksum_sidecar_status(PathBuf::from(&path).as_path(), &hash)
                    .map(|status| (Some(hash), status))
                    .map_err(|err| err.to_string())
            });
            let _ = sender.send(ChecksumEvent::Finished { path, result });
        });
    }

    fn phase_status_line(&self) -> String {
        let elapsed = self
            .phase_started_at
            .map(|started| started.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        let speed = if elapsed > 0.0 {
            self.phase_done_bytes as f64 / elapsed
        } else {
            0.0
        };
        let remaining = self.phase_total_bytes.saturating_sub(self.phase_done_bytes);
        let eta = if speed > 0.0 {
            format_duration((remaining as f64 / speed).round() as u64)
        } else {
            "--:--".to_string()
        };

        format!(
            "{}  {} / {}  {}/s  ETA {}",
            self.phase_name,
            format_bytes(self.phase_done_bytes),
            format_bytes(self.phase_total_bytes),
            format_bytes(speed as u64),
            eta
        )
    }

    fn checksum_status_line(&self) -> String {
        let elapsed = self
            .checksum_started_at
            .map(|started| started.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        let speed = if elapsed > 0.0 {
            self.checksum_done_bytes as f64 / elapsed
        } else {
            0.0
        };
        let remaining = self
            .checksum_total_bytes
            .saturating_sub(self.checksum_done_bytes);
        let eta = if speed > 0.0 {
            format_duration((remaining as f64 / speed).round() as u64)
        } else {
            "--:--".to_string()
        };

        format!(
            "Checksum  {} / {}  {}/s  ETA {}",
            format_bytes(self.checksum_done_bytes),
            format_bytes(self.checksum_total_bytes),
            format_bytes(speed as u64),
            eta
        )
    }

    fn start_flash(&mut self) {
        log_gui_event("start_flash: entered");
        self.last_error = None;
        self.show_preflight = false;

        let device = match self.selected_block_device() {
            Ok(device) => device,
            Err(err) => {
                self.last_error = Some(err);
                return;
            }
        };

        if let Err(err) = self.verify_selected_device_identity(&device) {
            self.last_error = Some(err);
            return;
        }

        if self.checksum_running
            || self.image_sha256_path.as_deref() != Some(self.image_path.trim())
        {
            self.show_preflight = true;
            self.last_error = Some("Pre-flight is still running".to_string());
            return;
        }

        if matches!(self.checksum_status, Some(ChecksumStatus::Mismatch { .. })) {
            self.show_preflight = true;
            self.last_error = Some("SHA256 sidecar mismatch blocks flashing".to_string());
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
        let force = self.force;
        let (sender, receiver) = mpsc::channel();
        let cancel = CancelFlag::default();
        let thread_cancel = cancel.clone();
        let helper_child = Arc::clone(&self.helper_child);

        self.receiver = Some(receiver);
        self.cancel_flag = Some(cancel);
        self.running = true;
        self.progress = 0.0;
        self.status = "Writing image".to_string();
        self.phase_name = "Writing".to_string();
        self.phase_done_bytes = 0;
        self.phase_total_bytes = image_size;
        self.phase_started_at = Some(Instant::now());
        log_gui_event(format!(
            "start_flash: spawning worker image={} device={} size={}",
            image_path.display(),
            device_path.display(),
            image_size
        ));

        thread::spawn(move || {
            if !is_root() {
                log_gui_event("flash_worker: non-root, starting pkexec helper");
                let _ = sender.send(FlashEvent::Status(
                    "Waiting for administrator authorization. If no prompt appears, check your polkit agent.".to_string(),
                ));
                if let Err(err) = run_helper_with_pkexec(
                    &sender,
                    &image_path,
                    &device_path,
                    chunk_size_mib,
                    verify_after_write,
                    force,
                    &helper_child,
                ) {
                    log_gui_event(format!("flash_worker: helper failed: {err}"));
                    let _ = sender.send(FlashEvent::Finished(Err(err)));
                }
                return;
            }

            log_gui_event("flash_worker: root write path");
            let _ = sender.send(FlashEvent::Phase {
                name: "Writing".to_string(),
                total_bytes: image_size,
            });
            let write_result = writer::write_image_with_progress(
                &image_path,
                &device_path,
                chunk_size_mib,
                |written| {
                    let _ = sender.send(FlashEvent::Progress {
                        done_bytes: written,
                        total_bytes: image_size,
                    });
                    thread_cancel.check()
                },
                &thread_cancel,
            );

            if let Err(err) = write_result {
                let _ = sender.send(FlashEvent::Finished(Err(err.to_string())));
                return;
            }

            if verify_after_write {
                let _ = sender.send(FlashEvent::Phase {
                    name: "Verifying".to_string(),
                    total_bytes: image_size,
                });
                let verify_result = verify::verify_image_with_progress(
                    &image_path,
                    &device_path,
                    chunk_size_mib,
                    |verified| {
                        let _ = sender.send(FlashEvent::Progress {
                            done_bytes: verified,
                            total_bytes: image_size,
                        });
                        thread_cancel.check()
                    },
                    &thread_cancel,
                );
                if let Err(err) = verify_result {
                    let _ = sender.send(FlashEvent::Finished(Err(err.to_string())));
                    return;
                }
            }

            let _ = sender.send(FlashEvent::Finished(Ok(())));
        });
    }

    fn cancel_flash(&mut self) {
        if let Some(cancel) = &self.cancel_flag {
            cancel.cancel();
        }
        if let Ok(mut child) = self.helper_child.lock() {
            if let Some(child) = child.as_mut() {
                let _ = child.kill();
            }
        }
        self.status = "Cancelling".to_string();
    }

    fn unmount_selected_target(&mut self, device_path: String) {
        match std::env::current_exe()
            .map_err(|err| err.to_string())
            .and_then(|exe| {
                let mut command = pkexec_command()?;
                command
                    .arg(exe)
                    .arg("unmount-helper")
                    .arg("--device")
                    .arg(&device_path);
                command.output().map_err(|err| err.to_string())
            }) {
            Ok(output) if output.status.success() => {
                self.status = "Target unmounted".to_string();
                self.last_error = None;
                self.refresh_devices();
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                self.last_error =
                    Some(privilege_error_message("failed to unmount target", &stderr));
            }
            Err(err) => {
                self.last_error = Some(err);
            }
        }
    }

    fn preflight_window(&mut self, ctx: &egui::Context) {
        let device = self.selected_block_device().ok();
        let image = inspect_image(PathBuf::from(self.image_path.trim()).as_path()).ok();
        let checksum_ready = self.image_sha256_path.as_deref() == Some(self.image_path.trim())
            && self.checksum_status.is_some()
            && !self.checksum_running;
        let checksum_ok = checksum_ready
            && !matches!(self.checksum_status, Some(ChecksumStatus::Mismatch { .. }));
        let can_flash = !self.running && device.is_some() && image.is_some() && checksum_ok;

        egui::Window::new("Pre-flight")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(520.0);
                ui.label(
                    RichText::new("Final check before writing")
                        .font(FontId::proportional(24.0))
                        .strong()
                        .color(Color32::from_rgb(236, 241, 235)),
                );
                ui.add_space(10.0);

                if let Some(image) = &image {
                    detail_row(ui, "Image", &image.path.display().to_string());
                    detail_row(ui, "Image size", &format!("{} bytes", image.size_bytes));
                    detail_row(
                        ui,
                        "SHA256",
                        if self.checksum_running {
                            "calculating..."
                        } else if !self.verify_source_checksum {
                            "skipped"
                        } else if matches!(self.checksum_status, Some(ChecksumStatus::Missing)) {
                            "not provided"
                        } else {
                            self.image_sha256.as_deref().unwrap_or("not calculated")
                        },
                    );
                    checksum_row(ui, self.checksum_status.as_ref());
                    if !self.verify_source_checksum {
                        ui.label(
                            RichText::new(
                                "Source checksum is opt-in. Write verification can still run after flashing.",
                            )
                            .color(Color32::from_rgb(148, 156, 154)),
                        );
                    }
                    if self.checksum_running {
                        ui.add(
                            egui::ProgressBar::new(if self.checksum_total_bytes == 0 {
                                0.0
                            } else {
                                self.checksum_done_bytes as f32 / self.checksum_total_bytes as f32
                            })
                            .desired_width(finite_width(ui, 220.0)),
                        );
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(self.checksum_status_line());
                        });
                    }
                } else {
                    ui.label(
                        RichText::new("Image is invalid or missing")
                            .color(Color32::from_rgb(245, 119, 98)),
                    );
                }

                if let Some(device) = &device {
                    detail_row(ui, "Target", &device.path);
                    detail_row(ui, "Model", device.model.as_deref().unwrap_or("unknown"));
                    detail_row(ui, "Target size", &format_size(device.size_bytes));
                    detail_row(
                        ui,
                        "Transport",
                        device.transport.as_deref().unwrap_or("unknown"),
                    );
                    detail_row(ui, "Risk", device.risk_label());
                    detail_row(ui, "Mountpoints", &format_mountpoints_recursive(device));
                    if device.has_mountpoints_recursive() {
                        ui.label(
                            RichText::new("Mounted target is blocked.")
                                .color(Color32::from_rgb(245, 119, 98)),
                        );
                    }
                } else {
                    ui.label(
                        RichText::new("Target is missing").color(Color32::from_rgb(245, 119, 98)),
                    );
                }

                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "Press Flash now to request administrator authorization through polkit.",
                    )
                    .color(Color32::from_rgb(148, 156, 154)),
                );

                if !checksum_ready {
                    ui.label(
                        RichText::new("Flash is locked until SHA256 pre-flight is complete.")
                            .color(Color32::from_rgb(245, 177, 66)),
                    );
                }

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.show_preflight = false;
                    }

                    if ui
                        .add_enabled(can_flash, egui::Button::new("Flash now"))
                        .clicked()
                    {
                        self.start_flash();
                    }
                });
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
                FlashEvent::Phase { name, total_bytes } => {
                    self.phase_name = name.clone();
                    self.phase_done_bytes = 0;
                    self.phase_total_bytes = total_bytes;
                    self.phase_started_at = Some(Instant::now());
                    self.progress = 0.0;
                    self.status = format!("{name} image");
                }
                FlashEvent::Progress {
                    done_bytes,
                    total_bytes,
                } => {
                    self.phase_done_bytes = done_bytes;
                    self.phase_total_bytes = total_bytes;
                    self.progress = (done_bytes as f32 / total_bytes as f32).clamp(0.0, 1.0);
                }
                FlashEvent::Finished(result) => {
                    self.running = false;
                    self.cancel_flag = None;
                    self.phase_started_at = None;
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

    fn poll_checksum_events(&mut self) {
        let Some(receiver) = self.checksum_receiver.take() else {
            return;
        };

        let mut keep_receiver = true;
        while let Ok(event) = receiver.try_recv() {
            match event {
                ChecksumEvent::Progress {
                    done_bytes,
                    total_bytes,
                } => {
                    self.checksum_done_bytes = done_bytes;
                    self.checksum_total_bytes = total_bytes;
                    self.progress = if total_bytes == 0 {
                        0.0
                    } else {
                        done_bytes as f32 / total_bytes as f32
                    };
                    self.status = self.checksum_status_line();
                }
                ChecksumEvent::Finished { path, result } => {
                    keep_receiver = false;
                    if path != self.image_path.trim() {
                        continue;
                    }
                    self.checksum_running = false;
                    self.checksum_done_bytes = self.checksum_total_bytes;
                    self.progress = 1.0;
                    match result {
                        Ok((hash, status)) => {
                            self.image_sha256 = hash;
                            self.image_sha256_path = Some(path);
                            self.checksum_status = Some(status);
                            self.status = "Review pre-flight confirmation".to_string();
                            self.last_error = None;
                        }
                        Err(err) => {
                            self.image_sha256 = None;
                            self.image_sha256_path = None;
                            self.checksum_status = None;
                            self.status = "Pre-flight failed".to_string();
                            self.last_error = Some(err);
                        }
                    }
                }
            }
        }

        if keep_receiver {
            self.checksum_receiver = Some(receiver);
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

fn format_bytes(bytes: u64) -> String {
    let gib = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    if gib >= 1.0 {
        format!("{gib:.1} GiB")
    } else {
        let mib = bytes as f64 / 1024.0 / 1024.0;
        if mib >= 1.0 {
            format!("{mib:.1} MiB")
        } else {
            format!("{bytes} B")
        }
    }
}

fn format_duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn finite_width(ui: &egui::Ui, min_width: f32) -> f32 {
    let width = ui.available_width();
    if width.is_finite() {
        width.max(min_width)
    } else {
        min_width
    }
}

fn format_mountpoints(mountpoints: &[String]) -> String {
    if mountpoints.is_empty() {
        "-".to_string()
    } else {
        mountpoints.join(", ")
    }
}

fn format_mountpoints_recursive(device: &BlockDevice) -> String {
    let mut mountpoints = device.mountpoints.clone();
    collect_child_mountpoints(device, &mut mountpoints);
    format_mountpoints(&mountpoints)
}

fn collect_child_mountpoints(device: &BlockDevice, mountpoints: &mut Vec<String>) {
    for child in &device.children {
        mountpoints.extend(child.mountpoints.iter().cloned());
        collect_child_mountpoints(child, mountpoints);
    }
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_min_width(500.0);
        ui.label(
            RichText::new(label)
                .font(FontId::proportional(13.0))
                .color(Color32::from_rgb(148, 156, 154)),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .font(FontId::monospace(13.0))
                    .color(Color32::from_rgb(232, 238, 233)),
            );
        });
    });
}

fn checksum_row(ui: &mut egui::Ui, status: Option<&ChecksumStatus>) {
    match status {
        Some(ChecksumStatus::Match { expected }) => detail_row(ui, "SHA256 sidecar", expected),
        Some(ChecksumStatus::Mismatch { expected }) => {
            detail_row(ui, "SHA256 sidecar", expected);
            ui.label(
                RichText::new("SHA256 sidecar mismatch. Flashing is blocked by pre-flight.")
                    .color(Color32::from_rgb(245, 119, 98)),
            );
        }
        Some(ChecksumStatus::Missing) | None => detail_row(ui, "SHA256 sidecar", "not found"),
    }
}

fn run_helper_with_pkexec(
    sender: &mpsc::Sender<FlashEvent>,
    image_path: &PathBuf,
    device_path: &PathBuf,
    chunk_size_mib: u64,
    verify_after_write: bool,
    force: bool,
    helper_child: &Arc<Mutex<Option<Child>>>,
) -> std::result::Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    log_gui_event(format!("run_helper_with_pkexec: exe={}", exe.display()));
    let mut command = pkexec_command()?;
    command
        .arg(exe)
        .arg("writer-helper")
        .arg("--image")
        .arg(image_path)
        .arg("--device")
        .arg(device_path)
        .arg("--chunk-size-mib")
        .arg(chunk_size_mib.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if verify_after_write {
        command.arg("--verify");
    }
    if force {
        command.arg("--force");
    }

    let mut child = command.spawn().map_err(|err| {
        log_gui_event(format!("run_helper_with_pkexec: spawn failed: {err}"));
        err.to_string()
    })?;
    log_gui_event(format!(
        "run_helper_with_pkexec: spawned helper pid={}",
        child.id()
    ));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to read writer helper stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to read writer helper stderr".to_string())?;
    if let Ok(mut slot) = helper_child.lock() {
        *slot = Some(child);
    }
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        parse_helper_line(sender, &line.map_err(|err| err.to_string())?);
    }

    let status = {
        let mut child = helper_child
            .lock()
            .map_err(|_| "failed to lock writer helper handle".to_string())?
            .take()
            .ok_or_else(|| "writer helper process was not available".to_string())?;
        child.wait().map_err(|err| err.to_string())?
    };
    if status.success() {
        let _ = sender.send(FlashEvent::Finished(Ok(())));
        Ok(())
    } else {
        let mut stderr_reader = BufReader::new(stderr);
        let mut stderr = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr_reader, &mut stderr);
        let stderr = stderr.trim().to_string();
        Err(if stderr.is_empty() {
            "writer helper failed or authorization was cancelled".to_string()
        } else {
            privilege_error_message(
                "writer helper failed or authorization was cancelled",
                &stderr,
            )
        })
    }
}

fn pkexec_command() -> std::result::Result<Command, String> {
    let Some(pkexec) = find_pkexec() else {
        return Err(format!(
            "root privileges are required, but no pkexec binary was found. Checked: {}",
            pkexec_candidates().join(", ")
        ));
    };
    log_gui_event(format!("pkexec_command: using {pkexec}"));
    let mut command = Command::new(pkexec);
    pass_gui_environment(&mut command);
    command.stdin(Stdio::null());
    Ok(command)
}

fn find_pkexec() -> Option<String> {
    if command_exists("pkexec") {
        return Some("pkexec".to_string());
    }

    pkexec_candidates()
        .into_iter()
        .find(|candidate| PathBuf::from(candidate).is_file())
        .map(str::to_string)
}

fn pkexec_candidates() -> Vec<&'static str> {
    vec!["/usr/bin/pkexec", "/bin/pkexec", "/usr/local/bin/pkexec"]
}

fn pass_gui_environment(command: &mut Command) {
    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
    ] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
}

fn privilege_error_message(fallback: &str, stderr: &str) -> String {
    if stderr.is_empty() {
        return fallback.to_string();
    }

    let lower = stderr.to_lowercase();
    if lower.contains("no authentication agent") || lower.contains("no polkit authentication agent")
    {
        return format!(
            "{stderr}\n\nNo polkit authentication agent answered the request. {}",
            polkit_agent_hint()
        );
    }

    if lower.contains("not authorized") || lower.contains("authorization") {
        return format!(
            "{stderr}\n\nIf you are running from cargo, install the desktop policy and run /usr/local/bin/eutheretcher gui for the cleanest polkit prompt."
        );
    }

    stderr.to_string()
}

fn polkit_agent_hint() -> String {
    let found: Vec<&str> = known_polkit_agents()
        .into_iter()
        .filter(|agent| agent.is_available())
        .map(|agent| agent.name)
        .collect();

    if found.is_empty() {
        "No common agent binary was found. Install and start one of: KDE polkit agent, GNOME polkit agent, LXQt policykit agent, MATE polkit agent, or xfce-polkit.".to_string()
    } else {
        format!(
            "Found common agent binaries: {}. Make sure one authentication agent is running in your desktop session.",
            found.join(", ")
        )
    }
}

struct PolkitAgent {
    name: &'static str,
    command: &'static str,
    paths: &'static [&'static str],
}

impl PolkitAgent {
    fn is_available(&self) -> bool {
        command_exists(self.command) || self.paths.iter().any(|path| PathBuf::from(path).is_file())
    }
}

fn known_polkit_agents() -> Vec<PolkitAgent> {
    vec![
        PolkitAgent {
            name: "KDE polkit agent",
            command: "polkit-kde-authentication-agent-1",
            paths: &["/usr/lib/polkit-kde-authentication-agent-1"],
        },
        PolkitAgent {
            name: "GNOME polkit agent",
            command: "polkit-gnome-authentication-agent-1",
            paths: &[
                "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1",
                "/usr/libexec/polkit-gnome-authentication-agent-1",
            ],
        },
        PolkitAgent {
            name: "LXQt policykit agent",
            command: "lxqt-policykit-agent",
            paths: &["/usr/bin/lxqt-policykit-agent"],
        },
        PolkitAgent {
            name: "MATE polkit agent",
            command: "polkit-mate-authentication-agent-1",
            paths: &["/usr/lib/polkit-mate/polkit-mate-authentication-agent-1"],
        },
        PolkitAgent {
            name: "xfce-polkit",
            command: "xfce-polkit",
            paths: &["/usr/lib/xfce-polkit/xfce-polkit"],
        },
    ]
}

fn parse_helper_line(sender: &mpsc::Sender<FlashEvent>, line: &str) {
    let mut parts = line.split('\t');
    match parts.next() {
        Some("PHASE") => {
            let Some(name) = parts.next() else {
                return;
            };
            let total_bytes = parts
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let _ = sender.send(FlashEvent::Phase {
                name: name.to_string(),
                total_bytes,
            });
        }
        Some("PROGRESS") => {
            let done_bytes = parts
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let total_bytes = parts
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let _ = sender.send(FlashEvent::Progress {
                done_bytes,
                total_bytes,
            });
        }
        Some("ERROR") => {
            let code = parts.next().unwrap_or("FAILED");
            let message = parts.next().unwrap_or("writer helper failed");
            let _ = sender.send(FlashEvent::Finished(Err(format!("{code}: {message}"))));
        }
        Some("DONE") => {}
        _ => {}
    }
}

fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {command}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
