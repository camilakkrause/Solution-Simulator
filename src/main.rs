mod colorimetry;
mod flask;
mod gpu;
mod spectrum;
mod ui;

use std::path::{Path, PathBuf};

use eframe::egui;

use flask::{FlaskGeometry, FlaskVolume, StopperType};
use gpu::{CameraState, LightingPreset, SceneState, SpectralRenderer};
use spectrum::{parse_spectrum_file, preset_by_name_or_slug, AbsorbanceSpectrum, BaselineMode};

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    if let Some(render_idx) = args.iter().position(|a| a == "--render") {
        if let Err(err) = run_headless_cli(&args, render_idx) {
            eprintln!("Error: {err}");
            std::process::exit(1);
        }
        return;
    }

    let initial_spectrum = args.first().and_then(|arg| {
        let p = Path::new(arg);
        if p.exists() {
            match parse_spectrum_file(p) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("Warning: could not load '{arg}': {e}");
                    None
                }
            }
        } else {
            preset_by_name_or_slug(arg)
        }
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SolutionSimulator — Spectral Ray-Traced Volumetric Flask")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([920.0, 620.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "SolutionSimulator",
        options,
        Box::new(move |cc| Ok(Box::new(ui::SolutionApp::new(cc, initial_spectrum)))),
    ) {
        eprintln!("Failed to launch GUI: {e}");
        std::process::exit(1);
    }
}

fn print_help() {
    println!(
        "\
SolutionSimulator — Physically-Based GPU Spectral Ray Tracer

USAGE:
  solution_simulator [spectrum.txt | preset_name]
  solution_simulator --render <spectrum.txt | preset> --out <output.png> [OPTIONS]

HEADLESS OPTIONS:
  --out <path>          Output PNG file path (default: output.png)
  --width <px>          Render width in pixels (default: 1024)
  --height <px>         Render height in pixels (default: 1024)
  --spp <n>             Samples per pixel (default: 256)
  --volume <ml>         Flask volume: 10, 25, 50, 100, 250, 500, 1000 (default: 100)
  --fill <frac>         Fill fraction relative to mark (default: 1.0)
  --conc <scale>        Concentration multiplier (default: 1.0)
  --path-cm <cm>        Cuvette reference path length in cm (default: 1.0)
  --baseline <mode>     raw | nir (default: raw)
  --preset <name>       studio | backlit | overhead | window (default: studio)
  --camera <view>       lab | eye | meniscus | top (default: lab)
  --caustics <on|off>   Enable bench spectral caustics (default: on)
  --dispersion <on|off> Enable Cauchy chromatic dispersion (default: off)
  --stopper <type>      none | glass | ptfe (default: none)
  --white-card <on|off> Enable white contrast card behind flask (default: off)
  --exposure <val>      Exposure multiplier (default: 1.0)
"
    );
}

fn load_spectrum_arg(arg: &str) -> Result<AbsorbanceSpectrum, String> {
    let p = Path::new(arg);
    if p.exists() {
        parse_spectrum_file(p)
    } else {
        preset_by_name_or_slug(arg).ok_or_else(|| {
            format!("'{arg}' is neither an existing file nor a known preset (thiaz, kmno4, bromophenol, cuso4, niso4, water).")
        })
    }
}

fn run_headless_cli(args: &[String], render_idx: usize) -> Result<(), String> {
    let spec_arg = args
        .get(render_idx + 1)
        .ok_or_else(|| "Missing spectrum path or preset name after --render.".to_string())?;
    let spectrum = load_spectrum_arg(spec_arg)?;

    let mut out_path = PathBuf::from("output.png");
    let mut width = 1024u32;
    let mut height = 1024u32;
    let mut spp = 256u32;
    let mut volume = FlaskVolume::Ml100;
    let mut fill_fraction = 1.0f32;
    let mut conc_scale = 1.0f32;
    let mut path_cm = 1.0f32;
    let mut baseline = BaselineMode::Raw;
    let mut lighting = LightingPreset::LabStudio;
    let mut camera_mode = "lab".to_string();
    let mut caustics = true;
    let mut dispersion = false;
    let mut stopper = StopperType::None;
    let mut white_card = false;
    let mut exposure = 1.0f32;

    let mut i = 0;
    while i < args.len() {
        if i == render_idx {
            i += 2;
            continue;
        }
        match args[i].as_str() {
            "--out" => {
                out_path = PathBuf::from(args.get(i + 1).ok_or("Missing value for --out")?);
                i += 2;
            }
            "--width" => {
                width = args.get(i + 1).ok_or("Missing --width")?.parse().map_err(|_| "Invalid --width")?;
                i += 2;
            }
            "--height" => {
                height = args.get(i + 1).ok_or("Missing --height")?.parse().map_err(|_| "Invalid --height")?;
                i += 2;
            }
            "--spp" => {
                spp = args.get(i + 1).ok_or("Missing --spp")?.parse().map_err(|_| "Invalid --spp")?;
                i += 2;
            }
            "--volume" => {
                let ml: u32 = args.get(i + 1).ok_or("Missing --volume")?.parse().map_err(|_| "Invalid --volume")?;
                volume = FlaskVolume::from_ml_int(ml)
                    .ok_or_else(|| format!("Unsupported flask volume {ml} mL. Use 10, 25, 50, 100, 250, 500, or 1000."))?;
                i += 2;
            }
            "--fill" => {
                fill_fraction = args.get(i + 1).ok_or("Missing --fill")?.parse().map_err(|_| "Invalid --fill")?;
                i += 2;
            }
            "--conc" => {
                conc_scale = args.get(i + 1).ok_or("Missing --conc")?.parse().map_err(|_| "Invalid --conc")?;
                i += 2;
            }
            "--path-cm" => {
                path_cm = args.get(i + 1).ok_or("Missing --path-cm")?.parse().map_err(|_| "Invalid --path-cm")?;
                i += 2;
            }
            "--baseline" => {
                let val = args.get(i + 1).ok_or("Missing --baseline")?.to_ascii_lowercase();
                baseline = match val.as_str() {
                    "raw" => BaselineMode::Raw,
                    "nir" => BaselineMode::AutoNirZero,
                    _ => return Err(format!("Unknown --baseline '{val}'; expected raw or nir.")),
                };
                i += 2;
            }
            "--preset" => {
                let val = args.get(i + 1).ok_or("Missing --preset")?.to_ascii_lowercase();
                lighting = match val.as_str() {
                    "studio" => LightingPreset::LabStudio,
                    "backlit" => LightingPreset::BacklitLightbox,
                    "overhead" => LightingPreset::OverheadLamp,
                    "window" => LightingPreset::WindowDaylight,
                    _ => return Err(format!("Unknown --preset '{val}'.")),
                };
                i += 2;
            }
            "--camera" => {
                camera_mode = args.get(i + 1).ok_or("Missing --camera")?.to_ascii_lowercase();
                i += 2;
            }
            "--caustics" => {
                let val = args.get(i + 1).ok_or("Missing --caustics")?.to_ascii_lowercase();
                caustics = matches!(val.as_str(), "on" | "true" | "1" | "yes");
                i += 2;
            }
            "--dispersion" => {
                let val = args.get(i + 1).ok_or("Missing --dispersion")?.to_ascii_lowercase();
                dispersion = matches!(val.as_str(), "on" | "true" | "1" | "yes");
                i += 2;
            }
            "--stopper" => {
                let val = args.get(i + 1).ok_or("Missing --stopper")?.to_ascii_lowercase();
                stopper = match val.as_str() {
                    "none" => StopperType::None,
                    "glass" => StopperType::Glass,
                    "ptfe" => StopperType::Ptfe,
                    _ => return Err(format!("Unknown --stopper '{val}'.")),
                };
                i += 2;
            }
            "--white-card" => {
                let val = args.get(i + 1).ok_or("Missing --white-card")?.to_ascii_lowercase();
                white_card = matches!(val.as_str(), "on" | "true" | "1" | "yes");
                i += 2;
            }
            "--exposure" => {
                exposure = args.get(i + 1).ok_or("Missing --exposure")?.parse().map_err(|_| "Invalid --exposure")?;
                i += 2;
            }
            other => return Err(format!("Unknown argument '{other}'. Run with --help.")),
        }
    }

    let flask = FlaskGeometry::build(volume);
    let mut state = SceneState::new(spectrum, &flask);
    state.fill_fraction = fill_fraction;
    state.conc_scale = conc_scale;
    state.path_length_cm = path_cm;
    state.baseline_mode = baseline;
    state.lighting_preset = lighting;
    state.enable_caustics = caustics;
    state.enable_dispersion = dispersion;
    state.stopper_type = stopper;
    state.enable_white_card = white_card;
    state.exposure = exposure;
    state.camera = match camera_mode.as_str() {
        "lab" => CameraState::preset_lab(&flask),
        "eye" => CameraState::preset_eye_level(&flask),
        "meniscus" => CameraState::preset_meniscus(&flask),
        "top" => CameraState::preset_top(&flask),
        other => return Err(format!("Unknown --camera '{other}'; expected lab, eye, meniscus, or top.")),
    };

    println!(
        "Rendering '{}' ({}, {}x{}, {} SPP, caustics={}) -> {}",
        state.spectrum.name,
        volume.label(),
        width,
        height,
        spp,
        caustics,
        out_path.display()
    );
    SpectralRenderer::render_headless_png(&state, &flask, width, height, spp, &out_path)?;
    println!("Saved {}", out_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colorimetry::{
        compute_transmitted_color, illuminant_white_xyz, xyz_to_srgb_u8, Illuminant, ToneMapper,
        D65_WHITE_XYZ,
    };
    use crate::flask::{eval_profile_radius, integrate_volume_to, ProfileSegment};
    use crate::gpu::uniform::{SceneUniform, SpectralEntry};
    use crate::spectrum::parse_spectrum_str;

    #[test]
    fn parser_skips_first_two_lines_and_handles_formats() {
        let txt = "\
\"Header line 1\"
\"Wavelength nm.\" \"Abs.\"
780,0; 0,050
700,0; 0,050
484,0; 0,616
380,0; 0,090
";
        let spec = parse_spectrum_str("test.txt", txt).expect("valid parse");
        assert_eq!(spec.raw_points.len(), 4);
        assert!((spec.min_lambda - 380.0).abs() < 1e-3);
        assert!((spec.max_lambda - 780.0).abs() < 1e-3);
        assert!((spec.visible_peak_lambda - 484.0).abs() < 1.0);
        assert!((spec.visible_peak_abs - 0.616).abs() < 1e-3);
        assert!((spec.nir_baseline - 0.050).abs() < 1e-3);
    }

    #[test]
    fn parser_rejects_short_file() {
        let txt = "Header 1\nHeader 2\n400.0 0.2\n";
        assert!(parse_spectrum_str("short.txt", txt).is_err());
    }

    #[test]
    fn parse_real_sample_c2_5_if_available() {
        let sample = Path::new(r"c:\Users\pedro\PycharmProjects\IC\moleculas\Thiaz1\epsilon\trial1\c2_5.txt");
        if !sample.exists() {
            return;
        }
        let spec = parse_spectrum_file(sample).expect("parse c2_5.txt");
        assert!((spec.min_lambda - 190.0).abs() < 1e-2);
        assert!((spec.max_lambda - 1100.0).abs() < 1e-2);
        assert!((spec.visible_peak_lambda - 484.0).abs() <= 2.0);
        assert!((spec.visible_peak_abs - 0.6162).abs() < 0.01);
        assert!((spec.nir_baseline - 0.047).abs() < 0.005);
        assert!(spec.warning.is_none());

        let sigma_raw = spec.effective_sigma_a(1.0, 1.0, BaselineMode::Raw);
        let sigma_nir = spec.effective_sigma_a(1.0, 1.0, BaselineMode::AutoNirZero);
        let c1 = compute_transmitted_color(&sigma_raw, 1.0, Illuminant::D65, true);
        let c6 = compute_transmitted_color(&sigma_raw, 6.0, Illuminant::D65, true);
        // Thiaz peak at 484 nm absorbs blue-cyan -> golden-yellow at 1 cm, deeper orange-red at 6 cm!
        assert!(c1.srgb_u8[0] > c1.srgb_u8[2]);
        assert!(c6.lab[0] < c1.lab[0]);
        assert!(sigma_nir[400] < 0.01);
    }

    #[test]
    fn colorimetry_white_point_and_dichromatism() {
        let d65_w = illuminant_white_xyz(Illuminant::D65);
        assert!((d65_w[0] - D65_WHITE_XYZ[0]).abs() < 0.002);
        assert!((d65_w[1] - 1.0).abs() < 1e-5);
        assert!((d65_w[2] - D65_WHITE_XYZ[2]).abs() < 0.002);

        let blank = preset_by_name_or_slug("water").unwrap();
        let sigma_zero = blank.effective_sigma_a(1.0, 1.0, BaselineMode::Raw);
        let clear = compute_transmitted_color(&sigma_zero, 6.0, Illuminant::D65, true);
        assert_eq!(clear.srgb_u8, [255, 255, 255]);
        assert!((clear.lab[0] - 100.0).abs() < 0.2);

        // Bromophenol Blue preset shifts hue from blue/cyan at 1 cm to ruby-red at 6 cm
        let bpb = preset_by_name_or_slug("bromophenol").unwrap();
        let sigma_bpb = bpb.effective_sigma_a(1.0, 1.0, BaselineMode::Raw);
        let thin = compute_transmitted_color(&sigma_bpb, 1.0, Illuminant::D65, true);
        let thick = compute_transmitted_color(&sigma_bpb, 6.0, Illuminant::D65, true);
        assert!(thin.srgb_u8[2] > thin.srgb_u8[0], "1 cm BPB should be blue-dominant: {:?}", thin.srgb_u8);
        assert!(thick.srgb_u8[0] > thick.srgb_u8[2], "6 cm BPB should shift red-dominant: {:?}", thick.srgb_u8);

        let mapped_white = xyz_to_srgb_u8(D65_WHITE_XYZ, ToneMapper::PbrNeutral);
        assert!(mapped_white[0] >= 235 && mapped_white[1] >= 235 && mapped_white[2] >= 235);
        assert!((mapped_white[0] as i32 - mapped_white[1] as i32).abs() <= 1);
        let linear_white = xyz_to_srgb_u8(D65_WHITE_XYZ, ToneMapper::Linear);
        assert_eq!(linear_white, [255, 255, 255]);
    }

    #[test]
    fn all_iso_1042_flasks_are_valid_and_calibrated() {
        for vol in FlaskVolume::ALL {
            let flask = FlaskGeometry::build(vol);
            let rel_err = (flask.calibrated_volume_ml - vol.ml()).abs() / vol.ml();
            assert!(
                rel_err < 0.002,
                "{:?}: calibrated {:.3} vs nominal {:.3}",
                vol,
                flask.calibrated_volume_ml,
                vol.ml()
            );
            assert!(
                flask.y_mark > flask.y_neck_start + 0.3 && flask.y_mark < flask.total_h - 0.8,
                "{:?}: y_mark={:.2} outside neck [{:.2}, {:.2}]",
                vol,
                flask.y_mark,
                flask.y_neck_start,
                flask.total_h
            );

            // Verify strictly positive glass wall thickness everywhere
            for step in 0..=100 {
                let y = flask.base_t + (step as f32 / 100.0) * (flask.total_h - flask.base_t);
                let r_out = eval_profile_radius(&flask.outer_segs, y);
                let r_in = eval_profile_radius(&flask.inner_segs, y);
                assert!(
                    r_out - r_in >= 0.45 * flask.wall_t,
                    "{:?} at y={:.2}: wall thickness {:.4} too thin!",
                    vol,
                    y,
                    r_out - r_in
                );
            }

            // Verify volume calibration check matches direct integration
            let v_at_mark = integrate_volume_to(&flask.inner_segs, flask.y_mark);
            assert!((v_at_mark - flask.calibrated_volume_ml).abs() < 1e-3);

            // Verify meniscus smoothness & overflow-free bounds across fill range
            for &fill in &[0.0_f32, 0.25, 0.5, 1.0, 1.05] {
                let m = flask.solve_meniscus(fill, 0.27);
                if fill <= 0.001 {
                    assert!(!m.fill_active);
                } else {
                    assert!(m.fill_active);
                    assert!(m.y_bot >= flask.base_t - 1e-4);
                    assert!(m.y_contact < flask.total_h);
                    assert!(m.h_men >= 0.0 && m.h_men <= 0.25);
                    if (fill - 1.0).abs() < 1e-5 {
                        assert!((m.y_bot - flask.y_mark).abs() < 0.02);
                    }
                }
            }
        }
    }

    #[test]
    fn gpu_struct_sizes_match_wgsl_alignment() {
        assert_eq!(std::mem::size_of::<SceneUniform>(), 304);
        assert_eq!(std::mem::align_of::<SceneUniform>(), 4);
        assert_eq!(std::mem::size_of::<SpectralEntry>(), 32);
        assert_eq!(std::mem::size_of::<ProfileSegment>(), 32);
    }

    #[test]
    fn beer_lut_and_wl_importance_match_exact_colorimetry() {
        use crate::colorimetry::{
            build_beer_lut, build_wl_importance_tables, BEER_LUT_LIQ_LEN,
        };
        use crate::spectrum::SPECTRUM_LEN;

        // 1. Check CMF importance PDF trapezoidal integral == 1.0 and icdf monotonicity
        let (icdf, pdf) = build_wl_importance_tables(Illuminant::D65);
        let mut trap = 0.5 * (pdf[0] as f64 + pdf[SPECTRUM_LEN - 1] as f64);
        for &p in &pdf[1..(SPECTRUM_LEN - 1)] {
            trap += p as f64;
        }
        assert!((trap - 1.0).abs() < 1e-4, "PDF integral = {trap}");
        assert!((icdf[0] - 380.0).abs() < 1e-3);
        assert!((icdf[SPECTRUM_LEN - 1] - 780.0).abs() < 1e-3);
        for w in icdf.windows(2) {
            assert!(w[1] >= w[0]);
        }

        // 2. Check Beer LUT at (0, 0) equals D65 white and interpolates d=1cm & d=6cm accurately
        let bpb = preset_by_name_or_slug("bromophenol").unwrap();
        let sigma_liq = bpb.effective_sigma_a(1.0, 1.0, BaselineMode::Raw);
        let sigma_glass = [0.0f32; SPECTRUM_LEN];
        let d_liq_max = 60.0_f32;
        let lut = build_beer_lut(&sigma_liq, &sigma_glass, 1.0, Illuminant::D65, d_liq_max, 12.0);
        let white = illuminant_white_xyz(Illuminant::D65);
        for c in 0..3 {
            assert!((lut[0][c] - white[c]).abs() < 1e-4);
        }

        // Bilinear lookup helper matching WGSL eval_beer_lut for d_glass = 0
        let sample_lut = |d_liq: f32| -> [f32; 3] {
            let uc = (d_liq / d_liq_max).clamp(0.0, 1.0).sqrt() * (BEER_LUT_LIQ_LEN - 1) as f32;
            let i0 = (uc.floor() as usize).min(BEER_LUT_LIQ_LEN - 2);
            let fx = uc - i0 as f32;
            [
                lut[i0][0] * (1.0 - fx) + lut[i0 + 1][0] * fx,
                lut[i0][1] * (1.0 - fx) + lut[i0 + 1][1] * fx,
                lut[i0][2] * (1.0 - fx) + lut[i0 + 1][2] * fx,
            ]
        };

        for &d in &[1.0_f32, 6.0_f32] {
            let exact = compute_transmitted_color(&sigma_liq, d, Illuminant::D65, false).xyz;
            let approx = sample_lut(d);
            for c in 0..3 {
                assert!(
                    (exact[c] - approx[c]).abs() < 1e-3,
                    "d={d} channel {c}: exact={} vs lut={}",
                    exact[c],
                    approx[c]
                );
            }
        }
    }
}
