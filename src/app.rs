use crate::audio::AudioEngine;
use crate::decoder::{self, AudioData};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

const OVERVIEW_BUCKETS: usize = 8192;

// ── Color theme ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppColors {
    pub background: Color32,
    pub waveform: Color32,
    pub playhead: Color32,
    pub selection_fill: Color32,
    pub selection_edge: Color32,
    pub center_line: Color32,
}

impl Default for AppColors {
    fn default() -> Self {
        Self {
            background: Color32::from_rgb(18, 18, 28),
            waveform: Color32::from_rgb(80, 200, 120),
            playhead: Color32::WHITE,
            selection_fill: Color32::from_rgba_premultiplied(80, 140, 255, 45),
            selection_edge: Color32::from_rgb(100, 160, 255),
            center_line: Color32::from_rgb(40, 40, 60),
        }
    }
}

// ── App state ─────────────────────────────────────────────────────────────────

pub struct App {
    engine: Option<AudioEngine>,
    engine_error: Option<String>,

    audio: Option<AudioData>,
    file_name: String,

    overview_min: Vec<f32>,
    overview_max: Vec<f32>,

    view_start: f64,
    view_end: f64,

    selection: Option<(f64, f64)>,
    drag_anchor: Option<f64>,

    speed: f32,
    volume: f32,

    colors: AppColors,
    show_colors: bool,

    status: String,
}

impl App {
    pub fn new() -> Self {
        let (engine, engine_error) = match AudioEngine::new() {
            Ok(e) => (Some(e), None),
            Err(e) => (None, Some(e.to_string())),
        };
        App {
            engine,
            engine_error,
            audio: None,
            file_name: String::new(),
            overview_min: Vec::new(),
            overview_max: Vec::new(),
            view_start: 0.0,
            view_end: 1.0,
            selection: None,
            drag_anchor: None,
            speed: 1.0,
            volume: 1.0,
            colors: AppColors::default(),
            show_colors: false,
            status: String::new(),
        }
    }

    // ── File loading ──────────────────────────────────────────────────────────

    fn load_file(&mut self, path: &std::path::Path) {
        match decoder::decode_file(&path.to_string_lossy()) {
            Err(e) => self.status = format!("Error: {e}"),
            Ok(raw) => {
                let target = self
                    .engine
                    .as_ref()
                    .map(|e| e.output_sample_rate)
                    .unwrap_or(raw.sample_rate);

                let audio = if raw.sample_rate != target {
                    decoder::resample(&raw, target)
                } else {
                    raw
                };

                let total = audio.total_frames();
                self.compute_overview(&audio);
                self.view_start = 0.0;
                self.view_end = total as f64;
                self.selection = None;

                self.file_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                self.status = format!(
                    "Loaded: {} ({:.1}s, {} Hz, {} ch)",
                    self.file_name,
                    audio.duration_secs(),
                    audio.sample_rate,
                    audio.channels
                );

                if let Some(engine) = &self.engine {
                    let mut st = engine.state.lock().unwrap();
                    st.samples = audio.samples.clone();
                    st.channels = audio.channels;
                    st.seek_to(0.0);
                    st.is_playing = false;
                    st.selection = None;
                }

                self.audio = Some(audio);
            }
        }
    }

    // ── Overview ──────────────────────────────────────────────────────────────

    fn compute_overview(&mut self, audio: &AudioData) {
        let total = audio.total_frames();
        if total == 0 {
            self.overview_min.clear();
            self.overview_max.clear();
            return;
        }
        let buckets = OVERVIEW_BUCKETS.min(total);
        let mut mins = vec![f32::MAX; buckets];
        let mut maxs = vec![f32::MIN; buckets];
        let ch = audio.channels;

        for frame in 0..total {
            let b = ((frame as f64 * buckets as f64 / total as f64) as usize).min(buckets - 1);
            let mut sum = 0.0f32;
            for c in 0..ch {
                sum += audio.samples[frame * ch + c];
            }
            let avg = sum / ch as f32;
            if avg < mins[b] { mins[b] = avg; }
            if avg > maxs[b] { maxs[b] = avg; }
        }
        for i in 0..buckets {
            if mins[i] == f32::MAX { mins[i] = 0.0; }
            if maxs[i] == f32::MIN { maxs[i] = 0.0; }
        }
        self.overview_min = mins;
        self.overview_max = maxs;
    }

    fn waveform_min_max(&self, fs: f64, fe: f64) -> (f32, f32) {
        let audio = match &self.audio { Some(a) => a, None => return (0.0, 0.0) };
        let total = audio.total_frames();
        if total == 0 { return (0.0, 0.0); }

        let ob = self.overview_min.len();

        if fe - fs >= 1.0 && ob > 0 {
            let b0 = ((fs / total as f64) * ob as f64) as usize;
            let b1 = (((fe / total as f64) * ob as f64) as usize + 1).min(ob);
            let b0 = b0.min(ob - 1);
            let (mut mn, mut mx) = (f32::MAX, f32::MIN);
            for b in b0..b1 {
                if self.overview_min[b] < mn { mn = self.overview_min[b]; }
                if self.overview_max[b] > mx { mx = self.overview_max[b]; }
            }
            (if mn == f32::MAX { 0.0 } else { mn }, if mx == f32::MIN { 0.0 } else { mx })
        } else {
            let ch = audio.channels;
            let i0 = (fs as usize).min(total);
            let i1 = (fe.ceil() as usize + 1).min(total);
            let (mut mn, mut mx) = (f32::MAX, f32::MIN);
            for frame in i0..i1 {
                if frame >= total { break; }
                let mut sum = 0.0f32;
                for c in 0..ch { sum += audio.samples[frame * ch + c]; }
                let avg = sum / ch as f32;
                if avg < mn { mn = avg; }
                if avg > mx { mx = avg; }
            }
            (if mn == f32::MAX { 0.0 } else { mn }, if mx == f32::MIN { 0.0 } else { mx })
        }
    }

    // ── View helpers ──────────────────────────────────────────────────────────

    fn frame_to_x(&self, frame: f64, rect: Rect) -> f32 {
        let span = (self.view_end - self.view_start).max(1.0);
        rect.left() + ((frame - self.view_start) / span) as f32 * rect.width()
    }

    fn x_to_frame(&self, x: f32, rect: Rect) -> f64 {
        let t = ((x - rect.left()) / rect.width()) as f64;
        self.view_start + t * (self.view_end - self.view_start)
    }

    fn zoom(&mut self, factor: f64, center: f64) {
        let total = self.audio.as_ref().map(|a| a.total_frames() as f64).unwrap_or(1.0);
        let span = ((self.view_end - self.view_start) * factor).clamp(100.0, total);
        self.view_start = (center - span * 0.5).max(0.0);
        self.view_end = (self.view_start + span).min(total);
        if self.view_end == total { self.view_start = (total - span).max(0.0); }
    }

    // ── Engine accessors ──────────────────────────────────────────────────────

    fn playback_pos(&self) -> f64 {
        self.engine.as_ref().map(|e| e.state.lock().unwrap().playback_pos).unwrap_or(0.0)
    }

    fn is_playing(&self) -> bool {
        self.engine.as_ref().map(|e| e.state.lock().unwrap().is_playing).unwrap_or(false)
    }

    fn toggle_play(&mut self) {
        if let Some(engine) = &self.engine {
            let mut st = engine.state.lock().unwrap();
            if st.samples.is_empty() { return; }
            if st.is_playing {
                st.is_playing = false;
            } else {
                let (start, end) = st.play_range();
                if st.playback_pos >= end || st.playback_pos < start {
                    st.seek_to(start);
                }
                st.is_playing = true;
            }
        }
    }

    fn stop(&mut self) {
        if let Some(engine) = &self.engine {
            let mut st = engine.state.lock().unwrap();
            st.is_playing = false;
            st.seek_to_start();
        }
    }

    fn play_from_beginning(&mut self) {
        if let Some(engine) = &self.engine {
            let mut st = engine.state.lock().unwrap();
            if st.samples.is_empty() { return; }
            st.seek_to(0.0);
            st.is_playing = true;
        }
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.sync_selection();
    }

    fn sync_selection(&mut self) {
        if let Some(engine) = &self.engine {
            engine.state.lock().unwrap().selection = self.selection;
        }
    }

    fn sync_speed(&mut self) {
        if let Some(engine) = &self.engine {
            engine.state.lock().unwrap().set_speed(self.speed as f64);
        }
    }

    fn sync_volume(&mut self) {
        if let Some(engine) = &self.engine {
            engine.state.lock().unwrap().volume = self.volume;
        }
    }
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.is_playing() { ctx.request_repaint(); }

        // Spacebar → toggle play/pause (consumed so it doesn't reach text fields)
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
            self.toggle_play();
        }

        // ── Top bar ───────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open File…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio", &["mp3", "flac", "wav", "ogg", "aac", "m4a", "opus"])
                        .pick_file()
                    {
                        self.load_file(&path);
                    }
                }
                if !self.file_name.is_empty() { ui.label(&self.file_name); }
                if let Some(err) = &self.engine_error {
                    ui.colored_label(Color32::RED, format!("Audio error: {err}"));
                }
            });
        });

        // ── Bottom controls ───────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("controls")
            .min_height(110.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);

                // Transport buttons — larger, centered
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;

                        let btn = |label: &str| {
                            egui::Button::new(
                                egui::RichText::new(label).size(16.0)
                            )
                        };
                        let bsz = Vec2::new(110.0, 40.0);

                        if ui.add_sized(bsz, btn("⏮  Start")).clicked() {
                            self.play_from_beginning();
                        }

                        let play_label = if self.is_playing() { "⏸  Pause" } else { "▶  Play" };
                        if ui.add_sized(bsz, btn(play_label)).clicked() {
                            self.toggle_play();
                        }

                        if ui.add_sized(bsz, btn("⏹  Stop")).clicked() {
                            self.stop();
                        }

                        let loop_mode = self.engine.as_ref()
                            .map(|e| e.state.lock().unwrap().loop_mode)
                            .unwrap_or(false);
                        let mut lm = loop_mode;
                        let loop_btn = egui::Button::new(
                            egui::RichText::new(if lm { "🔁  Loop ON" } else { "🔁  Loop OFF" }).size(16.0)
                        );
                        if ui.add_sized(bsz, loop_btn).clicked() {
                            lm = !lm;
                            if let Some(engine) = &self.engine {
                                engine.state.lock().unwrap().loop_mode = lm;
                            }
                        }

                        if ui.add_sized(bsz, btn("✕  Deselect")).clicked() {
                            self.clear_selection();
                        }
                    });
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);

                // Speed, zoom, colors row
                ui.horizontal(|ui| {
                    ui.label("Speed:");
                    let old = self.speed;
                    let speed_label = format!("{:.2}×", self.speed);
                    let slider = egui::Slider::new(&mut self.speed, 0.05_f32..=2.0_f32)
                        .logarithmic(true)
                        .text(speed_label)
                        .min_decimals(2);
                    if ui.add(slider).changed() || (self.speed - old).abs() > 1e-5 {
                        self.sync_speed();
                    }
                    if ui.small_button("1×").clicked() {
                        self.speed = 1.0;
                        self.sync_speed();
                    }

                    ui.add_space(16.0);
                    ui.label("Volume:");
                    let vol_label = format!("{:.0}%", self.volume * 100.0);
                    let vol_slider = egui::Slider::new(&mut self.volume, 0.0_f32..=1.0_f32)
                        .text(vol_label);
                    if ui.add(vol_slider).changed() {
                        self.sync_volume();
                    }

                    ui.add_space(16.0);
                    ui.label("Zoom:");
                    if ui.small_button("🔎+").clicked() {
                        let c = (self.view_start + self.view_end) * 0.5;
                        self.zoom(0.5, c);
                    }
                    if ui.small_button("🔎-").clicked() {
                        let c = (self.view_start + self.view_end) * 0.5;
                        self.zoom(2.0, c);
                    }
                    if ui.small_button("Fit").clicked() {
                        if let Some(a) = &self.audio {
                            self.view_start = 0.0;
                            self.view_end = a.total_frames() as f64;
                        }
                    }
                    if ui.small_button("Sel→View").clicked() {
                        if let Some((s, e)) = self.selection {
                            let pad = (e - s) * 0.05;
                            self.view_start = (s - pad).max(0.0);
                            self.view_end = if let Some(a) = &self.audio {
                                (e + pad).min(a.total_frames() as f64)
                            } else { e + pad };
                        }
                    }

                    ui.add_space(16.0);
                    if ui.small_button("🎨 Colors…").clicked() {
                        self.show_colors = !self.show_colors;
                    }

                    // Time display (right-aligned by filling remaining space)
                    if let Some(audio) = &self.audio {
                        let pos_s = self.playback_pos() / audio.sample_rate as f64;
                        let tot_s = audio.duration_secs();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("{} / {}", fmt_time(pos_s), fmt_time(tot_s)));
                        });
                    }
                });

                if !self.status.is_empty() {
                    ui.add_space(2.0);
                    ui.label(&self.status);
                }

                ui.add_space(4.0);
            });

        // ── Colors window ─────────────────────────────────────────────────────
        if self.show_colors {
            egui::Window::new("Colors")
                .open(&mut self.show_colors)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("color_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Waveform");
                        ui.color_edit_button_srgba(&mut self.colors.waveform);
                        ui.end_row();

                        ui.label("Playhead");
                        ui.color_edit_button_srgba(&mut self.colors.playhead);
                        ui.end_row();

                        ui.label("Selection fill");
                        ui.color_edit_button_srgba(&mut self.colors.selection_fill);
                        ui.end_row();

                        ui.label("Selection edge");
                        ui.color_edit_button_srgba(&mut self.colors.selection_edge);
                        ui.end_row();

                        ui.label("Center line");
                        ui.color_edit_button_srgba(&mut self.colors.center_line);
                        ui.end_row();

                        ui.label("Waveform background");
                        ui.color_edit_button_srgba(&mut self.colors.background);
                        ui.end_row();
                    });

                    ui.add_space(4.0);
                    if ui.button("Reset to defaults").clicked() {
                        self.colors = AppColors::default();
                    }
                });
        }

        // ── Waveform panel ────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(self.colors.background))
            .show(ctx, |ui| {
                self.draw_waveform(ui);
            });
    }
}

// ── Waveform ──────────────────────────────────────────────────────────────────

impl App {
    fn draw_waveform(&mut self, ui: &mut egui::Ui) {
        let desired = Vec2::new(ui.available_width(), ui.available_height());
        let (rect, response) = ui.allocate_exact_size(desired, Sense::click_and_drag());

        if !ui.is_rect_visible(rect) { return; }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.colors.background);

        // Center line
        painter.line_segment(
            [Pos2::new(rect.left(), rect.center().y), Pos2::new(rect.right(), rect.center().y)],
            Stroke::new(1.0, self.colors.center_line),
        );

        if self.audio.is_none() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Open an audio file to begin",
                egui::FontId::proportional(18.0),
                Color32::from_rgb(120, 120, 140),
            );
            return;
        }

        let total = self.audio.as_ref().unwrap().total_frames() as f64;
        let width = rect.width() as usize;
        let span = (self.view_end - self.view_start).max(1.0);

        // Waveform columns
        for px in 0..width {
            let fs = self.view_start + (px as f64 / width as f64) * span;
            let fe = self.view_start + ((px + 1) as f64 / width as f64) * span;
            let (mn, mx) = self.waveform_min_max(fs, fe);

            let x = rect.left() + px as f32;
            let mid = rect.center().y;
            let hh = rect.height() * 0.5;
            let y_top = (mid - mx * hh).clamp(rect.top(), rect.bottom());
            let y_bot = (mid - mn * hh).clamp(rect.top(), rect.bottom());

            painter.line_segment(
                [Pos2::new(x, y_top), Pos2::new(x, y_bot)],
                Stroke::new(1.0, self.colors.waveform),
            );
        }

        // Selection overlay
        if let Some((s, e)) = self.selection {
            let x0 = self.frame_to_x(s, rect).clamp(rect.left(), rect.right());
            let x1 = self.frame_to_x(e, rect).clamp(rect.left(), rect.right());
            if x1 > x0 {
                let sr = Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(x1, rect.bottom()));
                painter.rect_filled(sr, 0.0, self.colors.selection_fill);
                painter.rect_stroke(sr, 0.0, Stroke::new(1.0, self.colors.selection_edge));
            }
        }

        // Playhead
        let ph = self.frame_to_x(self.playback_pos(), rect);
        if ph >= rect.left() && ph <= rect.right() {
            painter.line_segment(
                [Pos2::new(ph, rect.top()), Pos2::new(ph, rect.bottom())],
                Stroke::new(2.0, self.colors.playhead),
            );
        }

        // ── Interactions ──────────────────────────────────────────────────────

        // Ctrl+scroll → zoom; plain scroll → pan
        let hover = ui.input(|i| i.pointer.hover_pos().unwrap_or_default());
        if rect.contains(hover) {
            let ctrl = ui.input(|i| i.modifiers.ctrl);
            let scroll = ui.input(|i| i.smooth_scroll_delta);

            if ctrl && scroll.y.abs() > 0.0 {
                let cf = ui.input(|i| i.pointer.hover_pos())
                    .map(|p| self.x_to_frame(p.x, rect))
                    .unwrap_or((self.view_start + self.view_end) * 0.5);
                let f = if scroll.y > 0.0 { 0.8 } else { 1.25 };
                self.zoom(f, cf);
            } else if scroll.x.abs() > 0.0 || (!ctrl && scroll.y.abs() > 0.0) {
                let delta = if scroll.x.abs() > 0.0 { scroll.x } else { -scroll.y };
                let delta_f = (delta as f64 / rect.width() as f64) * span;
                let max_s = (total - span).max(0.0);
                self.view_start = (self.view_start + delta_f).clamp(0.0, max_s);
                self.view_end = self.view_start + span;
            }
        }

        // Pinch zoom
        let zd = ui.input(|i| i.zoom_delta());
        if zd != 1.0 {
            let cf = ui.input(|i| i.pointer.hover_pos())
                .map(|p| self.x_to_frame(p.x, rect))
                .unwrap_or((self.view_start + self.view_end) * 0.5);
            self.zoom(1.0 / zd as f64, cf);
        }

        // Drag → selection
        if response.drag_started() {
            if let Some(p) = response.interact_pointer_pos() {
                self.drag_anchor = Some(self.x_to_frame(p.x, rect));
                self.selection = None;
                self.sync_selection();
            }
        }
        if response.dragged() {
            if let (Some(anchor), Some(p)) = (self.drag_anchor, response.interact_pointer_pos()) {
                let f = self.x_to_frame(p.x, rect);
                let (s, e) = if f < anchor { (f, anchor) } else { (anchor, f) };
                self.selection = Some((s.max(0.0), e.min(total)));
                self.sync_selection();
            }
        }
        if response.drag_stopped() {
            self.drag_anchor = None;
            if let Some((s, e)) = self.selection {
                if e - s < 10.0 { self.clear_selection(); }
            }
        }

        // Double-click → clear selection
        if response.double_clicked() { self.clear_selection(); }

        // Single click (no drag) → seek
        if response.clicked() && self.drag_anchor.is_none() {
            if let Some(p) = response.interact_pointer_pos() {
                let f = self.x_to_frame(p.x, rect).clamp(0.0, total);
                if let Some(engine) = &self.engine {
                    engine.state.lock().unwrap().seek_to(f);
                }
            }
        }
    }
}

fn fmt_time(secs: f64) -> String {
    let s = secs as u64;
    let ms = ((secs - s as f64) * 100.0) as u64;
    let (m, s) = (s / 60, s % 60);
    format!("{m}:{s:02}.{ms:02}")
}
