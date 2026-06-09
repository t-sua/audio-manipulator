use crate::audio::AudioEngine;
use crate::decoder::{self, AudioData};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

const OVERVIEW_BUCKETS: usize = 8192;
const WAVEFORM_HEIGHT: f32 = 400.0;
const BG_COLOR: Color32 = Color32::from_rgb(18, 18, 28);
const WAVE_COLOR: Color32 = Color32::from_rgb(80, 200, 120);
const SEL_FILL: Color32 = Color32::from_rgba_premultiplied(80, 140, 255, 45);
const SEL_EDGE: Color32 = Color32::from_rgb(100, 160, 255);
const PLAYHEAD_COLOR: Color32 = Color32::WHITE;

pub struct App {
    engine: Option<AudioEngine>,
    engine_error: Option<String>,

    audio: Option<AudioData>,
    file_name: String,

    /// Pre-computed overview min/max per bucket (mixed to mono).
    overview_min: Vec<f32>,
    overview_max: Vec<f32>,

    /// Visible frame range.
    view_start: f64,
    view_end: f64,

    /// Selection in frames.
    selection: Option<(f64, f64)>,
    drag_anchor: Option<f64>,

    speed: f32,
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
            status: String::new(),
        }
    }

    fn load_file(&mut self, path: &std::path::Path) {
        let path_str = path.to_string_lossy().to_string();
        match decoder::decode_file(&path_str) {
            Err(e) => {
                self.status = format!("Error loading file: {e}");
            }
            Ok(raw) => {
                let target_rate = self
                    .engine
                    .as_ref()
                    .map(|e| e.output_sample_rate)
                    .unwrap_or(raw.sample_rate);

                let audio = if raw.sample_rate != target_rate {
                    decoder::resample(&raw, target_rate)
                } else {
                    raw
                };

                let total_frames = audio.total_frames();
                self.compute_overview(&audio);
                self.view_start = 0.0;
                self.view_end = total_frames as f64;
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
                    st.playback_pos = 0.0;
                    st.is_playing = false;
                    st.selection = None;
                }

                self.audio = Some(audio);
            }
        }
    }

    fn compute_overview(&mut self, audio: &AudioData) {
        let total_frames = audio.total_frames();
        if total_frames == 0 {
            self.overview_min.clear();
            self.overview_max.clear();
            return;
        }

        let buckets = OVERVIEW_BUCKETS.min(total_frames);
        let mut mins = vec![f32::MAX; buckets];
        let mut maxs = vec![f32::MIN; buckets];
        let ch = audio.channels;

        for frame in 0..total_frames {
            let b = (frame as f64 * buckets as f64 / total_frames as f64) as usize;
            let b = b.min(buckets - 1);

            let mut sum = 0.0f32;
            for c in 0..ch {
                sum += audio.samples[frame * ch + c];
            }
            let avg = sum / ch as f32;

            if avg < mins[b] {
                mins[b] = avg;
            }
            if avg > maxs[b] {
                maxs[b] = avg;
            }
        }

        for i in 0..buckets {
            if mins[i] == f32::MAX {
                mins[i] = 0.0;
            }
            if maxs[i] == f32::MIN {
                maxs[i] = 0.0;
            }
        }

        self.overview_min = mins;
        self.overview_max = maxs;
    }

    fn waveform_min_max(&self, frame_start: f64, frame_end: f64) -> (f32, f32) {
        let audio = match &self.audio {
            Some(a) => a,
            None => return (0.0, 0.0),
        };

        let total = audio.total_frames();
        if total == 0 {
            return (0.0, 0.0);
        }

        let frames_span = frame_end - frame_start;
        let ob = self.overview_min.len();

        if frames_span >= 1.0 && ob > 0 {
            let b0 = ((frame_start / total as f64) * ob as f64) as usize;
            let b1 = (((frame_end / total as f64) * ob as f64) as usize + 1).min(ob);
            let b0 = b0.min(ob - 1);

            let mut mn = f32::MAX;
            let mut mx = f32::MIN;
            for b in b0..b1 {
                if self.overview_min[b] < mn {
                    mn = self.overview_min[b];
                }
                if self.overview_max[b] > mx {
                    mx = self.overview_max[b];
                }
            }
            (
                if mn == f32::MAX { 0.0 } else { mn },
                if mx == f32::MIN { 0.0 } else { mx },
            )
        } else {
            let i0 = (frame_start as usize).min(total);
            let i1 = (frame_end.ceil() as usize + 1).min(total);
            let ch = audio.channels;

            let mut mn = f32::MAX;
            let mut mx = f32::MIN;

            for frame in i0..i1 {
                if frame >= total {
                    break;
                }
                let mut sum = 0.0f32;
                for c in 0..ch {
                    sum += audio.samples[frame * ch + c];
                }
                let avg = sum / ch as f32;
                if avg < mn {
                    mn = avg;
                }
                if avg > mx {
                    mx = avg;
                }
            }

            (
                if mn == f32::MAX { 0.0 } else { mn },
                if mx == f32::MIN { 0.0 } else { mx },
            )
        }
    }

    fn frame_to_x(&self, frame: f64, rect: Rect) -> f32 {
        let span = (self.view_end - self.view_start).max(1.0);
        rect.left() + ((frame - self.view_start) / span) as f32 * rect.width()
    }

    fn x_to_frame(&self, x: f32, rect: Rect) -> f64 {
        let t = ((x - rect.left()) / rect.width()) as f64;
        self.view_start + t * (self.view_end - self.view_start)
    }

    fn zoom(&mut self, factor: f64, center_frame: f64) {
        let total = self
            .audio
            .as_ref()
            .map(|a| a.total_frames() as f64)
            .unwrap_or(1.0);

        let span = (self.view_end - self.view_start) * factor;
        let span = span.clamp(100.0, total);

        self.view_start = (center_frame - span * 0.5).max(0.0);
        self.view_end = (self.view_start + span).min(total);
        if self.view_end == total {
            self.view_start = (total - span).max(0.0);
        }
    }

    fn playback_pos(&self) -> f64 {
        self.engine
            .as_ref()
            .map(|e| e.state.lock().unwrap().playback_pos)
            .unwrap_or(0.0)
    }

    fn is_playing(&self) -> bool {
        self.engine
            .as_ref()
            .map(|e| e.state.lock().unwrap().is_playing)
            .unwrap_or(false)
    }

    fn toggle_play(&mut self) {
        if let Some(engine) = &self.engine {
            let mut st = engine.state.lock().unwrap();
            if st.samples.is_empty() {
                return;
            }
            if st.is_playing {
                st.is_playing = false;
            } else {
                let (start, end) = st.play_range();
                if st.playback_pos >= end || st.playback_pos < start {
                    st.playback_pos = start;
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

    fn sync_selection_to_engine(&mut self) {
        if let Some(engine) = &self.engine {
            let mut st = engine.state.lock().unwrap();
            st.selection = self.selection;
        }
    }

    fn sync_speed_to_engine(&mut self) {
        if let Some(engine) = &self.engine {
            let mut st = engine.state.lock().unwrap();
            st.speed = self.speed as f64;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request continuous repaint while playing so playhead moves.
        if self.is_playing() {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open File…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "Audio",
                            &["mp3", "flac", "wav", "ogg", "aac", "m4a", "opus"],
                        )
                        .pick_file()
                    {
                        self.load_file(&path);
                    }
                }

                if !self.file_name.is_empty() {
                    ui.label(&self.file_name);
                }

                if let Some(err) = &self.engine_error {
                    ui.colored_label(Color32::RED, format!("Audio engine error: {err}"));
                }
            });
        });

        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            ui.add_space(4.0);

            // Transport row
            ui.horizontal(|ui| {
                let playing = self.is_playing();

                let play_label = if playing { "⏸ Pause" } else { "▶ Play" };
                if ui.button(play_label).clicked() {
                    self.toggle_play();
                }

                if ui.button("⏹ Stop").clicked() {
                    self.stop();
                }

                let loop_mode = self
                    .engine
                    .as_ref()
                    .map(|e| e.state.lock().unwrap().loop_mode)
                    .unwrap_or(false);

                let mut lm = loop_mode;
                if ui.checkbox(&mut lm, "Loop").changed() {
                    if let Some(engine) = &self.engine {
                        engine.state.lock().unwrap().loop_mode = lm;
                    }
                }

                // Time display
                if let Some(audio) = &self.audio {
                    let pos_secs = self.playback_pos() / audio.sample_rate as f64;
                    let total_secs = audio.duration_secs();
                    ui.label(format!(
                        "{} / {}",
                        fmt_time(pos_secs),
                        fmt_time(total_secs)
                    ));
                }
            });

            ui.add_space(4.0);

            // Speed row
            ui.horizontal(|ui| {
                ui.label("Speed:");
                let old_speed = self.speed;
                let speed_label = format!("{:.2}×", self.speed);
                let slider = egui::Slider::new(&mut self.speed, 0.05_f32..=2.0_f32)
                    .logarithmic(true)
                    .text(speed_label);
                if ui.add(slider).changed() || (self.speed - old_speed).abs() > 1e-5 {
                    self.sync_speed_to_engine();
                }

                if ui.button("Reset").clicked() {
                    self.speed = 1.0;
                    self.sync_speed_to_engine();
                }

                ui.add_space(16.0);

                // Zoom controls
                ui.label("Zoom:");
                if ui.button("🔎+").clicked() {
                    let c = (self.view_start + self.view_end) * 0.5;
                    self.zoom(0.5, c);
                }
                if ui.button("🔎-").clicked() {
                    let c = (self.view_start + self.view_end) * 0.5;
                    self.zoom(2.0, c);
                }
                if ui.button("Fit").clicked() {
                    if let Some(audio) = &self.audio {
                        self.view_start = 0.0;
                        self.view_end = audio.total_frames() as f64;
                    }
                }

                if ui.button("Sel→View").clicked() {
                    if let Some((s, e)) = self.selection {
                        let pad = (e - s) * 0.05;
                        self.view_start = (s - pad).max(0.0);
                        self.view_end = if let Some(audio) = &self.audio {
                            (e + pad).min(audio.total_frames() as f64)
                        } else {
                            e + pad
                        };
                    }
                }
            });

            ui.add_space(2.0);

            if !self.status.is_empty() {
                ui.label(&self.status);
            }

            ui.add_space(4.0);
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_COLOR))
            .show(ctx, |ui| {
                self.draw_waveform(ui);
            });
    }
}

impl App {
    fn draw_waveform(&mut self, ui: &mut egui::Ui) {
        let desired = Vec2::new(ui.available_width(), WAVEFORM_HEIGHT.min(ui.available_height()));
        let (rect, response) =
            ui.allocate_exact_size(desired, Sense::click_and_drag());

        if !ui.is_rect_visible(rect) {
            return;
        }

        let painter = ui.painter_at(rect);

        // Background
        painter.rect_filled(rect, 0.0, BG_COLOR);

        // Center line
        painter.line_segment(
            [
                Pos2::new(rect.left(), rect.center().y),
                Pos2::new(rect.right(), rect.center().y),
            ],
            Stroke::new(1.0, Color32::from_rgb(40, 40, 60)),
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

        let total_frames = self.audio.as_ref().unwrap().total_frames() as f64;
        let width = rect.width() as usize;
        let span = (self.view_end - self.view_start).max(1.0);

        // Draw waveform columns
        for px in 0..width {
            let fs = self.view_start + (px as f64 / width as f64) * span;
            let fe = self.view_start + ((px + 1) as f64 / width as f64) * span;

            let (mn, mx) = self.waveform_min_max(fs, fe);

            let x = rect.left() + px as f32;
            let mid = rect.center().y;
            let half_h = rect.height() * 0.5;

            let y_top = (mid - mx * half_h).clamp(rect.top(), rect.bottom());
            let y_bot = (mid - mn * half_h).clamp(rect.top(), rect.bottom());

            painter.line_segment(
                [Pos2::new(x, y_top), Pos2::new(x, y_bot)],
                Stroke::new(1.0, WAVE_COLOR),
            );
        }

        // Selection overlay
        if let Some((sel_s, sel_e)) = self.selection {
            let x0 = self.frame_to_x(sel_s, rect).clamp(rect.left(), rect.right());
            let x1 = self.frame_to_x(sel_e, rect).clamp(rect.left(), rect.right());

            if x1 > x0 {
                let sel_rect = Rect::from_min_max(
                    Pos2::new(x0, rect.top()),
                    Pos2::new(x1, rect.bottom()),
                );
                painter.rect_filled(sel_rect, 0.0, SEL_FILL);
                painter.rect_stroke(sel_rect, 0.0, Stroke::new(1.0, SEL_EDGE));
            }
        }

        // Playhead
        let pos = self.playback_pos();
        let ph_x = self.frame_to_x(pos, rect);
        if ph_x >= rect.left() && ph_x <= rect.right() {
            painter.line_segment(
                [Pos2::new(ph_x, rect.top()), Pos2::new(ph_x, rect.bottom())],
                Stroke::new(2.0, PLAYHEAD_COLOR),
            );
        }

        // --- Interactions ---

        // Scroll to pan, Ctrl+scroll to zoom
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
        if rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())) {
            let ctrl = ui.input(|i| i.modifiers.ctrl);

            if ctrl && scroll_delta.y.abs() > 0.0 {
                let hover_frame = ui
                    .input(|i| i.pointer.hover_pos())
                    .map(|p| self.x_to_frame(p.x, rect))
                    .unwrap_or((self.view_start + self.view_end) * 0.5);

                let factor = if scroll_delta.y > 0.0 { 0.8 } else { 1.25 };
                self.zoom(factor, hover_frame);
            } else if scroll_delta.x.abs() > 0.0 || (!ctrl && scroll_delta.y.abs() > 0.0) {
                let delta_px = if scroll_delta.x.abs() > 0.0 {
                    scroll_delta.x
                } else {
                    -scroll_delta.y
                };
                let delta_frames = (delta_px as f64 / rect.width() as f64) * span;
                let max_start = (total_frames - span).max(0.0);
                self.view_start = (self.view_start + delta_frames).clamp(0.0, max_start);
                self.view_end = self.view_start + span;
            }
        }

        // Pinch-to-zoom
        let zoom_delta = ui.input(|i| i.zoom_delta());
        if zoom_delta != 1.0 {
            let center = ui
                .input(|i| i.pointer.hover_pos())
                .map(|p| self.x_to_frame(p.x, rect))
                .unwrap_or((self.view_start + self.view_end) * 0.5);
            self.zoom(1.0 / zoom_delta as f64, center);
        }

        // Click / drag for selection
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                let f = self.x_to_frame(pos.x, rect);
                self.drag_anchor = Some(f);
                self.selection = None;
                self.sync_selection_to_engine();
            }
        }

        if response.dragged() {
            if let (Some(anchor), Some(pos)) = (self.drag_anchor, response.interact_pointer_pos())
            {
                let f = self.x_to_frame(pos.x, rect);
                let (s, e) = if f < anchor { (f, anchor) } else { (anchor, f) };
                let total = total_frames;
                self.selection = Some((s.max(0.0), e.min(total)));
                self.sync_selection_to_engine();
            }
        }

        if response.drag_stopped() {
            // Keep selection but clear anchor
            self.drag_anchor = None;
            // If tiny selection (< 10 frames), clear it
            if let Some((s, e)) = self.selection {
                if (e - s) < 10.0 {
                    self.selection = None;
                    self.sync_selection_to_engine();
                }
            }
        }

        // Double-click clears selection
        if response.double_clicked() {
            self.selection = None;
            self.sync_selection_to_engine();
        }

        // Click without drag: seek playhead
        if response.clicked() && self.drag_anchor.is_none() {
            if let Some(pos) = response.interact_pointer_pos() {
                let f = self.x_to_frame(pos.x, rect);
                if let Some(engine) = &self.engine {
                    let mut st = engine.state.lock().unwrap();
                    st.playback_pos = f.clamp(0.0, total_frames);
                }
            }
        }
    }
}

fn fmt_time(secs: f64) -> String {
    let s = secs as u64;
    let ms = ((secs - s as f64) * 100.0) as u64;
    let m = s / 60;
    let s = s % 60;
    format!("{m}:{s:02}.{ms:02}")
}
