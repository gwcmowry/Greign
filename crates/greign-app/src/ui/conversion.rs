//! Conversion settings, comparison controls, and explicitly scoped results.
use super::{Action, App, Color32, GOLD, Material, RichText, Roll, egui, muted, primary};
use crate::model::Conversion;

pub(super) fn build_label(app: &App, roll: &Roll) -> &'static str {
    if app
        .pending_analysis
        .as_ref()
        .is_some_and(|p| p.roll_id == roll.id)
    {
        "Update preview"
    } else if roll.analysis.is_some() {
        "Build new preview"
    } else {
        "Build conversion preview"
    }
}

fn settings_text(conversion: &Conversion, frame: Option<&str>) -> String {
    let options = frame
        .and_then(|id| conversion.for_frame(id))
        .unwrap_or(&conversion.shared)
        .options;
    format!(
        "{} · Color balance {:.0}% · Lighting correction {}",
        if conversion.individual {
            "Individual frames"
        } else {
            "Shared roll"
        },
        options.color_balance * 100.,
        if options.illumination { "on" } else { "off" }
    )
}

pub(super) fn review(app: &mut App, ui: &mut egui::Ui, roll: &Roll) {
    let Some(pending) = &app.pending_analysis else {
        if roll
            .analysis
            .as_ref()
            .is_some_and(|a| !a.matches_settings(roll))
        {
            ui.colored_label(GOLD, "Settings changed · build a new preview to see them");
        }
        return;
    };
    if pending.roll_id != roll.id {
        return;
    }
    let proposal = &pending.conversion;
    let target = pending
        .frame_id
        .as_ref()
        .and_then(|id| roll.frames.iter().position(|f| f.id == *id));
    let target_name = target.map(|i| roll.frames[i].name.clone());
    let stale = proposal.is_stale(roll);
    let settings = settings_text(proposal, pending.frame_id.as_deref());
    ui.group(|ui| {
        let viewing = if app.comparing {
            "SAVED VERSION"
        } else if app.show_negative {
            "ORIGINAL SCAN · conversion preview is ready"
        } else if app.preview_analysis {
            "NEW PREVIEW · not applied"
        } else {
            "CURRENT CONVERSION · new preview is ready"
        };
        ui.label(RichText::new(viewing).strong().color(GOLD));
        if let Some(name) = &target_name {
            ui.label(RichText::new(format!("This frame only · {name}")).strong());
        }
        muted(ui, format!("Preview settings: {settings}"));
        if stale {
            ui.colored_label(
                GOLD,
                "Settings changed since this preview was built. Update it before applying.",
            );
        }
        ui.add_enabled_ui(!app.busy && !app.comparing, |ui| {
            ui.horizontal_wrapped(|ui| {
                let current = if roll.analysis.is_some() {
                    "Current conversion"
                } else {
                    "Original scan"
                };
                for (preview, label) in [(false, current), (true, "New preview")] {
                    if ui
                        .selectable_label(
                            app.preview_analysis == preview && !app.show_negative,
                            label,
                        )
                        .clicked()
                    {
                        app.preview_analysis = preview;
                        app.show_negative = false;
                        app.invalidate();
                    }
                }
                ui.separator();
                if stale && primary(ui, "Update preview").clicked() {
                    app.actions
                        .push(target.map_or(Action::Analyze, Action::AnalyzeFrame));
                }
                if ui
                    .add_enabled(
                        !stale && !roll.conversion_locked,
                        egui::Button::new(
                            RichText::new(if target.is_some() {
                                "Apply to this frame"
                            } else {
                                "Apply conversion"
                            })
                            .color(super::BG),
                        )
                        .fill(GOLD),
                    )
                    .clicked()
                {
                    app.actions.push(Action::AcceptAnalysis);
                }
                if ui.button("Discard preview").clicked() {
                    app.actions.push(Action::DiscardAnalysis);
                }
            });
            let mut keep = !app.neutral_analysis;
            if ui
                .checkbox(
                    &mut keep,
                    if target.is_some() {
                        "Keep this frame's lighting and color edits"
                    } else {
                        "Keep my lighting and color edits"
                    },
                )
                .changed()
            {
                app.neutral_analysis = !keep;
                app.invalidate();
            }
        });
        let edits = if app.neutral_analysis {
            "Uses neutral lighting and color."
        } else {
            "Includes your lighting and color edits."
        };
        let recovery = if roll.analysis.is_some() {
            "Your previous look is saved as a version."
        } else {
            "Undo returns to the original scan."
        };
        muted(
            ui,
            format!("{edits} Apply to edit or export this look. {recovery}"),
        );
        if target.is_some() {
            muted(
                ui,
                "Other frames keep their applied conversions and lighting/color edits.",
            );
        }
        if roll.conversion_locked {
            muted(
                ui,
                "Unlock the conversion in the side panel to apply this preview.",
            );
        }
    });
}

pub(super) fn panel(app: &mut App, ui: &mut egui::Ui) {
    if app.busy || app.comparing {
        ui.disable();
    }
    ui.label(RichText::new("Conversion").size(18.));
    let roll = app.library.rolls[app.roll].clone();
    if roll.material == Material::Positive {
        muted(
            ui,
            "This roll contains positive images. Use Adjust for lighting and color.",
        );
        return;
    }
    if roll.optimize_individually {
        if let Some(frame) = roll.frames.get(app.frame) {
            ui.add_space(8.);
            ui.group(|ui| {
                ui.label(RichText::new(format!("Frame film base · {}",frame.name)).strong());
                muted(ui,if frame.base.is_some() { "Manual film-base patch" } else { "Automatic film-base estimate" });
                if ui.add_enabled(!roll.conversion_locked && app.cache.contains_key(&frame.id),egui::Button::new("Sample frame film base")).clicked() { app.actions.push(Action::SampleBase); }
                if frame.base.is_some() && ui.add_enabled(!roll.conversion_locked,egui::Button::new("Use automatic for this frame")).clicked() {
                    app.actions.push(Action::SetBase {frame:Some(app.frame),base:None});
                }
                if ui.add_enabled(!roll.conversion_locked,egui::Button::new("Preview this frame")).clicked() { app.actions.push(Action::AnalyzeFrame(app.frame)); }
                muted(ui,"Sample clean, unexposed film outside the picture. A preview appears automatically for this frame only, even with multiple frames selected.");
            });
        }
    }
    ui.add_space(8.);
    ui.label(RichText::new("Whole-roll conversion").strong());
    if roll.material == Material::ColorNegative {
        ui.add_space(8.);
        if ui
            .add_enabled(
                !roll.conversion_locked && !roll.frames.is_empty(),
                egui::Button::new(RichText::new("Preview faded-film restoration").color(super::BG))
                    .fill(GOLD),
            )
            .clicked()
        {
            app.actions.push(Action::PreviewFaded);
        }
        muted(
            ui,
            "Sets color balance to 100%, corrects uneven lighting, and builds an individual-frame preview.",
        );
    }
    ui.add_space(12.);
    let mut options = roll.calibration;
    ui.add_enabled_ui(!roll.conversion_locked, |ui| {
        ui.checkbox(
            &mut options.illumination,
            "Correct uneven film-base lighting",
        );
        muted(ui, "Uses clean film borders to even out the backlight.");
        if roll.material == Material::ColorNegative {
            ui.add_space(8.);
            ui.label("Faded-film color balance");
            let mut percent = options.color_balance * 100.;
            if ui
                .add(
                    egui::Slider::new(&mut percent, 0.0..=100.)
                        .suffix("%")
                        .step_by(1.),
                )
                .changed()
            {
                options.color_balance = percent / 100.;
            }
            muted(
                ui,
                if options.color_balance == 0. {
                    "Off · no faded-film color balancing"
                } else {
                    "Balances faded colors. Reduce it if a scene is dominated by one color."
                },
            );
        }
    });
    if options != roll.calibration {
        app.library.rolls[app.roll].calibration = options;
        app.dirty = true;
        app.status = "Settings changed · update the conversion preview".into();
    }
    let roll = app.library.rolls[app.roll].clone();
    ui.add_space(12.);
    if ui
        .add_enabled(
            !roll.frames.is_empty(),
            egui::Button::new(build_label(app, &roll)),
        )
        .clicked()
    {
        app.actions.push(Action::Analyze);
    }
    let pending = app
        .pending_analysis
        .as_ref()
        .filter(|p| p.roll_id == roll.id);
    if let Some(proposal) = pending {
        if proposal.conversion.is_stale(&roll) {
            ui.colored_label(GOLD, "Settings changed · update preview");
        } else {
            muted(
                ui,
                "Compare Current conversion and New preview above the images, then Apply conversion.",
            );
        }
    } else if roll
        .analysis
        .as_ref()
        .is_some_and(|a| !a.matches_settings(&roll))
    {
        ui.colored_label(GOLD, "Settings changed · not yet previewed");
    } else {
        muted(
            ui,
            "A new preview appears automatically when analysis finishes.",
        );
    }
    ui.add_space(8.);
    app.dirty |= ui
        .add_enabled(
            roll.analysis.is_some(),
            egui::Checkbox::new(
                &mut app.library.rolls[app.roll].conversion_locked,
                "Lock applied conversion",
            ),
        )
        .changed();
    ui.separator();
    results(app, ui, &roll);
    ui.separator();
    ui.collapsing("Advanced calibration", |ui| {
        if ui.button("Inspect original scan").clicked() { app.show_negative = true; app.grid = false; app.invalidate(); }
        if let Some(warning) = roll.frames.get(app.frame).and_then(|f| f.scan_warning.as_ref()) { muted(ui, warning); }
        ui.add_space(8.);
        if ui.add_enabled(!roll.frames.is_empty() && !roll.conversion_locked && !roll.optimize_individually,
            egui::Button::new("Select film-base patch")).clicked() {
            app.actions.push(Action::SampleBase);
        }
        muted(ui, if roll.optimize_individually { "Use Frame film base above to override the active frame." } else { "Select clean, unexposed film outside the picture. This base is shared by the roll." });
        if ui.add_enabled(roll.base.is_some() && !roll.demo && !roll.conversion_locked && !roll.optimize_individually, egui::Button::new("Use automatic base estimate")).clicked() {
            app.actions.push(Action::SetBase {frame:None,base:None});
        }
        ui.add_space(8.);
        ui.label("Analysis area");
        let mut inset = roll.analysis_area.x;
        if ui.add_enabled(!roll.conversion_locked, egui::Slider::new(&mut inset, 0.0..=0.25).text("Inset")).changed() {
            app.library.rolls[app.roll].analysis_area = super::Rect { x:inset,y:inset,w:1.-2.*inset,h:1.-2.*inset,angle:0. };
            app.dirty = true;
        }
        muted(ui,"Restricts color analysis inside the detected picture; does not crop the export. Update preview after changing it.");
        if ui.add_enabled(roll.previous_analysis.is_some() && !roll.conversion_locked, egui::Button::new("Restore previous conversion")).clicked() { app.actions.push(Action::RestoreAnalysis); }
    });
}

fn results(app: &App, ui: &mut egui::Ui, roll: &Roll) {
    let Some(conversion) = app.displayed_conversion(roll) else {
        muted(ui, "Build a preview to inspect the conversion results.");
        return;
    };
    ui.label(
        RichText::new(if app.comparing {
            "Saved version results"
        } else if app.preview_analysis {
            "New preview results"
        } else {
            "Current conversion results"
        })
        .strong(),
    );
    muted(
        ui,
        settings_text(
            conversion,
            roll.frames.get(app.frame).map(|f| f.id.as_str()),
        ),
    );
    if let Some(frame) = roll.frames.get(app.frame) {
        ui.add_space(8.);
        ui.label(RichText::new(format!("Selected frame · {}", frame.name)).strong());
        if let Some(analysis) = conversion.for_frame(&frame.id) {
            let applied = analysis.illumination.iter().any(|v| *v != 0.);
            if analysis.options.illumination {
                ui.colored_label(
                    if applied { Color32::LIGHT_GREEN } else { GOLD },
                    if applied {
                        "Lighting correction applied"
                    } else {
                        "Lighting correction unavailable for this frame"
                    },
                );
            } else {
                muted(ui, "Lighting correction off");
            }
            if !conversion.individual && !analysis.warnings.is_empty() {
                muted(ui, "Uses shared calibration; see the roll warnings below.");
            } else if analysis.warnings.is_empty() {
                muted(ui, "No conversion warnings for this frame.");
            } else {
                ui.collapsing(
                    format!(
                        "This frame · {} {}",
                        analysis.warnings.len(),
                        if analysis.warnings.len() == 1 {
                            "warning"
                        } else {
                            "warnings"
                        }
                    ),
                    |ui| {
                        for warning in &analysis.warnings {
                            ui.label(warning);
                        }
                    },
                );
            }
            ui.collapsing("Measurement details", |ui| {
                ui.label(&analysis.method);
                muted(
                    ui,
                    format!(
                        "Film base {:.3} / {:.3} / {:.3}\nDensity range {:.2}",
                        analysis.base[0], analysis.base[1], analysis.base[2], analysis.range
                    ),
                );
            });
        } else {
            ui.colored_label(
                GOLD,
                "This frame has not been analyzed. Build a new preview to include it.",
            );
        }
    }
    let frames: Vec<_> = roll
        .frames
        .iter()
        .filter(|f| {
            !f.rejected
                && f.error.is_none()
                && conversion
                    .inputs
                    .as_ref()
                    .is_none_or(|i| i.frames.contains(&f.id))
        })
        .filter_map(|f| conversion.for_frame(&f.id).map(|a| (f, a)))
        .collect();
    let affected = frames
        .iter()
        .filter(|(_, a)| !a.warnings.is_empty())
        .count();
    ui.add_space(8.);
    ui.label(RichText::new("Across this roll").strong());
    if frames.iter().any(|(_, a)| a.options.illumination) {
        let corrected = frames
            .iter()
            .filter(|(_, a)| a.illumination.iter().any(|v| *v != 0.))
            .count();
        muted(
            ui,
            format!(
                "Lighting corrected on {corrected} of {} analyzed frames.",
                frames.len()
            ),
        );
        let skipped: Vec<_> = frames
            .iter()
            .filter(|(_, a)| a.illumination.iter().all(|v| *v == 0.))
            .map(|(f, _)| f.name.as_str())
            .collect();
        if !skipped.is_empty() {
            ui.collapsing(
                format!(
                    "Lighting unchanged · {} {}",
                    skipped.len(),
                    if skipped.len() == 1 {
                        "frame"
                    } else {
                        "frames"
                    }
                ),
                |ui| {
                    for name in skipped {
                        ui.label(name);
                    }
                },
            );
        }
    }
    if !conversion.individual && !conversion.shared.warnings.is_empty() {
        ui.collapsing("Shared roll warnings", |ui| {
            for warning in &conversion.shared.warnings {
                ui.label(warning);
            }
        });
    } else if affected > 0 {
        ui.collapsing(
            format!(
                "Roll warnings · {affected} {}",
                if affected == 1 { "frame" } else { "frames" }
            ),
            |ui| {
                for (frame, analysis) in &frames {
                    if !analysis.warnings.is_empty() {
                        ui.label(RichText::new(&frame.name).strong());
                        for warning in &analysis.warnings {
                            ui.label(warning);
                        }
                    }
                }
            },
        );
    } else {
        muted(ui, "No conversion warnings across analyzed frames.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnalysisInputs, Frame, Library};

    fn fixture() -> (tempfile::TempDir, egui::Context, App) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.path().into(), Library::default(), None);
        let mut roll = Roll::new("Conversion review");
        roll.frames = vec![
            Frame::new("clean.png".into()),
            Frame::new("uncertain.png".into()),
        ];
        roll.calibration = greign_film::CalibrationOptions {
            illumination: true,
            color_balance: 1.,
        };
        let analysis = greign_film::Analysis {
            version: greign_film::ANALYSIS_VERSION,
            base: [0.4, 0.2, 0.1],
            floor: [0.; 3],
            range: 1.,
            channel_scale: [1.; 3],
            illumination: [0.1, 0., 0., 0., 0.],
            options: roll.calibration,
            method: "fixture".into(),
            warnings: vec![],
            frames: 1,
        };
        let mut unavailable = analysis.clone();
        unavailable.illumination = [0.; 5];
        unavailable
            .warnings
            .push("Border unavailable on uncertain.png".into());
        let proposal = Conversion {
            frame_inputs: std::collections::BTreeMap::new(),
            shared: analysis.clone(),
            individual: true,
            per_frame: std::collections::BTreeMap::from([
                (roll.frames[0].id.clone(), analysis.clone()),
                (roll.frames[1].id.clone(), unavailable),
            ]),
            inputs: Some(AnalysisInputs {
                frame_bases: std::collections::BTreeMap::new(),
                base: None,
                area: roll.analysis_area,
                frames: roll.frames.iter().map(|f| f.id.clone()).collect(),
            }),
        };
        let mut accepted = analysis;
        accepted.options = greign_film::CalibrationOptions::default();
        accepted.illumination = [0.; 5];
        roll.analysis = Some(accepted.into());
        app.pending_analysis = Some(crate::model::ProposedConversion {
            roll_id: roll.id.clone(),
            conversion: proposal,
            frame_id: None,
        });
        app.library.rolls.push(roll);
        app.preview_analysis = true;
        app.panel = crate::Panel::Conversion;
        (dir, ctx, app)
    }
    fn run(
        app: &mut App,
        ctx: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 960.),
                )),
                events,
                ..Default::default()
            },
            |ctx| super::super::draw(app, ctx),
        )
    }
    fn position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing control: {label}"))
    }
    fn texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text()),
                _ => None,
            })
            .collect()
    }
    fn click(app: &mut App, ctx: &egui::Context, width: f32, label: &str) {
        let p = position(&run(app, ctx, width, vec![]), label);
        for pressed in [true, false] {
            run(
                app,
                ctx,
                width,
                vec![
                    egui::Event::PointerMoved(p),
                    egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
    }
    #[test]
    fn conversion_controls_fit_the_sidebar_and_scope_warnings_to_the_right_frame() {
        for width in [1080., 1440.] {
            let (_dir, ctx, mut app) = fixture();
            let output = run(&mut app, &ctx, width, vec![]);
            let labels = texts(&output);
            for label in [
                "Preview faded-film restoration",
                "Faded-film color balance",
                "Frame film base · clean.png",
                "Sample frame film base",
                "Preview this frame",
            ] {
                assert!(labels.contains(&label), "Missing {label}: {labels:?}");
            }
            assert!(
                !labels.contains(&"Border unavailable on uncertain.png"),
                "A different frame's warning must not be presented as this frame's result"
            );
            // The right panel scrolls; inspect its result section independently
            // of the settings above it to check warning scope at both widths.
            let summary_ctx = egui::Context::default();
            super::super::style(&summary_ctx);
            let summary = summary_ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(300., 960.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| results(&app, ui, &app.library.rolls[0]));
                },
            );
            let labels = texts(&summary);
            for label in [
                "Selected frame · clean.png",
                "Lighting correction applied",
                "Lighting corrected on 1 of 2 analyzed frames.",
                "Roll warnings · 1 frame",
            ] {
                assert!(labels.contains(&label), "Missing {label}");
            }
            assert!(!labels.contains(&"Border unavailable on uncertain.png"));
            for shape in &output.shapes {
                if let egui::Shape::Text(t) = &shape.shape {
                    if shape.clip_rect.min.x >= width - 301. && t.pos.y > 80. {
                        assert!(
                            t.pos.x + t.galley.size().x <= width + 1.,
                            "Sidebar overflow: {}",
                            t.galley.text()
                        );
                    }
                }
            }
            click(&mut app, &ctx, width, "Preview faded-film restoration");
            assert!(
                app.actions
                    .iter()
                    .any(|a| matches!(a, Action::PreviewFaded))
            );
        }
    }
    #[test]
    fn comparison_switches_views_and_stale_previews_cannot_be_applied_from_the_ui() {
        let (_dir, ctx, mut app) = fixture();
        click(&mut app, &ctx, 1440., "Current conversion");
        assert!(!app.preview_analysis);
        assert!(
            texts(&run(&mut app, &ctx, 1440., vec![]))
                .contains(&"CURRENT CONVERSION · new preview is ready")
        );
        click(&mut app, &ctx, 1440., "New preview");
        assert!(app.preview_analysis);
        app.library.rolls[0].calibration.color_balance = 0.5;
        let output = run(&mut app, &ctx, 1440., vec![]);
        assert!(texts(&output).contains(
            &"Settings changed since this preview was built. Update it before applying."
        ));
        app.actions.clear();
        click(&mut app, &ctx, 1440., "Apply conversion");
        assert!(
            !app.actions
                .iter()
                .any(|a| matches!(a, Action::AcceptAnalysis))
        );
        click(&mut app, &ctx, 1440., "Update preview");
        assert!(app.actions.iter().any(|a| matches!(a, Action::Analyze)));
    }
}
