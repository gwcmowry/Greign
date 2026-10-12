//! Image-space zoom independent of draft/full texture dimensions.
use eframe::egui::{Pos2, Rect, Vec2};
pub struct View {
    pub key: String,
    pub scale: f32,
    pub pan: Vec2,
    pub native_pending: bool,
    pub percent: Option<f32>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            key: String::new(),
            scale: 1.0,
            pan: Vec2::ZERO,
            native_pending: false,
            percent: None,
        }
    }
}
impl View {
    pub fn fit(&mut self) {
        self.scale = 1.0;
        self.pan = Vec2::ZERO;
        self.native_pending = false;
    }
    pub fn rect(&self, viewport: Rect, fit: Vec2) -> Rect {
        Rect::from_center_size(viewport.center() + self.pan, fit * self.scale)
    }
    pub fn clamp(&mut self, viewport: Rect, fit: Vec2) {
        if self.scale <= 1.0 {
            self.pan = Vec2::ZERO;
            return;
        }
        // Permit borders while zooming so portrait/letterboxed images retain
        // the cursor anchor too. Keep at least a small part of the image reachable.
        let limit =
            ((fit * self.scale + viewport.size()) * 0.5 - Vec2::splat(32.0)).max(Vec2::ZERO);
        self.pan = self.pan.clamp(-limit, limit);
    }
    pub fn zoom_at(&mut self, factor: f32, pointer: Pos2, viewport: Rect, fit: Vec2) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let previous = self.scale;
        self.scale = (self.scale * factor).clamp(1.0, 32.0);
        let pivot = pointer - viewport.center();
        self.pan = pivot - (pivot - self.pan) * (self.scale / previous);
        self.clamp(viewport, fit);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_anchor_survives_zoom_and_resolution_refinement() {
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let fit = Vec2::new(800.0, 600.0);
        let pointer = Pos2::new(500.0, 350.0);
        let mut view = View::default();
        let before = (pointer - view.rect(viewport, fit).min) / view.rect(viewport, fit).size();
        view.zoom_at(2.0, pointer, viewport, fit);
        let after = (pointer - view.rect(viewport, fit).min) / view.rect(viewport, fit).size();
        assert!((before - after).length() < 1e-6);
        // A new texture size is deliberately not an input to the transform.
        view.zoom_at(0.5, pointer, viewport, fit);
        assert!((view.scale - 1.0).abs() < 1e-6);
        assert_eq!(view.pan, Vec2::ZERO);
        let fit = Vec2::new(800.0, 400.0);
        let before = (pointer - view.rect(viewport, fit).min) / view.rect(viewport, fit).size();
        view.zoom_at(1.1, pointer, viewport, fit);
        let after = (pointer - view.rect(viewport, fit).min) / view.rect(viewport, fit).size();
        assert!((before - after).length() < 1e-6);
    }
}
