use std::path::PathBuf;

use eframe::{egui, wgpu};

use crate::colorimetry::{
    compute_transmitted_color, wavelength_to_srgb, ColorReadout, Illuminant, ToneMapper,
};
use crate::flask::{FlaskGeometry, FlaskVolume, SolventPreset, StopperType};
use crate::gpu::{save_rgba_png, CameraState, LightingPreset, SceneState, SpectralRenderer};
use crate::spectrum::{
    builtin_presets, parse_spectrum_file, AbsorbanceSpectrum, BaselineMode, LAMBDA_MAX,
    LAMBDA_MIN, SPECTRUM_LEN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlotMode {
    Absorbance,
    TransmittancePct,
}

pub struct SolutionApp {
    state: SceneState,
    flask: FlaskGeometry,
    presets: Vec<AbsorbanceSpectrum>,
    selected_preset_idx: Option<usize>,

    renderer: Option<SpectralRenderer>,
    texture_id: Option<egui::TextureId>,
    tex_size: (u32, u32),

    plot_mode: PlotMode,
    status_msg: Option<(String, bool)>, // (text, is_error)
    needs_scene_upload: bool,
    needs_accum_reset: bool,
    needs_display_only: bool,
    is_interacting: bool,
}

impl SolutionApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_spectrum: Option<AbsorbanceSpectrum>) -> Self {
        let presets = builtin_presets();
        let (spectrum, selected_preset_idx) = match initial_spectrum {
            Some(s) => (s, None),
            None => (presets[0].clone(), Some(0)),
        };

        let flask = FlaskGeometry::build(FlaskVolume::Ml100);
        let state = SceneState::new(spectrum, &flask);

        let mut renderer = None;
        let mut texture_id = None;
        let init_w = 960u32;
        let init_h = 768u32;

        if let Some(rs) = &cc.wgpu_render_state {
            let mut r = SpectralRenderer::new(&rs.device, &rs.queue, init_w, init_h);
            r.upload_scene(&rs.device, &rs.queue, &state, &flask);
            let tid = rs.renderer.write().register_native_texture(
                &rs.device,
                r.display_view(),
                wgpu::FilterMode::Linear,
            );
            texture_id = Some(tid);
            renderer = Some(r);
        }

        // Apply sleek dark scientific theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(20, 22, 27);
        visuals.window_fill = egui::Color32::from_rgb(24, 27, 34);
        cc.egui_ctx.set_visuals(visuals);

        Self {
            state,
            flask,
            presets,
            selected_preset_idx,
            renderer,
            texture_id,
            tex_size: (init_w, init_h),
            plot_mode: PlotMode::Absorbance,
            status_msg: None,
            needs_scene_upload: false,
            needs_accum_reset: false,
            needs_display_only: false,
            is_interacting: false,
        }
    }

    fn load_spectrum_path(&mut self, path: PathBuf) {
        match parse_spectrum_file(&path) {
            Ok(spec) => {
                self.status_msg = Some((
                    format!(
                        "Loaded '{}' ({} pts, {:.0}–{:.0} nm, peak {:.0} nm)",
                        spec.name,
                        spec.raw_points.len(),
                        spec.min_lambda,
                        spec.max_lambda,
                        spec.visible_peak_lambda
                    ),
                    false,
                ));
                self.state.spectrum = spec;
                self.selected_preset_idx = None;
                self.needs_scene_upload = true;
            }
            Err(err) => {
                self.status_msg = Some((err, true));
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.load_spectrum_path(path);
        }
    }

    fn render_viewport_panel(&mut self, ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(13, 15, 19)))
            .show(ctx, |ui| {
                let avail = ui.available_size();
                let ppp = ctx.pixels_per_point();
                let target_w = ((avail.x * ppp).round() as u32).clamp(256, 1600);
                let target_h = ((avail.y * ppp).round() as u32).clamp(256, 1200);

                if let (Some(rs), Some(renderer), Some(tex_id)) =
                    (render_state, self.renderer.as_mut(), self.texture_id)
                {
                    if (target_w, target_h) != self.tex_size {
                        renderer.resize(&rs.device, &rs.queue, target_w, target_h);
                        rs.renderer.write().update_egui_texture_from_wgpu_texture(
                            &rs.device,
                            renderer.display_view(),
                            wgpu::FilterMode::Linear,
                            tex_id,
                        );
                        self.tex_size = (target_w, target_h);
                        self.needs_accum_reset = true;
                    }

                    if self.needs_scene_upload {
                        renderer.upload_scene(&rs.device, &rs.queue, &self.state, &self.flask);
                        self.needs_scene_upload = false;
                        self.needs_accum_reset = false;
                        self.needs_display_only = false;
                    } else if self.needs_accum_reset {
                        renderer.reset_accumulation(&rs.device, &rs.queue);
                        self.needs_accum_reset = false;
                        self.needs_display_only = false;
                    }

                    let spp_now = renderer.total_spp();
                    if spp_now < self.state.max_spp {
                        let spp_step = if self.is_interacting {
                            2
                        } else if spp_now < 16 {
                            4
                        } else {
                            8
                        };
                        let rem = self.state.max_spp - spp_now;
                        renderer.render_step(
                            &rs.device,
                            &rs.queue,
                            &self.state,
                            &self.flask,
                            rem.min(spp_step),
                        );
                        self.needs_display_only = false;
                    } else if self.needs_display_only {
                        renderer.update_display_only(&rs.device, &rs.queue, &self.state, &self.flask);
                        self.needs_display_only = false;
                    }
                }

                // Draw ray-traced texture
                if let Some(tex_id) = self.texture_id {
                    let img_resp = ui.add(
                        egui::Image::new((tex_id, avail))
                            .sense(egui::Sense::click_and_drag()),
                    );

                    self.is_interacting = false;
                    if img_resp.dragged() {
                        self.is_interacting = true;
                        let delta = img_resp.drag_delta();
                        let shift = ui.input(|i| i.modifiers.shift);
                        let middle_or_right = ui.input(|i| i.pointer.secondary_down() || i.pointer.middle_down());
                        if shift || middle_or_right {
                            let (_, right, up, _) = self.state.camera.basis();
                            let scale = 0.0022 * self.state.camera.distance;
                            for k in 0..3 {
                                self.state.camera.target[k] +=
                                    (-delta.x * right[k] + delta.y * up[k]) * scale;
                            }
                            self.state.camera.target[1] = self.state.camera.target[1].clamp(0.0, 2.0 * self.flask.total_h);
                        } else {
                            self.state.camera.yaw -= delta.x * 0.0075;
                            self.state.camera.pitch = (self.state.camera.pitch + delta.y * 0.0075)
                                .clamp(-0.10, 1.48);
                        }
                        self.needs_accum_reset = true;
                    }

                    if img_resp.hovered() {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll.abs() > 0.1 {
                            self.is_interacting = true;
                            let factor = 0.996_f32.powf(scroll);
                            self.state.camera.distance =
                                (self.state.camera.distance * factor).clamp(2.2, 130.0);
                            self.needs_accum_reset = true;
                        }
                    }

                    if img_resp.double_clicked() {
                        self.state.camera = CameraState::preset_lab(&self.flask);
                        self.needs_accum_reset = true;
                    }

                    // Overlay HUD status pill & quick camera bar
                    let rect = img_resp.rect;
                    let spp = self.renderer.as_ref().map(|r| r.total_spp()).unwrap_or(0);
                    let hud_pos = rect.left_bottom() + egui::vec2(12.0, -34.0);
                    egui::Area::new(egui::Id::new("viewport_hud"))
                        .fixed_pos(hud_pos)
                        .interactable(false)
                        .show(ctx, |ui| {
                            egui::Frame::NONE
                                .fill(egui::Color32::from_rgba_unmultiplied(15, 17, 22, 200))
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(10, 5))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let status = if spp >= self.state.max_spp {
                                            "Converged"
                                        } else {
                                            "Tracing..."
                                        };
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{}  |  {}×{}  |  SPP {}/{} ({})  |  Drag: Orbit · Shift/Right-Drag: Pan · Scroll: Zoom",
                                                self.state.spectrum.name,
                                                self.tex_size.0,
                                                self.tex_size.1,
                                                spp,
                                                self.state.max_spp,
                                                status,
                                            ))
                                            .size(11.5)
                                            .color(egui::Color32::from_rgb(208, 214, 226)),
                                        );
                                    });
                                });
                        });
                }

                if let Some(r) = &self.renderer {
                    if r.total_spp() < self.state.max_spp || self.needs_accum_reset || self.needs_scene_upload {
                        ctx.request_repaint();
                    }
                }
            });
    }

    fn render_sidebar(&mut self, ctx: &egui::Context, render_state: Option<&eframe::egui_wgpu::RenderState>) {
        egui::SidePanel::right("controls_panel")
            .resizable(true)
            .default_width(370.0)
            .min_width(320.0)
            .max_width(480.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.heading("SolutionSimulator");
                    ui.label(
                        egui::RichText::new("Spectral Ray-Traced Volumetric Flask")
                            .size(11.5)
                            .color(egui::Color32::GRAY),
                    );
                    ui.separator();

                    // ---- 1. Spectrum Input ----
                    ui.collapsing(egui::RichText::new("1. Absorbance Spectrum (.txt)").strong(), |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Open .txt File...").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Spectrum TXT", &["txt", "csv", "dat", "tsv"])
                                    .pick_file()
                                {
                                    self.load_spectrum_path(path);
                                }
                            }
                            let current_label = self
                                .selected_preset_idx
                                .map(|i| self.presets[i].name.as_str())
                                .unwrap_or("Built-in Presets...");
                            egui::ComboBox::from_id_salt("preset_combo")
                                .selected_text(current_label)
                                .width(175.0)
                                .show_ui(ui, |ui| {
                                    for i in 0..self.presets.len() {
                                        let name = self.presets[i].name.clone();
                                        if ui
                                            .selectable_label(self.selected_preset_idx == Some(i), &name)
                                            .clicked()
                                        {
                                            self.state.spectrum = self.presets[i].clone();
                                            self.selected_preset_idx = Some(i);
                                            self.status_msg = None;
                                            self.needs_scene_upload = true;
                                        }
                                    }
                                });
                        });

                        if let Some((msg, is_err)) = &self.status_msg {
                            let color = if *is_err {
                                egui::Color32::from_rgb(240, 100, 100)
                            } else {
                                egui::Color32::from_rgb(130, 215, 160)
                            };
                            ui.label(egui::RichText::new(msg).size(11.0).color(color));
                        }
                        if let Some(warn) = &self.state.spectrum.warning {
                            ui.label(
                                egui::RichText::new(format!("Note: {warn}"))
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(235, 190, 95)),
                            );
                        }

                        ui.label(
                            egui::RichText::new(format!(
                                "Peak λ_max: {:.0} nm (A = {:.3})  |  NIR baseline: {:.4}  |  Drop .txt anywhere",
                                self.state.spectrum.visible_peak_lambda,
                                self.state.spectrum.visible_peak_abs,
                                self.state.spectrum.nir_baseline,
                            ))
                            .size(11.0)
                            .color(egui::Color32::LIGHT_GRAY),
                        );

                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.plot_mode, PlotMode::Absorbance, "Absorbance A(λ)");
                            ui.selectable_value(&mut self.plot_mode, PlotMode::TransmittancePct, "Transmittance %T");
                        });
                        self.draw_spectrum_plot(ui);

                        ui.add_space(4.0);
                        let mut spec_changed = false;
                        spec_changed |= ui
                            .add(
                                egui::Slider::new(&mut self.state.path_length_cm, 0.1..=10.0)
                                    .text("Cuvette path l₀ (cm)"),
                            )
                            .changed();

                        ui.horizontal(|ui| {
                            spec_changed |= ui
                                .add(
                                    egui::Slider::new(&mut self.state.conc_scale, 0.02..=50.0)
                                        .logarithmic(true)
                                        .text("Conc. factor"),
                                )
                                .changed();
                            if ui.small_button("1×").clicked() {
                                self.state.conc_scale = 1.0;
                                spec_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Baseline:");
                            if ui
                                .selectable_label(self.state.baseline_mode == BaselineMode::Raw, "Raw")
                                .clicked()
                            {
                                self.state.baseline_mode = BaselineMode::Raw;
                                spec_changed = true;
                            }
                            if ui
                                .selectable_label(
                                    self.state.baseline_mode == BaselineMode::AutoNirZero,
                                    "Auto NIR Zero",
                                )
                                .clicked()
                            {
                                self.state.baseline_mode = BaselineMode::AutoNirZero;
                                spec_changed = true;
                            }
                            let is_manual = matches!(self.state.baseline_mode, BaselineMode::Manual(_));
                            if ui.selectable_label(is_manual, "Manual").clicked() {
                                self.state.baseline_mode = BaselineMode::Manual(self.state.manual_baseline);
                                spec_changed = true;
                            }
                        });
                        if matches!(self.state.baseline_mode, BaselineMode::Manual(_)) {
                            spec_changed |= ui
                                .add(
                                    egui::Slider::new(&mut self.state.manual_baseline, -0.2..=1.0)
                                        .text("Baseline offset A₀"),
                                )
                                .changed();
                        }

                        if spec_changed {
                            self.needs_scene_upload = true;
                        }
                    });

                    // ---- 2. Colorimetry Readout ----
                    ui.collapsing(egui::RichText::new("2. Analytical Colorimetry").strong(), |ui| {
                        let sigma = self.state.sigma_a_liquid();
                        let cuvette = compute_transmitted_color(
                            &sigma,
                            self.state.path_length_cm,
                            self.state.illuminant,
                            self.state.white_balance,
                        );
                        let bulb_d = self.flask.bulb_inner_diameter_cm();
                        let bulb = compute_transmitted_color(
                            &sigma,
                            bulb_d,
                            self.state.illuminant,
                            self.state.white_balance,
                        );
                        ui.columns(2, |cols| {
                            draw_swatch_card(&mut cols[0], &format!("Cuvette ({:.1} cm)", self.state.path_length_cm), &cuvette);
                            draw_swatch_card(&mut cols[1], &format!("Flask Bulb ({:.1} cm)", bulb_d), &bulb);
                        });
                    });

                    // ---- 3. Volumetric Flask (ISO 1042) ----
                    ui.collapsing(egui::RichText::new("3. Volumetric Flask (ISO 1042)").strong(), |ui| {
                        let mut geom_changed = false;
                        let mut uniform_changed = false;

                        ui.horizontal_wrapped(|ui| {
                            for vol in FlaskVolume::ALL {
                                if ui.selectable_label(self.state.volume == vol, vol.label()).clicked() {
                                    self.state.volume = vol;
                                    self.flask = FlaskGeometry::build(vol);
                                    self.state.card_distance_cm = (3.6 * self.flask.r_bulb_max).max(11.0);
                                    self.state.camera = CameraState::preset_lab(&self.flask);
                                    geom_changed = true;
                                }
                            }
                        });
                        ui.label(
                            egui::RichText::new(format!(
                                "Calibrated V(y_mark) = {:.2} mL  |  Bulb ID = {:.2} cm  |  Neck ID = {:.2} cm",
                                self.flask.calibrated_volume_ml,
                                self.flask.bulb_inner_diameter_cm(),
                                self.flask.neck_inner_diameter_cm(),
                            ))
                            .size(11.0)
                            .color(egui::Color32::GRAY),
                        );

                        uniform_changed |= ui
                            .add(
                                egui::Slider::new(&mut self.state.fill_fraction, 0.0..=1.08)
                                    .text("Fill fraction"),
                            )
                            .changed();
                        ui.horizontal(|ui| {
                            if ui.small_button("At Calibration Mark (1.00)").clicked() {
                                self.state.fill_fraction = 1.0;
                                uniform_changed = true;
                            }
                            if ui.small_button("Half Bulb (0.50)").clicked() {
                                self.state.fill_fraction = 0.50;
                                uniform_changed = true;
                            }
                            if ui.small_button("Empty").clicked() {
                                self.state.fill_fraction = 0.0;
                                uniform_changed = true;
                            }
                        });

                        egui::ComboBox::from_label("Stopper")
                            .selected_text(self.state.stopper_type.label())
                            .show_ui(ui, |ui| {
                                for st in StopperType::ALL {
                                    if ui
                                        .selectable_value(&mut self.state.stopper_type, st, st.label())
                                        .changed()
                                    {
                                        uniform_changed = true;
                                    }
                                }
                            });

                        uniform_changed |= ui
                            .checkbox(&mut self.state.enable_markings, "Etched calibration ring & markings")
                            .changed();
                        geom_changed |= ui
                            .add(
                                egui::Slider::new(&mut self.state.glass_tint_strength, 0.0..=2.5)
                                    .text("Borosilicate rim tint"),
                            )
                            .changed();

                        if geom_changed {
                            self.needs_scene_upload = true;
                        } else if uniform_changed {
                            self.needs_accum_reset = true;
                        }
                    });

                    // ---- 4. Optics, Illuminant & Studio ----
                    ui.collapsing(egui::RichText::new("4. Optics, Illuminant & Studio").strong(), |ui| {
                        let mut upload_needed = false;
                        let mut reset_needed = false;
                        let mut display_needed = false;

                        egui::ComboBox::from_label("Solvent")
                            .selected_text(self.state.solvent.label())
                            .show_ui(ui, |ui| {
                                for sol in SolventPreset::ALL {
                                    if ui
                                        .selectable_value(&mut self.state.solvent, sol, sol.label())
                                        .changed()
                                    {
                                        reset_needed = true;
                                    }
                                }
                            });
                        if self.state.solvent == SolventPreset::Custom {
                            reset_needed |= ui
                                .add(egui::Slider::new(&mut self.state.custom_nd, 1.25..=1.70).text("Custom n_D"))
                                .changed();
                        }

                        egui::ComboBox::from_label("Illuminant")
                            .selected_text(self.state.illuminant.label())
                            .show_ui(ui, |ui| {
                                for il in Illuminant::ALL {
                                    if ui
                                        .selectable_value(&mut self.state.illuminant, il, il.label())
                                        .changed()
                                    {
                                        upload_needed = true;
                                    }
                                }
                            });

                        egui::ComboBox::from_label("Lighting Studio")
                            .selected_text(self.state.lighting_preset.label())
                            .show_ui(ui, |ui| {
                                for lp in LightingPreset::ALL {
                                    if ui
                                        .selectable_value(&mut self.state.lighting_preset, lp, lp.label())
                                        .changed()
                                    {
                                        reset_needed = true;
                                    }
                                }
                            });

                        reset_needed |= ui
                            .add(
                                egui::Slider::new(&mut self.state.light_azimuth_deg, -180.0..=180.0)
                                    .suffix("°")
                                    .text("Key light azimuth"),
                            )
                            .changed();

                        reset_needed |= ui
                            .checkbox(&mut self.state.enable_white_card, "White contrast card behind flask")
                            .changed();
                        if self.state.enable_white_card {
                            reset_needed |= ui
                                .add(
                                    egui::Slider::new(&mut self.state.card_distance_cm, 5.0..=30.0)
                                        .text("Card distance (cm)"),
                                )
                                .changed();
                        }
                        reset_needed |= ui
                            .add(egui::Slider::new(&mut self.state.bench_albedo, 0.15..=0.88).text("Bench albedo"))
                            .changed();

                        reset_needed |= ui
                            .checkbox(&mut self.state.enable_caustics, "Spectral bench caustics")
                            .changed();
                        reset_needed |= ui
                            .checkbox(&mut self.state.enable_dispersion, "Chromatic dispersion (prism edges)")
                            .on_hover_text("OFF: exact 1 nm full-spectrum Beer-Lambert integration (noise-free).\nON: Cauchy wavelength-dependent glass/solvent dispersion.")
                            .changed();

                        display_needed |= ui
                            .checkbox(&mut self.state.white_balance, "Bradford white balance (D65)")
                            .changed();
                        egui::ComboBox::from_label("Tone mapper")
                            .selected_text(self.state.tone_mapper.label())
                            .show_ui(ui, |ui| {
                                for tm in ToneMapper::ALL {
                                    if ui
                                        .selectable_value(&mut self.state.tone_mapper, tm, tm.label())
                                        .changed()
                                    {
                                        display_needed = true;
                                    }
                                }
                            });
                        display_needed |= ui
                            .add(
                                egui::Slider::new(&mut self.state.exposure, 0.25..=3.5)
                                    .logarithmic(true)
                                    .text("Exposure"),
                            )
                            .changed();
                        ui.add(egui::Slider::new(&mut self.state.max_spp, 32..=1024).text("Target SPP"));

                        if upload_needed {
                            self.needs_scene_upload = true;
                        } else if reset_needed {
                            self.needs_accum_reset = true;
                        } else if display_needed {
                            self.needs_display_only = true;
                        }
                    });

                    // ---- 5. Camera & Export ----
                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(egui::RichText::new("Camera Views & PNG Export").strong());
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("3/4 Lab").clicked() {
                            self.state.camera = CameraState::preset_lab(&self.flask);
                            self.needs_accum_reset = true;
                        }
                        if ui.button("Eye-Level").clicked() {
                            self.state.camera = CameraState::preset_eye_level(&self.flask);
                            self.needs_accum_reset = true;
                        }
                        if ui.button("Meniscus").clicked() {
                            self.state.camera = CameraState::preset_meniscus(&self.flask);
                            self.needs_accum_reset = true;
                        }
                        if ui.button("Top Caustics").clicked() {
                            self.state.camera = CameraState::preset_top(&self.flask);
                            self.needs_accum_reset = true;
                        }
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save PNG...").clicked() {
                            if let (Some(rs), Some(r)) = (render_state, self.renderer.as_ref()) {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("PNG Image", &["png"])
                                    .set_file_name("flask_render.png")
                                    .save_file()
                                {
                                    let (w, h) = r.size();
                                    match r
                                        .readback_rgba(&rs.device, &rs.queue)
                                        .and_then(|rgba| save_rgba_png(&path, w, h, rgba))
                                    {
                                        Ok(()) => {
                                            self.status_msg = Some((
                                                format!("Saved PNG ({w}×{h}) to '{}'", path.display()),
                                                false,
                                            ));
                                        }
                                        Err(e) => {
                                            self.status_msg = Some((e, true));
                                        }
                                    }
                                }
                            }
                        }
                        if ui.button("Reset Accumulation").clicked() {
                            self.needs_accum_reset = true;
                        }
                    });
                });
            });
    }

    fn draw_spectrum_plot(&self, ui: &mut egui::Ui) {
        let desired = egui::vec2(ui.available_width(), 155.0);
        let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::hover());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(14, 16, 21));
        let rainbow_h = 10.0;
        let pad_l = 34.0;
        let pad_r = 10.0;
        let pad_t = 10.0;
        let pad_b = 26.0;
        let plot_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + pad_l, rect.top() + pad_t),
            egui::pos2(rect.right() - pad_r, rect.bottom() - pad_b),
        );
        if plot_rect.width() <= 10.0 || plot_rect.height() <= 10.0 {
            return;
        }

        // Draw subtle grid lines
        let grid_col = egui::Color32::from_rgba_unmultiplied(120, 130, 150, 40);
        for k in 0..=4 {
            let y = plot_rect.top() + (k as f32 / 4.0) * plot_rect.height();
            painter.line_segment(
                [egui::pos2(plot_rect.left(), y), egui::pos2(plot_rect.right(), y)],
                egui::Stroke::new(1.0, grid_col),
            );
        }
        for wl in [400.0_f32, 500.0, 600.0, 700.0] {
            let t = (wl - LAMBDA_MIN) / (LAMBDA_MAX - LAMBDA_MIN);
            let x = plot_rect.left() + t * plot_rect.width();
            painter.line_segment(
                [egui::pos2(x, plot_rect.top()), egui::pos2(x, plot_rect.bottom())],
                egui::Stroke::new(1.0, grid_col),
            );
        }

        // Draw rainbow spectrum bar beneath X axis
        let strip_top = plot_rect.bottom() + 2.0;
        let strip_bot = strip_top + rainbow_h;
        let bins = 80usize;
        for b in 0..bins {
            let t0 = b as f32 / bins as f32;
            let t1 = (b + 1) as f32 / bins as f32;
            let wl = LAMBDA_MIN + 0.5 * (t0 + t1) * (LAMBDA_MAX - LAMBDA_MIN);
            let [cr, cg, cb] = wavelength_to_srgb(wl);
            let r = egui::Rect::from_min_max(
                egui::pos2(plot_rect.left() + t0 * plot_rect.width(), strip_top),
                egui::pos2(plot_rect.left() + t1 * plot_rect.width() + 0.5, strip_bot),
            );
            painter.rect_filled(r, 0.0, egui::Color32::from_rgb(cr, cg, cb));
        }

        // Wavelength axis ticks
        for wl in [400, 500, 600, 700, 780] {
            let t = (wl as f32 - LAMBDA_MIN) / (LAMBDA_MAX - LAMBDA_MIN);
            let x = plot_rect.left() + t * plot_rect.width();
            painter.text(
                egui::pos2(x, strip_bot + 2.0),
                egui::Align2::CENTER_TOP,
                format!("{wl}"),
                egui::FontId::proportional(9.5),
                egui::Color32::from_rgb(165, 175, 190),
            );
        }

        let offset = self
            .state
            .effective_baseline_mode()
            .offset(self.state.spectrum.nir_baseline);
        let sigma = self.state.sigma_a_liquid();

        match self.plot_mode {
            PlotMode::Absorbance => {
                let raw_max = self
                    .state
                    .spectrum
                    .absorbance_1nm
                    .iter()
                    .copied()
                    .fold(0.1_f32, f32::max);
                let y_max = (raw_max * 1.12).max(0.25);

                // Y-axis labels
                painter.text(
                    egui::pos2(plot_rect.left() - 4.0, plot_rect.top()),
                    egui::Align2::RIGHT_TOP,
                    format!("{y_max:.2}"),
                    egui::FontId::proportional(9.5),
                    egui::Color32::GRAY,
                );
                painter.text(
                    egui::pos2(plot_rect.left() - 4.0, plot_rect.bottom()),
                    egui::Align2::RIGHT_BOTTOM,
                    "0.0",
                    egui::FontId::proportional(9.5),
                    egui::Color32::GRAY,
                );

                // Baseline dashed indicator if non-zero
                if offset.abs() > 1e-4 {
                    let y_off = plot_rect.bottom() - (offset / y_max).clamp(0.0, 1.0) * plot_rect.height();
                    painter.line_segment(
                        [egui::pos2(plot_rect.left(), y_off), egui::pos2(plot_rect.right(), y_off)],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 180, 70)),
                    );
                }

                // Plot A(lambda)
                let pts: Vec<egui::Pos2> = (0..SPECTRUM_LEN)
                    .step_by(2)
                    .map(|i| {
                        let t = i as f32 / (SPECTRUM_LEN - 1) as f32;
                        let a = self.state.spectrum.absorbance_1nm[i];
                        let yn = (a / y_max).clamp(0.0, 1.0);
                        egui::pos2(
                            plot_rect.left() + t * plot_rect.width(),
                            plot_rect.bottom() - yn * plot_rect.height(),
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(1.8, egui::Color32::from_rgb(90, 195, 255)),
                ));

                // Peak lambda marker
                let pt = (self.state.spectrum.visible_peak_lambda - LAMBDA_MIN) / (LAMBDA_MAX - LAMBDA_MIN);
                let px = plot_rect.left() + pt.clamp(0.0, 1.0) * plot_rect.width();
                painter.line_segment(
                    [egui::pos2(px, plot_rect.top()), egui::pos2(px, plot_rect.bottom())],
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 130, 130, 130)),
                );
            }
            PlotMode::TransmittancePct => {
                painter.text(
                    egui::pos2(plot_rect.left() - 4.0, plot_rect.top()),
                    egui::Align2::RIGHT_TOP,
                    "100%",
                    egui::FontId::proportional(9.5),
                    egui::Color32::GRAY,
                );
                painter.text(
                    egui::pos2(plot_rect.left() - 4.0, plot_rect.bottom()),
                    egui::Align2::RIGHT_BOTTOM,
                    "0%",
                    egui::FontId::proportional(9.5),
                    egui::Color32::GRAY,
                );

                let bulb_d = self.flask.bulb_inner_diameter_cm();
                let cuv_pts: Vec<egui::Pos2> = (0..SPECTRUM_LEN)
                    .step_by(2)
                    .map(|i| {
                        let t = i as f32 / (SPECTRUM_LEN - 1) as f32;
                        let tr = (-sigma[i] * self.state.path_length_cm).exp().clamp(0.0, 1.0);
                        egui::pos2(
                            plot_rect.left() + t * plot_rect.width(),
                            plot_rect.bottom() - tr * plot_rect.height(),
                        )
                    })
                    .collect();
                let bulb_pts: Vec<egui::Pos2> = (0..SPECTRUM_LEN)
                    .step_by(2)
                    .map(|i| {
                        let t = i as f32 / (SPECTRUM_LEN - 1) as f32;
                        let tr = (-sigma[i] * bulb_d).exp().clamp(0.0, 1.0);
                        egui::pos2(
                            plot_rect.left() + t * plot_rect.width(),
                            plot_rect.bottom() - tr * plot_rect.height(),
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(
                    cuv_pts,
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(130, 210, 255)),
                ));
                painter.add(egui::Shape::line(
                    bulb_pts,
                    egui::Stroke::new(1.8, egui::Color32::from_rgb(255, 175, 95)),
                ));
            }
        }

        // Hover readout
        if let Some(pos) = response.hover_pos() {
            if plot_rect.contains(pos) {
                let u = ((pos.x - plot_rect.left()) / plot_rect.width()).clamp(0.0, 1.0);
                let wl = LAMBDA_MIN + u * (LAMBDA_MAX - LAMBDA_MIN);
                let idx = (wl.round() as i32 - 380).clamp(0, 400) as usize;
                let a = self.state.spectrum.absorbance_1nm[idx];
                let t_cuv = 100.0 * (-sigma[idx] * self.state.path_length_cm).exp();
                let t_bulb = 100.0 * (-sigma[idx] * self.flask.bulb_inner_diameter_cm()).exp();
                painter.line_segment(
                    [egui::pos2(pos.x, plot_rect.top()), egui::pos2(pos.x, plot_rect.bottom())],
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
                );
                response.on_hover_text(format!(
                    "λ = {wl:.0} nm\nA_raw = {a:.4}\nT(cuvette) = {t_cuv:.1}%\nT(bulb) = {t_bulb:.1}%"
                ));
            }
        }
    }
}

fn draw_swatch_card(ui: &mut egui::Ui, title: &str, c: &ColorReadout) {
    ui.group(|ui| {
        ui.label(egui::RichText::new(title).size(11.5).strong());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::hover());
        ui.painter().rect_filled(
            rect,
            4.0,
            egui::Color32::from_rgb(c.srgb_u8[0], c.srgb_u8[1], c.srgb_u8[2]),
        );
        ui.label(
            egui::RichText::new(format!(
                "{}  |  %T: {:.1}%\nxy: ({:.3}, {:.3})\nL*a*b*: ({:.0}, {:.0}, {:.0})",
                c.hex, c.mean_transmittance_pct, c.xy[0], c.xy[1], c.lab[0], c.lab[1], c.lab[2],
            ))
            .size(10.5)
            .color(egui::Color32::LIGHT_GRAY),
        );
    });
}

impl eframe::App for SolutionApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.handle_dropped_files(ctx);
        let render_state = frame.wgpu_render_state();
        self.render_sidebar(ctx, render_state);
        self.render_viewport_panel(ctx, render_state);
    }
}
