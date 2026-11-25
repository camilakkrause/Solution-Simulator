use std::f32::consts::LN_10;
use std::path::Path;

pub const LAMBDA_MIN: f32 = 380.0;
pub const LAMBDA_MAX: f32 = 780.0;
pub const SPECTRUM_LEN: usize = 401;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BaselineMode {
    Raw,
    AutoNirZero,
    Manual(f32),
}

impl BaselineMode {
    pub fn offset(self, nir_baseline: f32) -> f32 {
        match self {
            BaselineMode::Raw => 0.0,
            BaselineMode::AutoNirZero => nir_baseline,
            BaselineMode::Manual(v) => v,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AbsorbanceSpectrum {
    pub name: String,
    pub raw_points: Vec<(f32, f32)>,
    pub absorbance_1nm: [f32; SPECTRUM_LEN],
    pub min_lambda: f32,
    pub max_lambda: f32,
    pub visible_peak_lambda: f32,
    pub visible_peak_abs: f32,
    pub nir_baseline: f32,
    pub warning: Option<String>,
}

impl AbsorbanceSpectrum {
    /// Build an `AbsorbanceSpectrum` from unsorted `(wavelength_nm, absorbance)` pairs.
    pub fn from_raw_points(name: impl Into<String>, mut points: Vec<(f32, f32)>) -> Result<Self, String> {
        let name = name.into();
        if points.len() < 2 {
            return Err(format!(
                "Spectrum '{name}' must contain at least 2 valid data rows after skipping the first 2 header lines."
            ));
        }

        points.sort_by(|a, b| a.0.total_cmp(&b.0));

        // Deduplicate identical or nearly identical wavelengths by averaging their absorbance.
        let mut deduped: Vec<(f32, f32)> = Vec::with_capacity(points.len());
        let mut i = 0;
        while i < points.len() {
            let lambda0 = points[i].0;
            let mut sum = points[i].1;
            let mut count = 1usize;
            while i + count < points.len() && (points[i + count].0 - lambda0).abs() <= 1e-5 {
                sum += points[i + count].1;
                count += 1;
            }
            deduped.push((lambda0, sum / count as f32));
            i += count;
        }

        if deduped.len() < 2 {
            return Err(format!(
                "Spectrum '{name}' must have at least 2 distinct wavelengths."
            ));
        }

        let min_lambda = deduped.first().unwrap().0;
        let max_lambda = deduped.last().unwrap().0;

        let mut absorbance_1nm = [0.0f32; SPECTRUM_LEN];
        for (idx, slot) in absorbance_1nm.iter_mut().enumerate() {
            let lambda = LAMBDA_MIN + idx as f32;
            *slot = sample_piecewise_linear(&deduped, lambda);
        }

        let mut visible_peak_lambda = LAMBDA_MIN;
        let mut visible_peak_abs = f32::NEG_INFINITY;
        for (idx, &a) in absorbance_1nm.iter().enumerate() {
            if a > visible_peak_abs {
                visible_peak_abs = a;
                visible_peak_lambda = LAMBDA_MIN + idx as f32;
            }
        }

        // Compute median absorbance across 700..=780 nm (indices 320..=400).
        let mut nir_slice: Vec<f32> = absorbance_1nm[320..=400].to_vec();
        nir_slice.sort_by(|a, b| a.total_cmp(b));
        let nir_baseline = nir_slice[nir_slice.len() / 2];

        let mut warnings = Vec::new();
        if min_lambda > LAMBDA_MIN || max_lambda < LAMBDA_MAX {
            warnings.push(format!(
                "Source spans {min_lambda:.1}–{max_lambda:.1} nm; clamped endpoints to cover 380–780 nm."
            ));
        }
        if deduped.len() < 15 {
            warnings.push(format!(
                "Sparse wavelength grid ({} points); linearly interpolated to 1 nm.",
                deduped.len()
            ));
        }
        if visible_peak_abs > 3.5 {
            warnings.push(format!(
                "High visible peak absorbance ({visible_peak_abs:.2}); solution may appear opaque at full path length."
            ));
        }

        let warning = if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" "))
        };

        Ok(Self {
            name,
            raw_points: deduped,
            absorbance_1nm,
            min_lambda,
            max_lambda,
            visible_peak_lambda,
            visible_peak_abs,
            nir_baseline,
            warning,
        })
    }

    /// Napierian absorption coefficient sigma_a(lambda) in cm^-1 across 380..=780 nm.
    pub fn effective_sigma_a(
        &self,
        path_length_cm: f32,
        conc_scale: f32,
        baseline: BaselineMode,
    ) -> [f32; SPECTRUM_LEN] {
        let offset = baseline.offset(self.nir_baseline);
        let factor = LN_10 * conc_scale.max(0.0) / path_length_cm.max(1e-4);
        let mut out = [0.0f32; SPECTRUM_LEN];
        for (dst, &abs) in out.iter_mut().zip(self.absorbance_1nm.iter()) {
            let a_eff = (abs - offset).max(0.0);
            *dst = a_eff * factor;
        }
        out
    }
}

fn sample_piecewise_linear(sorted: &[(f32, f32)], lambda: f32) -> f32 {
    if lambda <= sorted[0].0 {
        return sorted[0].1;
    }
    let last = sorted.len() - 1;
    if lambda >= sorted[last].0 {
        return sorted[last].1;
    }
    let pos = sorted.partition_point(|&(x, _)| x < lambda);
    if pos == 0 {
        return sorted[0].1;
    }
    let (x0, y0) = sorted[pos - 1];
    let (x1, y1) = sorted[pos];
    let denom = x1 - x0;
    if denom <= 1e-6 {
        return 0.5 * (y0 + y1);
    }
    let t = (lambda - x0) / denom;
    y0 + t * (y1 - y0)
}

/// Parse a UV-Vis absorbance spectrum `.txt` file, skipping the first 2 header lines.
pub fn parse_spectrum_file(path: &Path) -> Result<AbsorbanceSpectrum, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("spectrum.txt");
    parse_spectrum_str(name, &text)
}

/// Parse a UV-Vis absorbance spectrum from text, strictly ignoring the first 2 lines.
pub fn parse_spectrum_str(name: &str, text: &str) -> Result<AbsorbanceSpectrum, String> {
    let mut points = Vec::new();
    for (idx_after_header, raw_line) in text.lines().skip(2).enumerate() {
        let line_num = idx_after_header + 3;
        let line = raw_line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        let (lambda, abs) = parse_two_columns(line).ok_or_else(|| {
            format!("Failed to parse 2 numeric columns on line {line_num}: '{line}'")
        })?;
        if !lambda.is_finite() || !abs.is_finite() {
            return Err(format!("Non-finite value on line {line_num}: '{line}'"));
        }
        points.push((lambda, abs));
    }
    AbsorbanceSpectrum::from_raw_points(name, points)
}

fn parse_two_columns(line: &str) -> Option<(f32, f32)> {
    let tokens: Vec<String> = if line.contains('\t') || line.contains(';') {
        line.split(['\t', ';'])
            .map(clean_token_decimal_comma)
            .filter(|s| !s.is_empty())
            .collect()
    } else if line.split_whitespace().count() >= 2 {
        line.split_whitespace()
            .map(clean_token_decimal_comma)
            .filter(|s| !s.is_empty())
            .collect()
    } else if line.contains(',') {
        line.split(',')
            .map(clean_token_basic)
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        return None;
    };

    if tokens.len() < 2 {
        return None;
    }
    let lambda = tokens[0].parse::<f32>().ok()?;
    let abs = tokens[1].parse::<f32>().ok()?;
    Some((lambda, abs))
}

fn clean_token_basic(raw: &str) -> String {
    raw.trim().trim_matches('"').trim_matches('\'').to_string()
}

fn clean_token_decimal_comma(raw: &str) -> String {
    clean_token_basic(raw).replace(',', ".")
}

fn gaussian(x: f32, center: f32, sigma: f32, amp: f32) -> f32 {
    let d = (x - center) / sigma;
    amp * (-0.5 * d * d).exp()
}

fn build_synthetic(name: &str, f: impl Fn(f32) -> f32) -> AbsorbanceSpectrum {
    let points: Vec<(f32, f32)> = (360..=800)
        .map(|wl| {
            let l = wl as f32;
            (l, f(l).max(0.0))
        })
        .collect();
    AbsorbanceSpectrum::from_raw_points(name, points).expect("valid synthetic preset")
}

/// Built-in realistic UV-Vis demo spectra.
pub fn builtin_presets() -> Vec<AbsorbanceSpectrum> {
    vec![
        build_synthetic("Thiaz-like Amber Dye (484 nm)", |l| {
            gaussian(l, 484.0, 29.0, 0.54)
                + gaussian(l, 444.0, 26.0, 0.24)
                + gaussian(l, 382.0, 22.0, 0.08)
        }),
        build_synthetic("Potassium Permanganate (KMnO4)", |l| {
            gaussian(l, 506.0, 10.5, 0.56)
                + gaussian(l, 526.0, 9.5, 0.92)
                + gaussian(l, 546.0, 9.5, 0.88)
                + gaussian(l, 567.0, 10.0, 0.36)
                + gaussian(l, 365.0, 18.0, 0.35)
        }),
        build_synthetic("Bromophenol Blue (Dichromatic)", |l| {
            gaussian(l, 592.0, 18.0, 1.65)
                + gaussian(l, 555.0, 16.0, 0.65)
                + gaussian(l, 478.0, 34.0, 0.24)
                + gaussian(l, 415.0, 26.0, 0.55)
        }),
        build_synthetic("Copper(II) Sulfate (CuSO4)", |l| {
            gaussian(l, 805.0, 92.0, 0.85) + gaussian(l, 355.0, 16.0, 0.10)
        }),
        build_synthetic("Nickel(II) Sulfate (NiSO4)", |l| {
            gaussian(l, 395.0, 23.0, 0.56)
                + gaussian(l, 655.0, 32.0, 0.20)
                + gaussian(l, 725.0, 44.0, 0.36)
        }),
        build_synthetic("Pure Water (Blank, A = 0)", |_| 0.0),
    ]
}

pub fn preset_by_name_or_slug(query: &str) -> Option<AbsorbanceSpectrum> {
    let q = query.trim().to_ascii_lowercase();
    let presets = builtin_presets();
    if let Some(exact) = presets.iter().find(|p| p.name.to_ascii_lowercase() == q) {
        return Some(exact.clone());
    }
    let idx = match q.as_str() {
        "thiaz" | "amber" => Some(0),
        "kmno4" | "permanganate" => Some(1),
        "bromophenol" | "dichromatic" | "bpb" => Some(2),
        "cuso4" | "copper" => Some(3),
        "niso4" | "nickel" => Some(4),
        "water" | "blank" => Some(5),
        _ => None,
    };
    if let Some(i) = idx {
        return Some(presets[i].clone());
    }
    presets
        .into_iter()
        .find(|p| p.name.to_ascii_lowercase().contains(&q))
}
