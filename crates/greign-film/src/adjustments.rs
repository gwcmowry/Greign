//! Luminance-domain lighting and a feathered working-RGB color mixer.
use serde::{Deserialize, Serialize};

/// Color-mixer labels in hue order.
pub const COLOR_NAMES: [&str; 8] = [
    "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
];

/// One color band's creative offsets. Zero is an exact bypass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorBand {
    /// Hue rotation in degrees, -30 through 30.
    pub hue: f32,
    /// Chroma offset in percent, -100 through 100.
    pub saturation: f32,
    /// Brightness offset, -100 through 100 (one stop each way).
    pub lightness: f32,
}
impl ColorBand {
    pub(crate) fn combined(self, other: Self) -> Self {
        Self {
            hue: self.hue + other.hue,
            saturation: self.saturation + other.saturation,
            lightness: self.lightness + other.lightness,
        }
    }
}
fn smooth(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

pub(crate) fn lighting(pixel: &mut [f32; 3], shadows: f32, highlights: f32) {
    if shadows == 0.0 && highlights == 0.0 {
        return;
    }
    let luminance = greign_pipeline::luminance(*pixel).max(0.000_001);
    let stops = (luminance / 0.18).log2();
    // Wide log-luminance transitions keep the curve monotonic even at extremes.
    // Scaling all channels together preserves chromaticity and does not clip.
    let exposure = 2.0
        * (shadows.clamp(-100.0, 100.0) / 100.0 * smooth(-stops / 5.0)
            + highlights.clamp(-100.0, 100.0) / 100.0 * smooth(stops / 4.0));
    for value in pixel {
        *value *= exposure.exp2();
    }
}

// Exact comparisons identify a channel selected by max(), and exact bypass values.
#[allow(clippy::many_single_char_names, clippy::float_cmp)]
pub(crate) fn mix_color(pixel: &mut [f32; 3], bands: &[ColorBand; 8]) {
    let original_y = greign_pipeline::luminance(*pixel);
    let [r, g, b] = *pixel;
    let hi = r.max(g).max(b);
    let lo = r.min(g).min(b);
    let chroma = hi - lo;
    if hi <= 0.0 || chroma <= hi * 0.0001 || original_y <= 0.0 {
        return;
    }
    let hue = if hi == r {
        (g - b) / chroma
    } else if hi == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    } * 60.0;
    let hue = hue.rem_euclid(360.0);
    let centers = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0, 360.0];
    let index = (0..8).find(|&i| hue < centers[i + 1]).unwrap_or(7);
    let blend = smooth((hue - centers[index]) / (centers[index + 1] - centers[index]));
    let a = bands[index];
    let b = bands[(index + 1) % 8];
    let interpolate = |a: f32, b: f32| a * (1.0 - blend) + b * blend;
    // Suppress unstable hue changes in almost-neutral pixels.
    let strength = smooth(chroma / hi / 0.08);
    let shift = interpolate(a.hue, b.hue).clamp(-60.0, 60.0) * strength;
    let sat = (1.0
        + interpolate(a.saturation, b.saturation).clamp(-100.0, 200.0) / 100.0 * strength)
        .max(0.0);
    let light =
        (interpolate(a.lightness, b.lightness).clamp(-100.0, 100.0) / 100.0 * strength).exp2();
    if shift == 0.0 && sat == 1.0 && light == 1.0 {
        return;
    }
    let h = (hue + shift).rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let rotated = match h as u32 {
        0 => [chroma, x, 0.0],
        1 => [x, chroma, 0.0],
        2 => [0.0, chroma, x],
        3 => [0.0, x, chroma],
        4 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    }
    .map(|v| v + lo);
    let rotated_y = greign_pipeline::luminance(rotated);
    *pixel = rotated.map(|v| (original_y + (v - rotated_y) * sat) * light);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lighting_is_monotonic_neutral_and_has_exact_bypass() {
        for shadows in [-100.0, 0.0, 100.0] {
            for highlights in [-100.0, 0.0, 100.0] {
                let mut previous = 0.0;
                for i in 0..1000 {
                    let value = 2.0_f32.powf(-16.0 + i as f32 * 0.025);
                    let mut pixel = [value; 3];
                    lighting(&mut pixel, shadows, highlights);
                    assert!(pixel[0] > previous);
                    assert_eq!(pixel[0].to_bits(), pixel[1].to_bits());
                    if shadows == 0.0 && highlights == 0.0 {
                        assert_eq!(pixel[0].to_bits(), value.to_bits());
                    }
                    previous = pixel[0];
                }
            }
        }
    }
    #[test]
    fn mixer_is_selective_preserves_luminance_and_neutrals() {
        let mut bands = [ColorBand::default(); 8];
        bands[0].hue = 25.0;
        bands[0].saturation = -30.0;
        let mut red = [0.8, 0.1, 0.1];
        let y = greign_pipeline::luminance(red);
        mix_color(&mut red, &bands);
        assert!(red[1] > red[2]);
        assert!((greign_pipeline::luminance(red) - y).abs() < 1e-6);
        for pixel in [[0.2; 3], [0.0; 3], [0.1, 0.1, 0.8]] {
            let mut out = pixel;
            mix_color(&mut out, &bands);
            assert_eq!(out.map(f32::to_bits), pixel.map(f32::to_bits));
        }
        let mut left = [0.8, 0.1, 0.10001];
        let mut right = [0.8, 0.10001, 0.1];
        mix_color(&mut left, &bands);
        mix_color(&mut right, &bands);
        assert!((left[0] - right[0]).abs() < 0.001);
    }
}
