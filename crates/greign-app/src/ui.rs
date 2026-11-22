#![allow(clippy::collapsible_if)]
use super::{Action, App, Panel, Roll, Tool};
mod conversion;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use greign_film::{self as film, Adjustments, Heal, Material, Rect};

const BG: Color32 = Color32::from_rgb(25, 27, 29);
const PANEL: Color32 = Color32::from_rgb(32, 34, 36);
const GOLD: Color32 = Color32::from_rgb(222, 175, 104);
const MUTED: Color32 = Color32::from_rgb(158, 161, 162);

pub fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(19, 21, 23);
    v.faint_bg_color = Color32::from_rgb(39, 41, 44);
    v.override_text_color = Some(Color32::from_rgb(230, 230, 224));
    v.selection.bg_fill = Color32::from_rgb(83, 68, 47);
    v.selection.stroke = Stroke::new(1.0, GOLD);
    v.widgets.inactive.bg_fill = Color32::from_rgb(47, 49, 51);
    v.widgets.hovered.bg_fill = Color32::from_rgb(65, 65, 61);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(10.0, 10.0);
        s.spacing.button_padding = Vec2::new(12.0, 8.0);
        s.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
        s.text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
    });
}
fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.add(egui::Label::new(RichText::new(text).color(MUTED).size(12.0)).wrap());
}
fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(Color32::from_rgb(30, 27, 22))).fill(GOLD))
}

pub fn draw(app: &mut App, ctx: &egui::Context) {
    app.compare_button = false;
    egui::TopBottomPanel::top("brand")
        .exact_height(64.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(16.0);
                ui.label(RichText::new("G R E I G N").size(22.0).color(GOLD));
                ui.add_space(18.0);
                muted(ui, "F I L M   W O R K S P A C E");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    if ui
                        .add_enabled(!app.busy && !app.redo.is_empty(), egui::Button::new("Redo"))
                        .clicked()
                    {
                        app.actions.push(Action::Redo);
                    }
                    if ui
                        .add_enabled(
                            !app.busy && !app.history.is_empty(),
                            egui::Button::new("Undo"),
                        )
                        .clicked()
                    {
                        app.actions.push(Action::Undo);
                    }
                    muted(ui, "Local library  ·  Originals preserved");
                });
            });
        });
    egui::TopBottomPanel::bottom("status")
        .min_height(35.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if app.busy || app.rendering {
                    ui.spinner();
                } else {
                    ui.label(RichText::new("•").color(GOLD));
                }
                ui.label(RichText::new(&app.status).size(12.0));
                if app.busy {
                    if let Some(batch) = &app.export_batch {
                        ui.label(format!("Roll {}/{}", batch.completed + 1, batch.total));
                    }
                    if let Some((done, total, bytes, elapsed)) = app.export_progress {
                        ui.add(
                            egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                                .desired_width(150.)
                                .text(format!(
                                    "{done}/{total} · {:.1} MB · {elapsed:.0}s",
                                    bytes as f64 / 1_000_000.
                                )),
                        );
                    }
                }
                if app.rendering {
                    muted(ui, "Updating previews…");
                }
                let active = app
                    .library
                    .rolls
                    .get(app.roll)
                    .and_then(|r| r.frames.get(app.frame));
                let gpu_ready = active.is_some_and(|f| gpu_settings(app, &f.id).is_some());
                if !gpu_ready && active.is_some_and(|f| !f.heals.is_empty()) {
                    muted(ui, "CPU preview · manual healing");
                }
                if gpu_ready {
                    muted(ui, "GPU · full-resolution source");
                } else if app.dragging {
                    muted(ui, "Interactive draft · refines on release");
                } else if !gpu_ready
                    && app.full_desired.as_ref().is_some_and(|key| {
                        app.detail_texture
                            .as_ref()
                            .is_none_or(|(_, done, _)| done != key)
                    })
                {
                    muted(ui, "Refining full-resolution active frame…");
                }
                if let Some(e) = &app.save_error {
                    ui.colored_label(Color32::LIGHT_RED, format!("Save failed: {e}"));
                }
                if app.last_export.is_some() && ui.small_button("Open export folder").clicked() {
                    app.actions.push(Action::OpenExport);
                }
            });
        });
    egui::SidePanel::left("rolls")
        .exact_width(192.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.add_space(20.0);
            muted(ui, "YOUR ROLLS");
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("roll-list").max_height((ui.available_height() - 220.0).max(100.0)).show(ui, |ui| {
            for (i, r) in app.library.rolls.iter().enumerate() {
                let text = format!(
                    "{}\n{} frames{}",
                    r.name,
                    r.frames.len(),
                    if r.demo { " · Demo" } else { "" }
                );
                if ui
                    .add_enabled(
                        !app.busy,
                        egui::Button::new(text)
                            .selected(app.roll == i)
                            .min_size(Vec2::new(164.0, 62.0)),
                    )
                    .clicked()
                {
                    app.roll = i;
                    app.frame = 0;
                    app.reference = None;
                    app.selected.clear();
                    app.scope_selected = false;
                    app.version_index = 0;
                    app.pending_analysis = None;
                    app.preview_analysis = false;
                    app.tool = Tool::View;
                    app.actions.push(Action::Load);
                }
            }
            });
            ui.add_space(12.0);
            if ui
                .add_enabled(
                    !app.busy,
                    egui::Button::new("+  Import a roll").min_size(Vec2::new(164.0, 36.0)),
                )
                .clicked()
            {
                app.new_roll_open = true;
            }
            if ui.add_enabled(!app.busy && !app.preview_analysis && !app.comparing && !app.library.rolls.is_empty(), egui::Button::new("Export ready rolls…").min_size(Vec2::new(164.0, 32.0))).clicked() {
                app.export_scope = super::ExportScope::AllReady;
                app.export_dialog = true;
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                muted(
                    ui,
                    "Arrow keys  Navigate\nR  Rotate · Shift+R  Reverse\nX  Keep / reject\nCtrl Z  Undo\nEsc  Leave tool",
                );
                ui.separator();
                muted(
                    ui,
                    "A roll is the starting point.\nA frame is the finishing touch.",
                );
            });
        });
    if app.library.rolls.is_empty() {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(150.0);
                    ui.heading("Your next roll starts here.");
                    ui.label(
                        "Import your camera scans together. Convert once, then make them yours.",
                    );
                    if primary(ui, "Import a roll").clicked() {
                        app.new_roll_open = true;
                    }
                });
            });
        dialogs(app, ctx);
        return;
    }
    app.roll = app.roll.min(app.library.rolls.len() - 1);
    let count = app.library.rolls[app.roll].frames.len();
    app.frame = app.frame.min(count.saturating_sub(1));
    egui::SidePanel::right("adjustments")
        .exact_width(300.0)
        .resizable(false)
        .show(ctx, |ui| {
            right(app, ui);
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG).inner_margin(20.0))
        .show(ctx, |ui| {
            let r = app.library.rolls[app.roll].clone();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let kept = r.frames.iter().filter(|f| !f.rejected).count();
            let ready = r.material == Material::Positive || r.frames.iter().filter(|f| !f.rejected).all(|f| r.analysis_for(f).is_some());
                    if ui
                        .add_enabled(
                            !app.busy
                                && !app.preview_analysis
                                && !app.comparing
                                && ready
                                && kept > 0,
                            egui::Button::new(
                                RichText::new(format!("Export roll · {kept}")).color(BG),
                            )
                            .fill(GOLD),
                        )
                        .on_disabled_hover_text(if app.preview_analysis { "Apply the new conversion before exporting it." } else { "Finish processing and analyze all kept frames before exporting." })
                        .clicked()
                    {
                        app.export_scope = super::ExportScope::Roll;
                        if r.export.destination.is_some() {
                            app.actions.push(Action::Export);
                        } else {
                            app.export_dialog = true;
                        }
                    }
                    if ui.button("Export settings…").clicked() {
                        app.export_scope = super::ExportScope::Roll;
                        app.export_dialog = true;
                    }
                    if ui.button("Metadata").clicked() {
                        app.metadata_open = true;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&r.name).heading()).truncate()).on_hover_text(&r.name);
                    });
                });
            });
            ui.horizontal(|ui| {
                muted(
                    ui,
                    format!(
                        "{}  /  {} frames",
                        if r.stock.is_empty() {
                            "Film stock not set"
                        } else {
                            &r.stock
                        },
                        r.frames.len()
                    ),
                );
                if r.demo {
                    ui.label(
                        RichText::new("DEMO · SIMULATED NEGATIVES")
                            .size(11.0)
                            .color(GOLD),
                    );
                }
            });
            ui.add_space(10.0);
            if r.material != Material::Positive {
                ui.add_enabled_ui(!app.busy && !app.comparing && !r.conversion_locked, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Conversion:");
                        if ui.selectable_label(r.optimize_individually, "Optimize frames individually").clicked() {app.actions.push(Action::SetIndividual(true));}
                        if ui.selectable_label(!r.optimize_individually, "Shared roll calibration").clicked() {app.actions.push(Action::SetIndividual(false));}
                    });
                });
                muted(ui, if r.optimize_individually {"Each frame gets its own conversion. Build a preview, compare, then apply."} else {"One conversion for consistent capture settings. Build a preview, compare, then apply."});
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !app.busy && !r.frames.is_empty() && r.material != Material::Positive,
                        egui::Button::new(conversion::build_label(app, &r)),
                    )
                    .clicked()
                {
                    app.actions.push(Action::Analyze);
                }
                if ui
                    .add_enabled(!app.busy && !r.demo, egui::Button::new("Add scans"))
                    .clicked()
                {
                    app.actions.push(Action::Import);
                }
                if ui.add_enabled(!app.busy && !r.demo, egui::Button::new("Add folder…")).clicked() {
                    app.actions.push(Action::ImportFolder);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.selectable_label(!app.grid, "Single frame").clicked() {
                        app.grid = false;
                    }
                    if ui.selectable_label(app.grid, "Contact sheet").clicked() {
                        app.grid = true;
                        app.tool = Tool::View;
                    }
                });
            });
            ui.add_space(8.0);
            ui.add_enabled_ui(!app.busy && !app.comparing && !app.preview_analysis && !r.frames.is_empty(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Whole roll:");
                    if ui.button("Autocrop all frames").on_hover_text("Crop each picture inside its film base and straighten slight tilt. Uncertain frames keep their current crops. Undo restores the entire batch.").clicked() {
                        app.actions.push(Action::AutoCropAll);
                    }
                    if ui.add_enabled(r.material == Material::Positive || r.analysis.is_some(), egui::Button::new("Auto upright")).on_hover_text("Use a local face model to find upright people in converted photos. Clear matches get quarter-turn rotations; uncertain frames keep their orientation and are marked for review. Apply a conversion first. Undo restores the whole batch.").clicked() {
                        app.actions.push(Action::AutoUpright);
                    }
                    let mirrored_count = r.frames.iter().filter(|f| f.mirrored).count();
                    let mut mirrored = !r.frames.is_empty() && mirrored_count == r.frames.len();
                    let mixed = mirrored_count > 0 && !mirrored;
                    if ui.add(egui::Checkbox::new(&mut mirrored, "Mirror all frames").indeterminate(mixed)).on_hover_text("Set left-to-right mirroring for every frame. Each frame keeps its own rotation and crop.").changed() {
                        app.actions.push(Action::MirrorAll(mirrored));
                    }
                });
            });
            ui.add_space(8.0);
            if app.pending_analysis.as_ref().is_none_or(|p| p.roll_id != r.id) {
                if r.analysis.as_ref().is_some_and(|a| a.individual && a.per_frame.values().any(|analysis| analysis.version < film::ANALYSIS_VERSION)) {
                    ui.colored_label(GOLD, "Updated conversion available · build a new preview");
                } else if r.analysis.as_ref().is_some_and(|a| !a.shared.warnings.is_empty()) {
                    muted(ui, "Some frames have conversion notes · see Conversion for details");
                }
            }
            if app.grid && !r.frames.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    muted(
                        ui,
                        format!(
                            "{} selected · Ctrl-click toggles · Shift-click selects a range",
                            app.selected.len()
                        ),
                    );
                    if ui.small_button("Select all").clicked() {
                        app.selected = r.frames.iter().map(|f| f.id.clone()).collect();
                    }
                    if ui.small_button("Clear selection").clicked() {
                        app.selected.clear();
                    }
                    if ui.add_enabled(!app.busy && !app.comparing && !app.preview_analysis,
                        egui::Button::new(if app.selected.is_empty() {"Rotate frame · R"} else {"Rotate selected · R"}))
                        .on_hover_text("Rotate clockwise. Shift+R rotates counterclockwise. One Undo restores all affected frames.").clicked() {
                        app.actions.push(Action::Rotate);
                    }
                    if ui
                        .add_enabled(
                            !app.selected.is_empty(),
                            egui::Button::new("Adjust selected"),
                        )
                        .clicked()
                    {
                        app.scope_roll = false;
                        app.scope_selected = true;
                        app.panel = Panel::Adjust;
                    }
                });
            }
            conversion::review(app, ui, &r);
            if app.comparing {
                ui.colored_label(GOLD, "SAVED VERSION · release compare to return to current edits");
            }
            ui.separator();
            if r.frames.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.heading("A place for the whole roll");
                    muted(
                        ui,
                        "Choose all frames at once. RAW, or sRGB TIFF / JPEG / PNG.",
                    );
                    if primary(ui, "Choose scans…").clicked() {
                        app.actions.push(Action::Import);
                    }
                });
            } else if app.grid {
                grid(app, ui, &r);
            } else {
                inspector(app, ui, &r);
            }
        });
    dialogs(app, ctx);
}

fn right(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(20.0);
    ui.horizontal(|ui| {
        for (panel, label) in [
            (Panel::Adjust, "Adjust"),
            (Panel::Conversion, "Conversion"),
            (Panel::Dust, "Dust correction"),
        ] {
            if ui.selectable_label(app.panel == panel, label).clicked() {
                app.panel = panel;
                if panel == Panel::Dust {
                    app.grid = false;
                    app.scope_roll = false;
                    app.scope_selected = false;
                    app.tool = Tool::Heal;
                } else {
                    app.tool = Tool::View;
                }
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().show(ui,|ui|{match app.panel {
        Panel::Adjust=> adjustment_panel(app, ui),
        Panel::Conversion => conversion::panel(app, ui),
        Panel::Dust=>{
            if app.busy || app.comparing || app.preview_analysis { ui.disable(); }
            ui.label(RichText::new("Dust correction").size(18.0));muted(ui,"Only the spots you mark are changed.");ui.add_space(12.0);
            if ui.selectable_label(app.tool==Tool::Heal,"Click to heal").clicked(){app.tool=Tool::Heal;}
            let mut size=app.dust_radius*1000.0;
            if ui.add(egui::Slider::new(&mut size,2.0..=50.0).text("Brush size")).changed(){app.dust_radius=size/1000.0;}
            if ui.add_enabled(!app.busy,egui::Button::new("Load full-resolution detail")).clicked(){app.actions.push(Action::Detail);}
            muted(ui,"Click a dust spot. The connected circle is its source. Move the source if nearby texture is a better match.");
            let r=&mut app.library.rolls[app.roll];if let Some(f)=r.frames.get_mut(app.frame){
                ui.add_space(12.0);ui.label(format!("{} manual corrections",f.heals.len()));
                for i in 0..f.heals.len(){if ui.selectable_label(app.selected_heal==Some(i),format!("Spot {:02}",i+1)).clicked(){app.selected_heal=Some(i);}}
                if let Some(i)=app.selected_heal.filter(|i|*i<f.heals.len()){
                    if ui.button(if app.move_source{"Click the new source…"}else{"Move source"}).clicked(){app.move_source=true;app.tool=Tool::Heal;}
                    if ui.button("Remove this correction").clicked(){f.heals.remove(i);app.selected_heal=None;app.dirty=true;}
                }
                if ui.add_enabled(!f.heals.is_empty(),egui::Button::new("Undo last spot")).clicked(){f.heals.pop();app.selected_heal=None;app.dirty=true;}
            }
            ui.add_space(18.0);muted(ui,"There is no automatic dust removal. Hair, grain, and other image details stay untouched unless you mark them.");
        }
    }});
}
fn histogram_panel(app: &mut App, ui: &mut egui::Ui) {
    app.request_histogram(ui.ctx());
    ui.label(RichText::new("Histogram · RGB").strong());
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 90.), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 3., Color32::from_rgb(18, 20, 22));
    for fraction in [0.25, 0.5, 0.75] {
        let x = rect.left() + rect.width() * fraction;
        ui.painter().line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(1., Color32::from_gray(40)),
        );
    }
    if let Some(bins) = &app.histogram {
        let peak = bins.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
        for (channel, color) in bins.iter().zip([
            Color32::from_rgb(235, 100, 95),
            Color32::from_rgb(100, 215, 130),
            Color32::from_rgb(100, 155, 245),
        ]) {
            let points = channel
                .iter()
                .enumerate()
                .map(|(i, count)| {
                    egui::pos2(
                        rect.left() + i as f32 / 255. * rect.width(),
                        rect.bottom() - 2. - *count as f32 / peak * (rect.height() - 4.),
                    )
                })
                .collect();
            ui.painter()
                .add(egui::Shape::line(points, Stroke::new(1.2, color)));
        }
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if app.histogram_error.is_some() {
                "Histogram unavailable"
            } else if app.dragging {
                "Updates after slider release"
            } else if app.histogram_pending {
                "Updating…"
            } else {
                "Select a loaded frame"
            },
            egui::FontId::proportional(12.),
            Color32::GRAY,
        );
    }
    muted(ui, "Shadows ← RGB preview → Highlights");
    if let Some(error) = &app.histogram_error {
        muted(ui, error);
    }
    ui.add_space(10.);
}

fn adjustment_panel(app: &mut App, ui: &mut egui::Ui) {
    histogram_panel(app, ui);
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(app.scope_roll, "Whole roll").clicked() {
            app.scope_roll = true;
            app.scope_selected = false;
        }
        if ui
            .selectable_label(!app.scope_roll && !app.scope_selected, "This frame")
            .clicked()
        {
            app.scope_roll = false;
            app.scope_selected = false;
        }
        if ui
            .add_enabled(
                !app.selected.is_empty(),
                egui::Button::new("Selected").selected(app.scope_selected && !app.scope_roll),
            )
            .clicked()
        {
            app.scope_roll = false;
            app.scope_selected = true;
        }
    });
    let r = &app.library.rolls[app.roll];
    let active = r.frames.get(app.frame);
    let previous = if app.scope_roll {
        r.adjustments
    } else {
        active.map_or(Adjustments::default(), |f| f.adjustments)
    };
    let mut edits = previous;
    ui.colored_label(
        GOLD,
        if app.scope_roll {
            "Editing whole roll".into()
        } else if app.scope_selected {
            format!("Relative edits · {} selected", app.selected.len())
        } else {
            format!("Editing frame {:02}", app.frame + 1)
        },
    );
    let enabled = !app.busy
        && !app.comparing
        && !app.preview_analysis
        && (app.scope_roll || active.is_some())
        && (!app.scope_selected || !app.selected.is_empty());
    ui.add_enabled_ui(enabled, |ui| {
        let mut changed=false;
        egui::CollapsingHeader::new("Lighting").default_open(true).show(ui, |ui| {
            changed |= slider(ui,"Exposure (EV)",&mut edits.brightness,-3.0..=3.0);
            changed |= slider(ui,"Contrast",&mut edits.contrast,-50.0..=50.0);
            changed |= slider(ui,"Highlights",&mut edits.highlights,-100.0..=100.0);
            changed |= slider(ui,"Shadows",&mut edits.shadows,-100.0..=100.0);
            muted(ui,"Broad tonal adjustments; cannot restore clipped scan detail.");
        });
        egui::CollapsingHeader::new("Color balance").default_open(true).show(ui, |ui| {
            if ui.add_enabled(app.library.rolls[app.roll].material!=Material::BlackAndWhite,egui::Button::new("Pick neutral gray / white…")).clicked() {
                app.tool=Tool::Neutral;app.grid=false;app.show_negative=false;app.scope_selected=false;app.invalidate();
            }
            muted(ui,"Click a neutral patch in the positive. Changes warmth/tint in the roll or active frame; does not measure film base.");
            changed |= slider(ui,"Warmth",&mut edits.warmth,-50.0..=50.0);
            changed |= slider(ui,"Tint",&mut edits.tint,-50.0..=50.0);
            changed |= slider(ui,"Saturation",&mut edits.saturation,-100.0..=100.0);
        });
        ui.collapsing("Color mixer", |ui| {
            muted(ui,"Overlapping color bands · affects matching colors everywhere, not individual objects.");
            for (i,name) in film::COLOR_NAMES.iter().enumerate() {
                let colors=[(180,45,50),(230,141,54),(224,201,77),(45,118,73),(62,181,177),(62,94,176),(127,77,170),(174,61,142)];
                let (red,green,blue)=colors[i];
                let foreground=if matches!(i,1|2|4) {Color32::from_rgb(20,24,28)}else{Color32::WHITE};
                egui::Frame::new().fill(Color32::from_rgb(red,green,blue)).inner_margin(6.0).corner_radius(3.0).show(ui,|ui|{
                    ui.set_min_width((ui.available_width()-12.0).max(1.0));
                    ui.label(RichText::new(*name).color(foreground).strong());
                });
                ui.push_id(i, |ui| {
                    changed |= ui.add(egui::Slider::new(&mut edits.colors[i].hue,-30.0..=30.0).fixed_decimals(2).text("Hue")).changed();
                    changed |= ui.add(egui::Slider::new(&mut edits.colors[i].saturation,-100.0..=100.0).fixed_decimals(2).text("Saturation")).changed();
                    changed |= ui.add(egui::Slider::new(&mut edits.colors[i].lightness,-100.0..=100.0).fixed_decimals(2).text("Brightness")).changed();
                });
                ui.add_space(8.0);
            }
        });
        if changed { app.change_adjustments(previous,edits); }
        if ui.button("Reset adjustments in this scope").clicked() {app.actions.push(Action::Reset);}
        muted(ui,"Frame settings are offsets from the roll. Selected-frame sliders show the active frame; changes are added to each selected frame, preserving differences.");
        ui.collapsing("Copy / paste frame corrections", |ui| {
            if ui.button("Copy active frame · Ctrl+C").clicked() {app.actions.push(Action::CopyEdits);}
            ui.checkbox(&mut app.paste_options.lighting,"Lighting");
            ui.checkbox(&mut app.paste_options.color,"Color balance + mixer");
            ui.checkbox(&mut app.paste_options.composition,"Crop + rotation");
            ui.checkbox(&mut app.paste_options.dust,"Manual dust spots (same scan alignment only)");
            muted(ui,"Ctrl+V pastes to the selected frames, or the active frame. Paste replaces the chosen groups, without copying roll settings, rejection or review flags. Text fields keep normal copy/paste.");
            if ui.add_enabled(app.clipboard.is_some()&&!app.scope_roll,egui::Button::new(if app.scope_selected {"Paste to selected frames"} else {"Paste to this frame"})).clicked() {app.actions.push(Action::PasteEdits);}
        });
        if !app.library.rolls[app.roll].frames.is_empty() {
            ui.collapsing("Composition", |ui| {
                if ui.button("Crop this frame · C").clicked() {app.actions.push(Action::Crop);}
                if ui.button("Auto crop picture").clicked() {app.actions.push(Action::AutoCrop);}
                if ui.button("Reset crop").clicked() {app.actions.push(Action::ResetCrop);}
                if ui.button(if app.grid && !app.selected.is_empty() {"Rotate selected · R"} else {"Rotate clockwise · R"}).clicked() {app.actions.push(Action::Rotate);}
                if ui.selectable_label(app.library.rolls[app.roll].frames[app.frame].mirrored,"Invert orientation (mirror)").clicked() {app.actions.push(Action::Mirror);}
                if ui.button("Apply this orientation to whole roll").clicked() {app.actions.push(Action::OrientationToRoll);}
            });
        }
    });
    ui.separator();
    ui.collapsing("Saved versions / before & after", |ui| {
        ui.text_edit_singleline(&mut app.version_name);
        if ui.add_enabled(!app.busy&&!app.comparing,egui::Button::new("Save named version")).clicked() {app.actions.push(Action::SaveVersion);}
        let versions=&app.library.rolls[app.roll].versions;
        app.version_index=app.version_index.min(versions.len().saturating_sub(1));
        for (i,version) in versions.iter().enumerate() {
            if ui.selectable_value(&mut app.version_index,i,format!("{} · {}",i+1,version.name)).changed() {app.revision+=1;}
        }
        if !versions.is_empty() {
            app.compare_button = ui.button("Hold to compare saved version").is_pointer_button_down_on();
            muted(ui,"Or hold \\ outside text fields. Release to return to current edits.");
            if ui.add_enabled(!app.busy&&!app.comparing&&!app.library.rolls[app.roll].conversion_locked,egui::Button::new("Restore selected version")).clicked() {app.actions.push(Action::RestoreVersion);}
            muted(ui,"Restore keeps a safety version of your current edits. Frames added later remain in the roll.");
        }
    });
}

fn slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.label(RichText::new(label).size(13.0));
    let r = ui.add(
        egui::Slider::new(value, range)
            .fixed_decimals(2)
            .clamping(egui::SliderClamping::Always),
    );
    ui.add_space(4.0);
    r.changed()
}

fn grid(app: &mut App, ui: &mut egui::Ui, roll: &Roll) {
    let columns = ((ui.available_width() / 235.0).floor() as usize).max(1);
    let width = (ui.available_width() - 14.0 * (columns - 1) as f32) / columns as f32;
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("contact-sheet")
            .num_columns(columns)
            .spacing([14.0, 14.0])
            .show(ui, |ui| {
                for (i, frame) in roll.frames.iter().enumerate() {
                    let selected = app.frame == i || app.selected.contains(&frame.id);
                    egui::Frame::new()
                        .fill(if selected {
                            Color32::from_rgb(48, 45, 39)
                        } else {
                            PANEL
                        })
                        .stroke(Stroke::new(
                            1.0,
                            if selected {
                                GOLD
                            } else {
                                Color32::from_rgb(52, 54, 56)
                            },
                        ))
                        .inner_margin(8.0)
                        .corner_radius(5.0)
                        .show(ui, |ui| {
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.set_width(width - 16.0);
                                let size = Vec2::new(width - 16.0, 174.0);
                                let (rect, response) =
                                    ui.allocate_exact_size(size, egui::Sense::click());
                                ui.painter()
                                    .rect_filled(rect, 0.0, Color32::from_rgb(19, 21, 23));
                                if let Some((gpu, settings, natural)) = gpu_settings(app, &frame.id)
                                {
                                    let image_rect = egui::Rect::from_center_size(
                                        rect.center(),
                                        fit(natural, size),
                                    );
                                    ui.painter().add(greign_gl_preview::Preview::callback(
                                        gpu, image_rect, settings,
                                    ));
                                } else if let Some(t) = app.textures.get(&frame.id) {
                                    let image_rect = egui::Rect::from_center_size(
                                        rect.center(),
                                        fit(t.size_vec2(), size),
                                    );
                                    ui.painter().image(
                                        t.id(),
                                        image_rect,
                                        egui::Rect::from_min_max(
                                            egui::Pos2::ZERO,
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );
                                } else {
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        if frame.error.is_some() {
                                            "Needs attention"
                                        } else {
                                            "Loading…"
                                        },
                                        egui::FontId::proportional(13.0),
                                        MUTED,
                                    );
                                }
                                if response.clicked() {
                                    let modifiers = ui.input(|input| input.modifiers);
                                    app.select_frame(i, modifiers.command, modifiers.shift);
                                }
                                if response.double_clicked() {
                                    app.frame = i;
                                    app.grid = false;
                                    app.scope_roll = false;
                                    app.scope_selected = false;
                                }
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{:02}", i + 1)).color(if selected {
                                            GOLD
                                        } else {
                                            MUTED
                                        }),
                                    );
                                    let label = frame.name.trim_end_matches("-after.jpg");
                                    ui.label(RichText::new(label).size(12.0));
                                    if frame.rejected {
                                        ui.label(
                                            RichText::new("REJECT")
                                                .size(10.0)
                                                .color(Color32::LIGHT_RED),
                                        );
                                    } else if frame.adjustments != Adjustments::default()
                                        || !frame.heals.is_empty()
                                    {
                                        ui.label(RichText::new("EDITED").size(10.0).color(GOLD));
                                    }
                                });
                                if let Some(e) = &frame.error {
                                    ui.label(RichText::new(e).size(11.0).color(Color32::LIGHT_RED));
                                }
                                if let Some(warning) = &frame.scan_warning {
                                    ui.label(RichText::new("SCAN WARNING").color(GOLD).size(11.0))
                                        .on_hover_text(warning);
                                }
                                let mut review = frame.review;
                                if ui
                                    .add_enabled(
                                        !app.busy && !app.comparing,
                                        egui::Checkbox::new(&mut review, "Needs my review"),
                                    )
                                    .changed()
                                {
                                    app.library.rolls[app.roll].frames[i].review = review;
                                    app.dirty = true;
                                }
                            });
                        });
                    if (i + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    });
}

fn inspector(app: &mut App, ui: &mut egui::Ui, roll: &Roll) {
    let Some(frame) = roll.frames.get(app.frame) else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "{:02} / {:02}  ·  {}",
            app.frame + 1,
            roll.frames.len(),
            frame.name
        ));
        if ui
            .add_enabled(
                !app.busy
                    && !app.comparing
                    && !app.preview_analysis
                    && !frame.rejected
                    && (roll.analysis_for(frame).is_some() || roll.material == Material::Positive),
                egui::Button::new("Export frame…"),
            )
            .clicked()
        {
            app.export_scope = super::ExportScope::Frame;
            app.export_dialog = true;
        }
        if ui
            .button(if frame.rejected {
                "Keep frame"
            } else {
                "Reject"
            })
            .clicked()
        {
            app.actions.push(Action::Reject);
        }
        if ui
            .button(if app.reference.as_deref() == Some(&frame.id) {
                "Unpin reference"
            } else {
                "Pin reference"
            })
            .clicked()
        {
            app.reference = if app.reference.as_deref() == Some(&frame.id) {
                None
            } else {
                Some(frame.id.clone())
            };
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(!app.show_negative, "Positive")
            .clicked()
        {
            app.show_negative = false;
            app.tool = Tool::View;
            app.drag_start = None;
            app.invalidate();
        }
        if ui
            .selectable_label(app.show_negative, "Original negative")
            .clicked()
        {
            app.show_negative = true;
            app.tool = Tool::View;
            app.drag_start = None;
            app.invalidate();
        }
        let editable = !app.busy && !app.comparing && !app.preview_analysis;
        if ui.add_enabled(editable, egui::Button::new("Invert orientation").selected(frame.mirrored)).on_hover_text("Mirror the positive left-to-right for a scan taken from the opposite side. Original inspection remains as captured.").clicked() {app.actions.push(Action::Mirror);}
        if ui
            .add_enabled(editable, egui::Button::new("Crop · C"))
            .clicked()
        {
            app.actions.push(Action::Crop);
        }
        if ui
            .add_enabled(editable, egui::Button::new("Auto crop"))
            .on_hover_text("Find the picture inside the film base and straighten slight tilt")
            .clicked()
        {
            app.actions.push(Action::AutoCrop);
        }
        if ui
            .add_enabled(
                editable && frame.crop != Rect::default(),
                egui::Button::new("Reset crop"),
            )
            .clicked()
        {
            app.actions.push(Action::ResetCrop);
        }
        if ui.selectable_label(!app.zoom, "Fit").clicked() {
            app.zoom = false;
            app.view.fit();
        }
        if ui
            .selectable_label(
                app.view.percent.is_some_and(|p| (p - 100.).abs() < 0.5),
                "100% detail",
            )
            .clicked()
        {
            app.actions.push(Action::Detail);
        }
        if app.zoom {
            if let Some(percent) = app.view.percent {
                muted(ui, format!("{percent:.0}%"));
            }
        }
        match app.tool {
            Tool::Neutral => {
                ui.colored_label(GOLD, "Click a neutral gray / white patch");
            }
            Tool::Heal => {
                ui.colored_label(
                    GOLD,
                    if app.move_source {
                        "Click a new source"
                    } else {
                        "Click to heal"
                    },
                );
            }
            Tool::Crop => {
                ui.colored_label(GOLD, "Drag a crop on the original · Esc cancels");
            }
            Tool::Base => {
                ui.colored_label(GOLD, if app.library.rolls[app.roll].optimize_individually { format!("Drag clean, unexposed film outside the picture · {} only",frame.name) } else { "Drag a clean film-base patch · shared by this roll".into() });
            }
            Tool::View => {}
        }
    });
    if let Some(reference) = app.reference.clone().filter(|id| id != &frame.id) {
        ui.columns(2, |cols| {
            if let Some(t) = app.textures.get(&reference) {
                cols[0].label("PINNED REFERENCE");
                let size = fit(
                    t.size_vec2(),
                    Vec2::new(cols[0].available_width(), cols[0].available_height() - 24.0),
                );
                cols[0].image((t.id(), size));
            }
            canvas(app, &mut cols[1], frame);
        });
    } else {
        canvas(app, ui, frame);
    }
}
fn fit(size: Vec2, available: Vec2) -> Vec2 {
    size * (available.x.max(1.0) / size.x).min(available.y.max(1.0) / size.y)
}
#[allow(clippy::many_single_char_names)]
fn view_to_source(p: [f32; 2], crop: Rect, rotation: u8, mirrored: bool, aspect: f32) -> [f32; 2] {
    let [mut u, v] = p;
    if mirrored {
        u = 1. - u;
    }
    let [x, y] = match rotation % 4 {
        1 => [v, 1.0 - u],
        2 => [1.0 - u, 1.0 - v],
        3 => [1.0 - v, u],
        _ => [u, v],
    };
    crop.source_point([x, y], aspect)
}
fn source_to_view(p: [f32; 2], crop: Rect, rotation: u8, mirrored: bool, aspect: f32) -> [f32; 2] {
    let [x, y] = crop.crop_point(p, aspect);
    let mut point = match rotation % 4 {
        1 => [1.0 - y, x],
        2 => [1.0 - x, 1.0 - y],
        3 => [y, 1.0 - x],
        _ => [x, y],
    };
    if mirrored {
        point[0] = 1. - point[0];
    }
    point
}

type GpuSettings = (
    std::sync::Arc<std::sync::Mutex<greign_gl_preview::Preview>>,
    greign_gl_preview::Settings,
    Vec2,
);
fn gpu_settings(app: &App, id: &str) -> Option<GpuSettings> {
    let gpu = app.gpu.as_ref()?;
    let roll = app.library.rolls.get(app.roll)?;
    if roll.frames.get(app.frame)?.id != id || (app.show_negative && !app.comparing) {
        return None;
    }
    let version = if app.comparing {
        roll.versions.get(app.version_index)
    } else {
        None
    };
    let frame = version
        .and_then(|v| v.state.frames.iter().find(|f| f.id == id))
        .or_else(|| roll.frames.iter().find(|f| f.id == id))?;
    if !frame.heals.is_empty()
        || (roll.material != Material::Positive
            && app
                .displayed_conversion(roll)
                .and_then(|a| a.for_frame(id))
                .is_none())
    {
        return None;
    }
    let resource = gpu.lock().unwrap();
    if app.full_desired.as_ref() != Some(&resource.key)
        || !resource.key.starts_with(&format!("{id}:"))
    {
        return None;
    }
    let edits = app.displayed_adjustments(roll, frame);
    let mut size = Vec2::new(
        (resource.width as f32 * frame.crop.w).floor().max(1.),
        (resource.height as f32 * frame.crop.h).floor().max(1.),
    );
    if frame.rotation % 2 == 1 {
        size = Vec2::new(size.y, size.x);
    }
    Some((
        gpu.clone(),
        greign_gl_preview::Settings {
            mirrored: frame.mirrored,
            edits,
            material: roll.material,
            crop: frame.crop,
            rotation: frame.rotation,
        },
        size,
    ))
}

fn canvas(app: &mut App, ui: &mut egui::Ui, frame: &super::model::Frame) {
    let gpu = gpu_settings(app, &frame.id);
    let roll = &app.library.rolls[app.roll];
    let saved = if app.comparing {
        roll.versions.get(app.version_index)
    } else {
        None
    };
    let frame = saved
        .and_then(|v| v.state.frames.iter().find(|f| f.id == frame.id))
        .unwrap_or(frame)
        .clone();
    let negative = (!app.comparing && app.show_negative)
        || (app
            .displayed_conversion(roll)
            .and_then(|a| a.for_frame(&frame.id))
            .is_none()
            && roll.material != Material::Positive);
    let detail = app.detail_texture.as_ref().filter(|(id, key, _)| {
        *id == frame.id && app.full_desired.as_ref() == Some(key) && !app.dragging
    });
    let full_size = gpu
        .as_ref()
        .map(|(_, _, size)| *size)
        .or_else(|| detail.map(|(_, _, t)| t.size_vec2()));
    let Some(texture) = detail
        .map(|(_, _, t)| t.clone())
        .or_else(|| app.textures.get(&frame.id).cloned())
    else {
        ui.centered_and_justified(|ui| {
            ui.spinner();
        });
        return;
    };
    ui.scope(|ui| {
        let (viewport, response) = ui.allocate_exact_size(
            Vec2::new(
                ui.available_width().max(1.0),
                (ui.available_height() - 12.0).max(1.0),
            ),
            egui::Sense::click_and_drag(),
        );
        ui.set_clip_rect(viewport.intersect(ui.clip_rect()));
        let key = serde_json::to_string(&(
            &frame.id,
            frame.crop,
            frame.rotation,
            frame.mirrored,
            negative,
        ))
        .unwrap();
        if app.view.key != key {
            app.view.fit();
            app.view.key = key;
            app.zoom = false;
        }
        // Use stable scan aspect, not the current low/full-resolution texture.
        let natural = app
            .cache
            .get(&frame.id)
            .map_or(texture.size_vec2(), |scan| {
                let mut size = Vec2::new(scan.image.width as f32, scan.image.height as f32);
                if !negative {
                    size = Vec2::new(size.x * frame.crop.w, size.y * frame.crop.h);
                    if frame.rotation % 2 == 1 {
                        size = Vec2::new(size.y, size.x);
                    }
                }
                size
            });
        let fitted = fit(natural, viewport.size());
        if app.view.native_pending {
            if let Some(size) = full_size {
                app.view.scale = (size.x / ui.ctx().pixels_per_point() / fitted.x).clamp(1.0, 32.0);
                app.view.native_pending = false;
            }
        }
        if response.hovered() {
            let (factor, pointer, scroll, command) = ui.input(|i| {
                (
                    i.zoom_delta(),
                    i.pointer.hover_pos(),
                    i.smooth_scroll_delta,
                    i.modifiers.command || i.modifiers.ctrl || i.modifiers.mac_cmd,
                )
            });
            if let Some(pointer) = pointer {
                if (factor - 1.0).abs() > 0.00001 {
                    app.view.zoom_at(factor, pointer, viewport, fitted);
                    app.zoom = app.view.scale > 1.0;
                    app.refine = true;
                    app.scheduled = None;
                }
            }
            if !command {
                app.view.pan += scroll;
            }
        }
        if response.dragged_by(egui::PointerButton::Middle)
            || (app.tool == Tool::View && response.dragged_by(egui::PointerButton::Primary))
        {
            app.view.pan += ui.input(|i| i.pointer.delta());
        }
        app.view.clamp(viewport, fitted);
        app.view.percent = full_size
            .map(|size| app.view.scale * fitted.x * ui.ctx().pixels_per_point() / size.x * 100.);
        let rect = app.view.rect(viewport, fitted);
        if let Some((gpu, settings, _)) = &gpu {
            ui.painter().add(greign_gl_preview::Preview::callback(
                gpu.clone(),
                rect,
                *settings,
            ));
        } else {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let aspect = app.cache.get(&frame.id).map_or(1., |scan| {
            scan.image.width as f32 / scan.image.height as f32
        });
        let source_crop = if negative {
            Rect::default()
        } else {
            frame.crop
        };
        let source_rotation = if negative { 0 } else { frame.rotation };
        let to_source = |pos: egui::Pos2| {
            view_to_source(
                [
                    ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
                    ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0),
                ],
                source_crop,
                source_rotation,
                !negative && frame.mirrored,
                aspect,
            )
        };
        if app.tool == Tool::Crop && negative && frame.crop != Rect::default() {
            let corners = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| {
                let p = frame.crop.source_point(p, aspect);
                egui::pos2(
                    rect.left() + p[0] * rect.width(),
                    rect.top() + p[1] * rect.height(),
                )
            });
            for i in 0..4 {
                ui.painter()
                    .line_segment([corners[i], corners[(i + 1) % 4]], Stroke::new(1.5, GOLD));
            }
        }
        if app.tool == Tool::Neutral
            && !negative
            && !app.busy
            && !app.comparing
            && !app.preview_analysis
            && response.clicked()
        {
            if let Some(pos) = response
                .interact_pointer_pos()
                .filter(|p| rect.contains(*p))
            {
                app.pick_neutral(to_source(pos), ui.ctx());
            }
        }
        if app.tool == Tool::Heal
            && !app.show_negative
            && !app.comparing
            && !app.preview_analysis
            && !app.busy
        {
            for (i, heal) in frame.heals.iter().enumerate() {
                let point = source_to_view(
                    heal.target,
                    frame.crop,
                    frame.rotation,
                    frame.mirrored,
                    aspect,
                );
                let pos = egui::pos2(
                    rect.left() + point[0] * rect.width(),
                    rect.top() + point[1] * rect.height(),
                );
                let r =
                    heal.radius * rect.width().min(rect.height()) / frame.crop.w.min(frame.crop.h);
                ui.painter().circle_stroke(pos, r, Stroke::new(1.2, GOLD));
                if app.selected_heal == Some(i) {
                    let point = source_to_view(
                        heal.source,
                        frame.crop,
                        frame.rotation,
                        frame.mirrored,
                        aspect,
                    );
                    let source = egui::pos2(
                        rect.left() + point[0] * rect.width(),
                        rect.top() + point[1] * rect.height(),
                    );
                    ui.painter()
                        .circle_stroke(source, r, Stroke::new(1.2, Color32::LIGHT_BLUE));
                    ui.painter()
                        .line_segment([pos, source], Stroke::new(1.0, GOLD));
                }
            }
            if response.clicked() {
                if let Some(pos) = response
                    .interact_pointer_pos()
                    .filter(|p| rect.contains(*p))
                {
                    let target = to_source(pos);
                    let f = &mut app.library.rolls[app.roll].frames[app.frame];
                    if app.move_source {
                        if let Some(i) = app.selected_heal.filter(|i| *i < f.heals.len()) {
                            f.heals[i].source = target;
                            app.dirty = true;
                        }
                        app.move_source = false;
                    } else {
                        f.heals.push(Heal {
                            target,
                            source: film::suggest_source(
                                target,
                                app.dust_radius,
                                texture.aspect_ratio(),
                            ),
                            radius: app.dust_radius,
                        });
                        app.selected_heal = Some(f.heals.len() - 1);
                        app.dirty = true;
                    }
                }
            }
        }
        if (app.tool == Tool::Crop || app.tool == Tool::Base)
            && !app.comparing
            && !app.preview_analysis
            && !app.busy
            && (app.tool != Tool::Base || !app.library.rolls[app.roll].conversion_locked)
        {
            if response.drag_started() {
                app.drag_start = ui
                    .input(|i| i.pointer.press_origin())
                    .filter(|p| rect.contains(*p));
            }
            if let (Some(start), Some(end)) = (app.drag_start, response.interact_pointer_pos()) {
                ui.painter().rect_stroke(
                    egui::Rect::from_two_pos(start, end),
                    0.0,
                    Stroke::new(1.5, GOLD),
                    egui::StrokeKind::Inside,
                );
                if response.drag_stopped() {
                    let a = to_source(start);
                    let b = to_source(end);
                    let region = Rect {
                        x: a[0].min(b[0]),
                        y: a[1].min(b[1]),
                        w: (a[0] - b[0]).abs(),
                        h: (a[1] - b[1]).abs(),
                        angle: 0.0,
                    };
                    if region.valid() && region.w > 0.005 && region.h > 0.005 {
                        if app.tool == Tool::Crop {
                            app.library.rolls[app.roll].frames[app.frame].crop = region;
                            app.dirty = true;
                            app.status = "Crop applied · Undo restores the previous crop".into();
                        } else if let Some(scan) = app.cache.get(&frame.id) {
                            match film::measure_base(scan, region) {
                                Ok(base) => {
                                    app.actions.push(Action::SetBase {
                                        frame: app.library.rolls[app.roll]
                                            .optimize_individually
                                            .then_some(app.frame),
                                        base: Some(base),
                                    });
                                }
                                Err(e) => app.status = e.to_string(),
                            }
                        }
                    }
                    if app.tool == Tool::Crop {
                        app.show_negative = app.crop_return_negative;
                        app.invalidate();
                    }
                    app.drag_start = None;
                    app.tool = Tool::View;
                }
            }
        }
    });
}

fn dialogs(app: &mut App, ctx: &egui::Context) {
    if app.new_roll_open {
        let mut open = true;
        let mut create = false;
        egui::Window::new("Import a roll").open(&mut open).collapsible(false).resizable(false).default_width(460.0).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.label("Choose the folder containing this roll's scans.");
            ui.horizontal(|ui| {
                if primary(ui, "Choose roll folder…").clicked() {
                    if let Some(folder) = rfd::FileDialog::new().set_title("Choose a roll folder").pick_folder() {
                        app.choose_import_folder(folder);
                    }
                }
                if ui.button("Choose individual files…").clicked() {
                    if let Some(mut paths) = rfd::FileDialog::new().add_filter("Scans", &["cr3", "CR3", "cr2", "nef", "arw", "dng", "orf", "rw2", "raf", "tif", "tiff", "jpg", "jpeg", "png"]).pick_files() {
                        paths.retain(|p| p.is_file() && super::model::supported(p));
                        paths.sort(); paths.dedup();
                        if app.new_name.trim().is_empty() {
                            app.new_name = paths.first().and_then(|p| p.parent()).map(super::workflow::folder_name).unwrap_or_default();
                        }
                        app.new_paths = paths;
                        app.new_folder = None;
                        app.new_import_error = None;
                    }
                }
            });
            if let Some(folder) = &app.new_folder { ui.label(folder.display().to_string()); }
            ui.label(format!("{} scans selected", app.new_paths.len()));
            if let Some(error) = &app.new_import_error { ui.colored_label(Color32::LIGHT_RED, error); }
            ui.add_space(8.0);
            ui.label("Roll name"); ui.text_edit_singleline(&mut app.new_name);
            ui.label("Film stock (optional)"); ui.text_edit_singleline(&mut app.new_stock);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.new_material, Material::ColorNegative, "Color negative");
                ui.selectable_value(&mut app.new_material, Material::BlackAndWhite, "B&W");
                ui.selectable_value(&mut app.new_material, Material::Positive, "Positive");
            });
            muted(ui, "Imports image files directly inside the folder. Subfolders are separate rolls. Originals stay in place.");
            create = ui.add_enabled(!app.busy && !app.new_paths.is_empty() && app.new_import_error.is_none(), egui::Button::new(RichText::new("Import roll").color(BG)).fill(GOLD)).clicked();
        });
        app.new_roll_open = open;
        if create {
            app.import_new_roll(ctx);
        }
    }
    if app.metadata_open {
        let mut open = true;
        egui::Window::new("Roll metadata")
            .open(&mut open)
            .default_width(430.0)
            .show(ctx, |ui| {
                let r = &mut app.library.rolls[app.roll];
                for (label, value) in [
                    ("Roll name", &mut r.name),
                    ("Film stock", &mut r.stock),
                    ("Photographed with (film camera)", &mut r.film_camera),
                    ("Date photographed", &mut r.date),
                ] {
                    ui.label(label);
                    app.dirty |= ui.text_edit_singleline(value).changed();
                }
                ui.label("Development / push-pull / notes");
                muted(ui,"Date format: YYYY-MM-DD, optionally HH:MM:SS. Date-only uses noon; no time zone is invented. This is written as the original capture date in new JPEG/TIFF/DNG exports, not the scan date. Frames share the roll date.");
                if let Err(error)=greign_export::CaptureDate::parse(&r.date) {ui.colored_label(Color32::LIGHT_RED,error);}
                app.dirty |= ui.text_edit_multiline(&mut r.notes).changed();
                if let Some(f) = r.frames.first() {
                    if let Some(scan) = app.cache.get(&f.id) {
                        muted(ui, format!("Scanned with: {}", scan.capture));
                    }
                }
            });
        app.metadata_open = open;
    }
    if app.export_dialog && !app.library.rolls.is_empty() {
        export_dialog(app, ctx);
    }
    if !app.export_retry.is_empty() {
        let mut retry_now = false;
        egui::Window::new("Export needs attention").show(ctx, |ui| {
            for retry in &app.export_retry {
                ui.label(format!("{} · {} failed frames", retry.roll.name, retry.ids.len()));
                for frame in &retry.roll.frames { if retry.ids.contains(&frame.id) { ui.label(&frame.name); } }
            }
            muted(ui, "Successful exports are saved. Restore missing files or resolve the destination problem, then retry with the original frozen settings.");
            retry_now = ui.add_enabled(!app.busy, egui::Button::new("Retry failed frames only")).clicked();
        });
        if retry_now {
            app.actions.push(Action::RetryExport);
        }
    }
}

fn export_dialog(app: &mut App, ctx: &egui::Context) {
    use super::ExportScope;
    let mut open = true;
    let mut export = false;
    let mut defaults = None;
    let batch = app.export_scope == ExportScope::AllReady;
    egui::Window::new(if batch { "Export ready rolls" } else { "Export frames" }).open(&mut open).collapsible(false).default_size([520.0, 600.0]).max_height((ctx.screen_rect().height() - 100.0).max(300.0)).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).vscroll(true).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let (count, ready) = if batch {
            let readiness: Vec<_> = app.library.rolls.iter().map(|roll| super::workflow::export_ready(roll, None)).collect();
            let count: usize = readiness.iter().filter_map(|r| r.as_ref().ok()).sum();
            let rolls = readiness.iter().filter(|r| r.is_ok()).count();
            egui::CollapsingHeader::new("Rolls in this batch").default_open(true).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("batch-roll-list").max_height(120.0).show(ui, |ui| {
                for (roll, result) in app.library.rolls.iter().zip(&readiness) {
                    match result {
                        Ok(frames) => { ui.label(format!("{} · {frames} frames", roll.name)); }
                        Err(error) => { muted(ui, format!("Skipped: {} · {error}", roll.name)); }
                    }
                }
            });
            });
            ui.label(format!("{rolls} ready rolls · {count} kept frames · {} skipped", app.library.rolls.len() - rolls));
            (count, rolls > 0)
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut app.export_scope, ExportScope::Roll, "Whole roll");
                ui.selectable_value(&mut app.export_scope, ExportScope::Selected, "Selected frames");
                ui.selectable_value(&mut app.export_scope, ExportScope::Frame, "Active frame");
            });
            let roll = &app.library.rolls[app.roll];
            let only = match app.export_scope {
                ExportScope::Selected => Some(app.selected.clone()),
                ExportScope::Frame => Some(roll.frames.get(app.frame).map(|f| f.id.clone()).into_iter().collect()),
                _ => None,
            };
            match super::workflow::export_ready(roll, only.as_ref()) {
                Ok(count) => { ui.label(format!("{count} kept frames to export")); (count, true) }
                Err(error) => { ui.colored_label(Color32::LIGHT_RED, error.to_string()); (0, false) }
            }
        };
        let example_roll = if batch {
            app.library.rolls.iter().find(|roll| super::workflow::export_ready(roll, None).is_ok()).unwrap_or(&app.library.rolls[app.roll])
        } else { &app.library.rolls[app.roll] };
        let roll_name = example_roll.name.clone();
        let preset = if batch { &mut app.library.export_defaults } else { &mut app.library.rolls[app.roll].export };
        ui.separator();
        ui.label("Master export folder");
        let destination = preset.destination.as_ref().unwrap_or(&app.export_dir);
        ui.label(destination.display().to_string());
        if ui.button("Choose master folder…").clicked() {
            if let Some(path) = rfd::FileDialog::new().set_directory(destination).pick_folder() { preset.destination = Some(path); app.dirty = true; }
        }
        ui.label("Roll folder suffix");
        app.dirty |= ui.text_edit_singleline(&mut preset.folder_suffix).changed();
        let destination = preset.destination.as_ref().unwrap_or(&app.export_dir);
        let folder = super::workflow::export_folder_name(&roll_name, &preset.folder_suffix);
        ui.label(format!("{}: {}", if batch { "Example" } else { "Saves to" }, destination.join(folder).display()));
        muted(ui, "Each roll gets its own folder. Existing folders are preserved; repeats add (2), (3), …");
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Full JPEG · quality 95").clicked() { preset.dng = false; preset.tiff = false; preset.long_edge = 0; preset.jpeg_quality = 95; app.dirty = true; }
            if ui.button("Full TIFF").clicked() { preset.dng = false; preset.tiff = true; preset.long_edge = 0; app.dirty = true; }
            if ui.button("Share JPEG · 2048 px").clicked() { preset.dng = false; preset.tiff = false; preset.long_edge = 2048; preset.jpeg_quality = 90; app.dirty = true; }
        });
        let format = if preset.dng { "DNG" } else if preset.tiff { "TIFF" } else { "JPEG" };
        let size = if preset.dng || preset.long_edge == 0 { "Full resolution".into() } else { format!("{} px longest edge", preset.long_edge) };
        let quality = if preset.dng || preset.tiff { String::new() } else { format!(" · quality {}", preset.jpeg_quality) };
        ui.label(format!("{format} · {size}{quality}"));
        egui::CollapsingHeader::new("More export options").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(!preset.dng && !preset.tiff, "JPEG").clicked() { preset.dng = false; preset.tiff = false; app.dirty = true; }
            if ui.selectable_label(!preset.dng && preset.tiff, "TIFF · 16-bit ZIP").clicked() { preset.dng = false; preset.tiff = true; app.dirty = true; }
            if ui.selectable_label(preset.dng, "DNG · linear 32-bit float").clicked() { preset.dng = true; preset.tiff = false; app.dirty = true; }
        });
        if preset.dng { muted(ui, "Full-resolution edited positive in linear Rec.2020. Edits are baked in. Large uncompressed files; other editors may apply a different initial look. Lightroom import still needs validation."); }
        if !preset.dng && !preset.tiff { app.dirty |= ui.add(egui::Slider::new(&mut preset.jpeg_quality, 1..=100).text("JPEG quality")).changed(); }
        ui.horizontal(|ui| {
            ui.label("Longest edge (0 = full resolution)");
            app.dirty |= ui.add_enabled(!preset.dng, egui::DragValue::new(&mut preset.long_edge).range(0..=32000)).changed();
        });
        ui.label("Filename prefix (optional)"); app.dirty |= ui.text_edit_singleline(&mut preset.prefix).changed();
        });
        if ui.button("Use these settings for every roll").clicked() {
            let mut saved = preset.clone();
            // Remember even the platform's default destination for one-click export.
            if saved.destination.is_none() { saved.destination = Some(app.export_dir.clone()); }
            defaults = Some(saved);
        }
        muted(ui, "Applies to existing and future rolls. Export roll then uses these settings in one click.");
        if !batch {
            let r = &mut app.library.rolls[app.roll];
            egui::CollapsingHeader::new("Presets for this roll").show(ui, |ui| {
                for preset in r.export_presets.clone() { if ui.button(format!("Use {}", preset.name)).clicked() { r.export = preset; app.dirty = true; } }
                ui.label("Preset name"); app.dirty |= ui.text_edit_singleline(&mut r.export.name).changed();
                if ui.add_enabled(!r.export.name.trim().is_empty(), egui::Button::new("Save / update preset for this roll")).clicked() {
                    r.export_presets.retain(|p| p.name != r.export.name); r.export_presets.push(r.export.clone()); app.dirty = true;
                }
            });
        }
        ui.separator();
        export = ui.add_enabled(ready && count > 0 && !app.busy && !app.preview_analysis && !app.comparing, egui::Button::new(RichText::new(if batch { "Export all ready rolls" } else { "Export kept frames" }).color(BG)).fill(GOLD)).clicked();
    });
    if let Some(preset) = defaults {
        app.set_export_defaults(preset);
    }
    app.export_dialog = open;
    if export {
        app.export_dialog = false;
        app.actions.push(Action::Export);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_import_and_batch_export_dialogs_show_destinations_and_skipped_rolls() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(
            &ctx,
            directory.path().into(),
            super::super::model::Library::default(),
            None,
        );
        let mut roll = Roll::new("Kyoto 2026");
        roll.material = Material::Positive;
        roll.frames
            .push(super::super::model::Frame::new("scan.png".into()));
        app.library.rolls = vec![roll, Roll::new("Unconverted")];
        app.new_roll_open = true;
        let render = |app: &mut App, width, height| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    ..Default::default()
                },
                |ctx| draw(app, ctx),
            )
        };
        let labels = |output: &egui::FullOutput| -> Vec<String> {
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect()
        };
        for (width, height) in [(1080., 720.), (1440., 960.)] {
            // egui settles window sizing after the first frame.
            render(&mut app, width, height);
            let texts = labels(&render(&mut app, width, height));
            assert!(texts.iter().any(|s| s == "Choose roll folder…"));
            assert!(texts.iter().any(|s| s == "Import roll"));
        }
        app.new_roll_open = false;
        app.roll = 1; // The batch example must name a roll that will actually be exported.
        app.export_scope = super::super::ExportScope::AllReady;
        app.export_dialog = true;
        for (width, height) in [(1080., 720.), (1440., 960.)] {
            render(&mut app, width, height);
            let texts = labels(&render(&mut app, width, height));
            for label in [
                "Master export folder",
                "Roll folder suffix",
                "Use these settings for every roll",
                "Export all ready rolls",
            ] {
                assert!(
                    texts.iter().any(|s| s == label),
                    "Missing {label}: {texts:?}"
                );
            }
            assert!(texts.iter().any(|s| s.contains("Kyoto 2026 - converted")));
            assert!(texts.iter().any(|s| s.contains("Skipped: Unconverted")));
        }
    }

    #[test]
    fn crop_drag_can_expand_an_existing_rotated_crop_on_the_full_original() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(
            &ctx,
            dir.path().to_owned(),
            super::super::model::Library::default(),
            None,
        );
        let mut roll = Roll::new("Crop");
        let mut frame = super::super::model::Frame::new("scan.png".into());
        frame.crop = Rect {
            x: 0.4,
            y: 0.4,
            w: 0.2,
            h: 0.2,
            angle: 2.5,
        };
        frame.rotation = 1;
        frame.mirrored = true;
        let texture = ctx.load_texture(
            "crop-fixture",
            egui::ColorImage::new([300, 200], Color32::GRAY),
            egui::TextureOptions::LINEAR,
        );
        let texture_id = texture.id();
        app.textures.insert(frame.id.clone(), texture);
        roll.frames.push(frame);
        app.library.rolls.push(roll);
        app.apply_action(Action::Crop, &ctx);
        app.apply_action(Action::Crop, &ctx);
        assert!(app.show_negative);
        let run = |app: &mut App, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 700.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    let frame = app.library.rolls[0].frames[0].clone();
                    egui::CentralPanel::default().show(ctx, |ui| canvas(app, ui, &frame));
                },
            )
        };
        let output = run(&mut app, vec![]);
        let rect = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id => {
                    Some(mesh.calc_bounds())
                }
                _ => None,
            })
            .unwrap();
        let a = rect.min + rect.size() * 0.2;
        let b = rect.min + rect.size() * 0.8;
        run(
            &mut app,
            vec![
                egui::Event::PointerMoved(a),
                egui::Event::PointerButton {
                    pos: a,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
        run(&mut app, vec![egui::Event::PointerMoved(b)]);
        run(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: b,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
        );
        let frame = &app.library.rolls[0].frames[0];
        assert!((frame.crop.x - 0.2).abs() < 0.001);
        assert!((frame.crop.y - 0.2).abs() < 0.001);
        assert!((frame.crop.w - 0.6).abs() < 0.001);
        assert!((frame.crop.h - 0.6).abs() < 0.001);
        assert_eq!(frame.rotation, 1);
        assert!(frame.crop.angle.abs() < f32::EPSILON);
        assert!(frame.mirrored);
        assert!(!app.show_negative);
        assert!(app.dirty && app.drag_start.is_none() && app.tool == Tool::View);
    }

    #[test]
    fn opening_mixer_exposes_all_eight_color_groups() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(
            &ctx,
            dir.path().to_owned(),
            super::super::model::Library::default(),
            None,
        );
        let mut roll = Roll::new("Mixer");
        roll.frames
            .push(super::super::model::Frame::new("one.png".into()));
        app.library.rolls.push(roll);
        let run = |app: &mut App, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440., 4000.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| draw(app, ctx),
            )
        };
        let output = run(&mut app, vec![]);
        let text_y = |label: &str| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => Some(text.pos.y),
                _ => None,
            })
        };
        assert!(text_y("Histogram · RGB").unwrap() < text_y("Exposure (EV)").unwrap());
        assert!(text_y("Clipping overlay").is_none());
        let position = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    (text.galley.text() == "Color mixer")
                        .then(|| text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .unwrap();
        for pressed in [true, false] {
            run(
                &mut app,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
        // Finish the expanding-header animation without desktop interaction.
        ctx.style_mut(|style| style.animation_time = 0.0);
        let output = run(&mut app, vec![]);
        for name in film::COLOR_NAMES {
            assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()==name)),"Missing expanded {name} group");
        }
    }
    #[test]
    fn command_wheel_zooms_image_without_changing_ui_scale() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(
            &ctx,
            dir.path().to_owned(),
            super::super::model::Library::default(),
            None,
        );
        let mut roll = Roll::new("Zoom");
        roll.material = Material::Positive;
        let frame = super::super::model::Frame::new("source.png".into());
        app.textures.insert(
            frame.id.clone(),
            ctx.load_texture(
                "zoom-fixture",
                egui::ColorImage::new([100, 80], Color32::GRAY),
                egui::TextureOptions::LINEAR,
            ),
        );
        roll.frames.push(frame);
        app.library.rolls.push(roll);
        app.grid = false;
        let scale = ctx.pixels_per_point();
        let run = |app: &mut App, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440.0, 960.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| draw(app, ctx),
            )
        };
        run(&mut app, vec![]);
        run(
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(700.0, 500.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, 3.0),
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                },
            ],
        );
        assert!(app.view.scale > 1.0);
        assert!((ctx.pixels_per_point() - scale).abs() < 1e-6);
    }
    #[test]
    fn roll_crop_and_mirror_controls_are_visible_and_clickable_above_the_contact_sheet() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(
            &ctx,
            directory.path().to_owned(),
            super::super::model::Library::default(),
            None,
        );
        let mut roll = Roll::new("Roll tools");
        roll.material = Material::Positive;
        roll.frames = ["one.png", "two.png"]
            .map(|name| super::super::model::Frame::new(name.into()))
            .to_vec();
        roll.frames[0].mirrored = true;
        app.library.rolls.push(roll);
        let run = |app: &mut App, events, width| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 960.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| draw(app, ctx),
            )
        };
        for width in [1080., 1440.] {
            let output = run(&mut app, vec![], width);
            for label in ["Autocrop all frames", "Auto upright", "Mirror all frames"] {
                let position = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == label => {
                            Some(text.pos + text.galley.size() * 0.5)
                        }
                        _ => None,
                    })
                    .expect("Roll controls should be visible without opening an expander");
                assert!(position.y < 350.);
                app.actions.clear();
                for pressed in [true, false] {
                    run(
                        &mut app,
                        vec![
                            egui::Event::PointerMoved(position),
                            egui::Event::PointerButton {
                                pos: position,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::default(),
                            },
                        ],
                        width,
                    );
                }
                assert!(app.actions.iter().any(|action| matches!(
                    (label, action),
                    ("Autocrop all frames", Action::AutoCropAll)
                        | ("Auto upright", Action::AutoUpright)
                        | ("Mirror all frames", Action::MirrorAll(true))
                )));
                assert!(app.grid);
            }
        }
    }

    #[test]
    fn native_widgets_accept_headless_pointer_input_and_panels_draw() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(
            &ctx,
            directory.path().to_owned(),
            super::super::model::Library::default(),
            None,
        );
        let mut roll = Roll::new("Widget test");
        roll.frames
            .push(super::super::model::Frame::new("one.png".into()));
        roll.frames
            .push(super::super::model::Frame::new("two.png".into()));
        app.selected = roll.frames.iter().map(|f| f.id.clone()).collect();
        app.library.rolls.push(roll);
        let run = |app: &mut App, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440.0, 960.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| draw(app, ctx),
            )
        };
        let output = run(&mut app, vec![]);
        let selected = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    (text.galley.text() == "Selected").then(|| text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .expect("Selected scope button must be painted");
        for pressed in [true, false] {
            run(
                &mut app,
                vec![
                    egui::Event::PointerMoved(selected),
                    egui::Event::PointerButton {
                        pos: selected,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
        assert!(app.scope_selected);
        assert!(!app.scope_roll);
        for panel in [Panel::Adjust, Panel::Conversion, Panel::Dust] {
            app.panel = panel;
            assert!(!run(&mut app, vec![]).shapes.is_empty());
        }
        app.export_dialog = true;
        assert!(!run(&mut app, vec![]).shapes.is_empty());
    }
    #[test]
    fn dust_coordinates_survive_crop_and_rotation() {
        let crop = Rect {
            x: 0.2,
            y: 0.1,
            w: 0.6,
            h: 0.7,
            angle: -2.5,
        };
        for rotation in 0..4 {
            for mirrored in [false, true] {
                let p = [0.35, 0.63];
                let v = source_to_view(p, crop, rotation, mirrored, 1.5);
                let out = view_to_source(v, crop, rotation, mirrored, 1.5);
                assert!((p[0] - out[0]).abs() < 0.00001 && (p[1] - out[1]).abs() < 0.00001);
            }
        }
    }
}
