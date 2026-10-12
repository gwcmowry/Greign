#![allow(clippy::collapsible_if)]
use super::{Action, App, Panel, Roll, Tool};
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
    ui.label(RichText::new(text).color(MUTED).size(12.0));
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
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                muted(
                    ui,
                    "Arrow keys  Navigate\nR  Keep / reject\nCtrl Z  Undo\nEsc  Leave tool",
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
                ui.heading(&r.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let kept = r.frames.iter().filter(|f| !f.rejected).count();
                    let ready = r.analysis.is_some() || r.material == Material::Positive;
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
                        .clicked()
                    {
                        app.export_dialog = true;
                    }
                    if ui.button("Metadata").clicked() {
                        app.metadata_open = true;
                    }
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
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !app.busy && !r.frames.is_empty() && r.material != Material::Positive,
                        egui::Button::new(if r.analysis.is_some() {
                            "Re-analyze & convert"
                        } else {
                            "Analyze & convert roll"
                        }),
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
            if r.analysis.as_ref().is_some_and(|a| !a.warnings.is_empty()) {
                ui.colored_label(
                    GOLD,
                    "Analysis needs attention · see Conversion for details",
                );
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
            if app.comparing {
                ui.colored_label(
                    GOLD,
                    "SAVED VERSION · release compare to return to current edits",
                );
            }
            if app.preview_analysis {
                ui.colored_label(
                    GOLD,
                    "PROPOSED ANALYSIS · exports still use the accepted conversion",
                );
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
        Panel::Conversion=>{
            if app.busy || app.comparing { ui.disable(); }
            ui.label(RichText::new("Guided calibration").size(18.0));
            muted(ui,"1. Inspect the original scan for clipping and uneven lighting.");
            if ui.button("Inspect original scan").clicked() { app.show_negative=true; app.grid=false; app.invalidate(); }
            let r=&app.library.rolls[app.roll];
            if let Some(warning)=r.frames.get(app.frame).and_then(|f| f.scan_warning.as_ref()) { ui.colored_label(GOLD, warning); }
            if let Some(a)=&r.analysis {
                ui.add_space(8.0);ui.label(RichText::new(&a.method).color(GOLD));muted(ui,format!("Shared by {} analyzed frames",a.frames));
                ui.add_space(8.0);muted(ui,format!("Film base   {:.3} / {:.3} / {:.3}\nDensity range   {:.2}",a.base[0],a.base[1],a.base[2],a.range));
                for warning in &a.warnings {ui.add_space(10.0);ui.label(RichText::new(warning).size(12.0).color(GOLD));}
            }else{muted(ui,"Analyze the roll to estimate its film base and density range.");}
            ui.add_space(16.0); muted(ui,"2. Measure a clean, unexposed film border.");
            if ui.add_enabled(!r.frames.is_empty()&&!app.busy&&!r.conversion_locked,egui::Button::new("Select film-base patch")).clicked(){app.grid=false;app.tool=Tool::Base;app.show_negative=true;app.revision+=1;}
            muted(ui,"Drag over clean, unexposed film. Your selection is separate from the final crop.");
            if ui.add_enabled(r.base.is_some()&&!r.demo&&!r.conversion_locked,egui::Button::new("Use automatic base estimate")).clicked(){app.library.rolls[app.roll].base=None;app.dirty=true;app.status="Re-analyze to apply the automatic base estimate".into();}
            ui.add_space(14.0);let r=&mut app.library.rolls[app.roll];
            ui.collapsing("3. Analysis area",|ui|{
                muted(ui,"Inset from all sides. Excludes holder and film border; does not crop the export.");
                let mut inset=r.analysis_area.x;
                if ui.add_enabled(!r.conversion_locked,egui::Slider::new(&mut inset,0.0..=0.25).text("Inset")).changed(){r.analysis_area=Rect{x:inset,y:inset,w:1.0-2.0*inset,h:1.0-2.0*inset};app.dirty=true;app.status="Re-analyze to use the changed analysis area".into();}
            });
            app.dirty |= ui.add_enabled(r.analysis.is_some()&&!app.busy,egui::Checkbox::new(&mut r.conversion_locked,"Lock accepted conversion")).changed();
            muted(ui,"A lock protects the accepted calibration. New scans use it; new analysis requires your approval after unlocking.");
            if ui.add_enabled(!app.busy&&!r.frames.is_empty(),egui::Button::new("4. Analyze / propose conversion")).clicked(){app.show_negative=false;app.actions.push(Action::Analyze);}
            if let Some((id, a))=&app.pending_analysis {
                if *id==r.id {
                    ui.separator();ui.colored_label(GOLD,"Proposed calibration (not saved yet)");
                    muted(ui,format!("{} · range {:.3}",a.method,a.range));
                    for warning in &a.warnings {ui.colored_label(GOLD,warning);}
                    if ui.checkbox(&mut app.preview_analysis,"Preview proposed conversion").changed(){app.show_negative=false;app.revision+=1;}
                    if ui.add_enabled(!r.conversion_locked&&!app.busy,egui::Button::new("Accept proposed conversion")).clicked(){app.actions.push(Action::AcceptAnalysis);}
                    if ui.button("Discard proposal").clicked(){app.actions.push(Action::DiscardAnalysis);}
                }
            }
            if ui.add_enabled(r.previous_analysis.is_some()&&!app.busy&&!r.conversion_locked,egui::Button::new("Restore previous analysis")).clicked(){app.actions.push(Action::RestoreAnalysis);}
            ui.add_space(16.0);muted(ui,"Initial density / virtual-paper model. Film-stock calibration and full-roll color consistency still need real scan validation.");
        }
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
            ui.collapsing("Composition · active frame only", |ui| {
                if ui.button("Crop this frame").clicked() {app.grid=false;app.tool=Tool::Crop;app.scope_roll=false;app.scope_selected=false;app.show_negative=false;app.invalidate();}
                if ui.button("Reset crop").clicked() {app.library.rolls[app.roll].frames[app.frame].crop=Rect::default();app.dirty=true;}
                if ui.button("Rotate clockwise").clicked() {app.actions.push(Action::Rotate);}
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
                    && (roll.analysis.is_some() || roll.material == Material::Positive),
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
    ui.horizontal(|ui| {
        if ui
            .selectable_label(!app.show_negative, "Positive")
            .clicked()
        {
            app.show_negative = false;
            app.invalidate();
        }
        if ui
            .selectable_label(app.show_negative, "Original negative")
            .clicked()
        {
            app.show_negative = true;
            app.invalidate();
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
                ui.colored_label(GOLD, "Drag an export crop");
            }
            Tool::Base => {
                ui.colored_label(GOLD, "Drag a clean film-base patch");
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
fn view_to_source(p: [f32; 2], crop: Rect, rotation: u8) -> [f32; 2] {
    let [u, v] = p;
    let [x, y] = match rotation % 4 {
        1 => [v, 1.0 - u],
        2 => [1.0 - u, 1.0 - v],
        3 => [1.0 - v, u],
        _ => [u, v],
    };
    [crop.x + x * crop.w, crop.y + y * crop.h]
}
fn source_to_view(p: [f32; 2], crop: Rect, rotation: u8) -> [f32; 2] {
    let x = (p[0] - crop.x) / crop.w;
    let y = (p[1] - crop.y) / crop.h;
    match rotation % 4 {
        1 => [1.0 - y, x],
        2 => [1.0 - x, 1.0 - y],
        3 => [y, 1.0 - x],
        _ => [x, y],
    }
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
    if !frame.heals.is_empty() {
        return None;
    }
    let resource = gpu.lock().unwrap();
    if app.full_desired.as_ref() != Some(&resource.key)
        || !resource.key.starts_with(&format!("{id}:"))
    {
        return None;
    }
    let edits = version
        .map_or(roll.adjustments, |v| v.state.adjustments)
        .combined(frame.adjustments);
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
        || (saved.map_or(roll.analysis.is_none(), |v| v.state.analysis.is_none())
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
    ui.scope(|ui|{
        let (viewport,response)=ui.allocate_exact_size(Vec2::new(ui.available_width().max(1.0),(ui.available_height()-12.0).max(1.0)),egui::Sense::click_and_drag());
        ui.set_clip_rect(viewport.intersect(ui.clip_rect()));
        let key=serde_json::to_string(&(&frame.id,frame.crop,frame.rotation,negative)).unwrap();
        if app.view.key!=key {app.view.fit();app.view.key=key;app.zoom=false;}
        // Use stable scan aspect, not the current low/full-resolution texture.
        let natural=app.cache.get(&frame.id).map_or(texture.size_vec2(),|scan|{
            let mut size=Vec2::new(scan.image.width as f32,scan.image.height as f32);
            if !negative {size=Vec2::new(size.x*frame.crop.w,size.y*frame.crop.h);if frame.rotation%2==1 {size=Vec2::new(size.y,size.x);}}
            size
        });
        let fitted=fit(natural,viewport.size());
        if app.view.native_pending {if let Some(size)=full_size {app.view.scale=(size.x/ui.ctx().pixels_per_point()/fitted.x).clamp(1.0,32.0);app.view.native_pending=false;}}
        if response.hovered() {
            let (factor,pointer,scroll,command)=ui.input(|i|(i.zoom_delta(),i.pointer.hover_pos(),i.smooth_scroll_delta,i.modifiers.command||i.modifiers.ctrl||i.modifiers.mac_cmd));
            if let Some(pointer)=pointer {if (factor-1.0).abs()>0.00001 {app.view.zoom_at(factor,pointer,viewport,fitted);app.zoom=app.view.scale>1.0;app.refine=true;app.scheduled=None;}}
            if !command {app.view.pan+=scroll;}
        }
        if response.dragged_by(egui::PointerButton::Middle)||(app.tool==Tool::View&&response.dragged_by(egui::PointerButton::Primary)) {app.view.pan+=ui.input(|i|i.pointer.delta());}
        app.view.clamp(viewport,fitted);
        app.view.percent=full_size.map(|size|app.view.scale*fitted.x*ui.ctx().pixels_per_point()/size.x*100.);
        let rect=app.view.rect(viewport,fitted);
        if let Some((gpu,settings,_))=&gpu {ui.painter().add(greign_gl_preview::Preview::callback(gpu.clone(),rect,*settings));}
        else {ui.painter().image(texture.id(),rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),Color32::WHITE);}
        let source_crop=if negative {Rect::default()} else {frame.crop};
        let source_rotation=if negative {0} else {frame.rotation};
        let to_source=|pos:egui::Pos2|view_to_source([((pos.x-rect.left())/rect.width()).clamp(0.0,1.0),((pos.y-rect.top())/rect.height()).clamp(0.0,1.0)],source_crop,source_rotation);
        if app.tool==Tool::Neutral&&!negative&&!app.busy&&!app.comparing&&!app.preview_analysis&&response.clicked() {
            if let Some(pos)=response.interact_pointer_pos().filter(|p|rect.contains(*p)) {app.pick_neutral(to_source(pos),ui.ctx());}
        }
        if app.tool==Tool::Heal&&!app.show_negative&&!app.comparing&&!app.preview_analysis&&!app.busy {
            for (i,heal) in frame.heals.iter().enumerate(){let point=source_to_view(heal.target,frame.crop,frame.rotation);
                let pos=egui::pos2(rect.left()+point[0]*rect.width(),rect.top()+point[1]*rect.height());
                let r=heal.radius*rect.width().min(rect.height())/frame.crop.w.min(frame.crop.h);
                ui.painter().circle_stroke(pos,r,Stroke::new(1.2,GOLD));
                if app.selected_heal==Some(i){let point=source_to_view(heal.source,frame.crop,frame.rotation);let source=egui::pos2(rect.left()+point[0]*rect.width(),rect.top()+point[1]*rect.height());
                    ui.painter().circle_stroke(source,r,Stroke::new(1.2,Color32::LIGHT_BLUE));ui.painter().line_segment([pos,source],Stroke::new(1.0,GOLD));}
            }
            if response.clicked(){if let Some(pos)=response.interact_pointer_pos().filter(|p|rect.contains(*p)){let target=to_source(pos);let f=&mut app.library.rolls[app.roll].frames[app.frame];
                if app.move_source {if let Some(i)=app.selected_heal.filter(|i|*i<f.heals.len()){f.heals[i].source=target;app.dirty=true;}app.move_source=false;}
                else {f.heals.push(Heal{target,source:film::suggest_source(target,app.dust_radius,texture.aspect_ratio()),radius:app.dust_radius});app.selected_heal=Some(f.heals.len()-1);app.dirty=true;}
            }}
        }
        if (app.tool==Tool::Crop||app.tool==Tool::Base)&&!app.comparing&&!app.preview_analysis&&!app.busy&&(app.tool!=Tool::Base||!app.library.rolls[app.roll].conversion_locked) {
            if response.drag_started(){app.drag_start=ui.input(|i|i.pointer.press_origin());}
            if let (Some(start),Some(end))=(app.drag_start,response.interact_pointer_pos()){
                ui.painter().rect_stroke(egui::Rect::from_two_pos(start,end),0.0,Stroke::new(1.5,GOLD),egui::StrokeKind::Inside);
                if response.drag_stopped(){let a=to_source(start);let b=to_source(end);let region=Rect{x:a[0].min(b[0]),y:a[1].min(b[1]),w:(a[0]-b[0]).abs(),h:(a[1]-b[1]).abs()};
                    if region.valid()&&region.w>0.005&&region.h>0.005 {
                        if app.tool==Tool::Crop {app.library.rolls[app.roll].frames[app.frame].crop=region;app.dirty=true;}
                        else if let Some(scan)=app.cache.get(&frame.id){match film::measure_base(scan,region){Ok(base)=>{app.library.rolls[app.roll].base=Some(base);app.dirty=true;app.status="Film-base patch selected. Re-analyze & convert to apply it.".into();}Err(e)=>app.status=e.to_string()}}
                    }app.drag_start=None;app.tool=Tool::View;
                }
            }
        }
    });
}

fn dialogs(app: &mut App, ctx: &egui::Context) {
    if app.new_roll_open {
        let mut open = true;
        let mut create = false;
        egui::Window::new("Import a roll").open(&mut open).collapsible(false).resizable(false).default_width(410.0).show(ctx,|ui|{
            ui.label("Give this roll a home.");ui.add_space(8.0);ui.label("Roll name");ui.text_edit_singleline(&mut app.new_name);
            ui.label("Film stock (optional)");ui.text_edit_singleline(&mut app.new_stock);
            ui.horizontal(|ui|{ui.selectable_value(&mut app.new_material,Material::ColorNegative,"Color negative");ui.selectable_value(&mut app.new_material,Material::BlackAndWhite,"B&W");ui.selectable_value(&mut app.new_material,Material::Positive,"Positive");});
            muted(ui,"Choose all scans in the next dialog. Original files remain in place and are never modified.");
            create=primary(ui,"Create roll & choose scans").clicked();
        });
        app.new_roll_open = open;
        if create {
            let before = app.library.clone();
            let mut r = Roll::new(if app.new_name.trim().is_empty() {
                "Untitled roll"
            } else {
                app.new_name.trim()
            });
            r.stock.clone_from(&app.new_stock);
            r.material = app.new_material;
            app.library.rolls.push(r);
            app.roll = app.library.rolls.len() - 1;
            app.frame = 0;
            app.selected.clear();
            app.scope_selected = false;
            app.version_index = 0;
            app.pending_analysis = None;
            app.preview_analysis = false;
            app.new_roll_open = false;
            app.new_name.clear();
            app.new_stock.clear();
            app.remember(before);
            app.save();
            app.actions.push(Action::Import);
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
    if app.export_dialog {
        let mut open = true;
        let mut export = false;
        egui::Window::new("Export frames").open(&mut open).collapsible(false).default_width(450.0).show(ctx,|ui|{
        let r=&mut app.library.rolls[app.roll];
        ui.horizontal_wrapped(|ui|{
            ui.selectable_value(&mut app.export_scope,super::ExportScope::Roll,"Whole roll");
            ui.selectable_value(&mut app.export_scope,super::ExportScope::Selected,"Selected frames");
            ui.selectable_value(&mut app.export_scope,super::ExportScope::Frame,"Active frame");
        });
        let count=r.frames.iter().enumerate().filter(|(i,f)|!f.rejected&&match app.export_scope {super::ExportScope::Roll=>true,super::ExportScope::Selected=>app.selected.contains(&f.id),super::ExportScope::Frame=>*i==app.frame}).count();
        ui.label(format!("{count} kept frames to export"));
        let date_valid=match greign_export::CaptureDate::parse(&r.date) {
            Ok(Some(date))=>{ui.label(format!("Capture date: {}",date.as_exif()));true},
            Ok(None)=>{ui.label("No roll date set · capture date will be omitted");true},
            Err(error)=>{ui.colored_label(Color32::LIGHT_RED,error);false},
        };
        ui.horizontal(|ui|{
            if ui.button("Full JPEG · quality 95").clicked(){r.export.dng=false;r.export.tiff=false;r.export.long_edge=0;r.export.jpeg_quality=95;app.dirty=true;}
            if ui.button("Full TIFF").clicked(){r.export.dng=false;r.export.tiff=true;r.export.long_edge=0;app.dirty=true;}
            if ui.button("Share JPEG · 2048 px").clicked(){r.export.dng=false;r.export.tiff=false;r.export.long_edge=2048;r.export.jpeg_quality=90;app.dirty=true;}
        });
        for preset in r.export_presets.clone() {if ui.button(format!("Use {}",preset.name)).clicked(){r.export=preset;app.dirty=true;}}
        ui.horizontal_wrapped(|ui|{
            if ui.selectable_label(!r.export.dng&&!r.export.tiff,"JPEG").clicked(){r.export.dng=false;r.export.tiff=false;app.dirty=true;}
            if ui.selectable_label(!r.export.dng&&r.export.tiff,"TIFF · 16-bit ZIP").clicked(){r.export.dng=false;r.export.tiff=true;app.dirty=true;}
            if ui.selectable_label(r.export.dng,"DNG · linear 32-bit float").clicked(){r.export.dng=true;r.export.tiff=false;app.dirty=true;}
        });
        if r.export.dng {muted(ui,"Full-resolution edited positive in linear Rec.2020. Conversion, adjustments, crop and healing are baked in. Not original camera RAW. Large uncompressed files; other editors may apply a different initial look. Lightroom import still needs validation.");}
        if !r.export.dng && !r.export.tiff {
            app.dirty|=ui.add(egui::Slider::new(&mut r.export.jpeg_quality,1..=100).text("JPEG quality")).changed();
            muted(ui,"95 keeps fine detail; 90 is a smaller sharing copy. Quality 100 is still lossy and can produce much larger files. Full JPEG stays at quality 95 unless you change it.");
        }
        ui.label("Longest edge in pixels (0 = full resolution)");
        app.dirty|=ui.add_enabled(!r.export.dng,egui::DragValue::new(&mut r.export.long_edge).range(0..=32000)).changed();
        ui.label("Filename prefix (letters, numbers, dash, underscore)");app.dirty|=ui.text_edit_singleline(&mut r.export.prefix).changed();
        let destination=r.export.destination.as_ref().unwrap_or(&app.export_dir);
        ui.label(destination.display().to_string());if ui.button("Choose destination…").clicked(){if let Some(path)=rfd::FileDialog::new().set_directory(destination).pick_folder(){r.export.destination=Some(path);app.dirty=true;}}
        ui.label("Preset name");app.dirty|=ui.text_edit_singleline(&mut r.export.name).changed();
        if ui.add_enabled(!r.export.name.trim().is_empty(),egui::Button::new("Save / update preset for this roll")).clicked(){r.export_presets.retain(|p|p.name!=r.export.name);r.export_presets.push(r.export.clone());app.dirty=true;}
        muted(ui,"Ctrl+E opens this dialog. Settings are remembered. Each export gets a new folder and recipe. If a frame fails, retry only failed frames with the same frozen settings.");
        export=ui.add_enabled(date_valid&&count>0&&!app.busy&&!app.preview_analysis&&!app.comparing,egui::Button::new("Export kept frames")).clicked();
    });
        app.export_dialog = open;
        if export {
            app.export_dialog = false;
            app.actions.push(Action::Export);
        }
    }
    if let Some(retry) = &app.export_retry {
        let mut retry_now = false;
        egui::Window::new("Export needs attention").show(ctx,|ui|{
            ui.label(format!("{} frames failed. Successful exports are already saved.",retry.ids.len()));
            ui.label(&app.status);
            for frame in &retry.roll.frames {if retry.ids.contains(&frame.id){ui.label(&frame.name);}}
            muted(ui,"Restore missing files or resolve the destination problem, then retry. Uses the original export settings, even if you have edited the roll since.");
            retry_now=ui.add_enabled(!app.busy,egui::Button::new("Retry failed frames only")).clicked();
        });
        if retry_now {
            app.actions.push(Action::RetryExport);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        };
        for rotation in 0..4 {
            let p = [0.35, 0.63];
            let v = source_to_view(p, crop, rotation);
            let out = view_to_source(v, crop, rotation);
            assert!((p[0] - out[0]).abs() < 0.00001 && (p[1] - out[1]).abs() < 0.00001);
        }
    }
}
