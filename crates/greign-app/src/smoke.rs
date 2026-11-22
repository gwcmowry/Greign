//! Opt-in native egui input replay, independent of desktop pointer scaling.
use super::App;
use eframe::egui::{self, Event, Modifiers, PointerButton};
use serde::Deserialize;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Step {
    /// Supply a folder-picker result; the visible Import roll button still commits it.
    ChooseRollFolder {
        path: PathBuf,
    },
    Click {
        x: f32,
        y: f32,
        #[serde(default)]
        command: bool,
        #[serde(default)]
        shift: bool,
    },
    Drag {
        x: f32,
        y: f32,
        to_x: f32,
        to_y: f32,
    },
    Key {
        name: String,
        #[serde(default)]
        command: bool,
        #[serde(default)]
        shift: bool,
    },
    Capture {
        path: PathBuf,
    },
    Wait {
        ms: u64,
    },
}
pub struct Replay {
    steps: Vec<Step>,
    index: usize,
    phase: u8,
    next: Instant,
}
impl Replay {
    pub fn finished(&self) -> bool {
        self.index >= self.steps.len()
    }
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        Ok(Self {
            steps: serde_json::from_slice(&std::fs::read(path)?)?,
            index: 0,
            phase: 0,
            next: Instant::now() + Duration::from_secs(3),
        })
    }
    pub fn inject(&mut self, app: &mut App, ctx: &egui::Context, input: &mut egui::RawInput) {
        if self.index >= self.steps.len() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(30));
        if Instant::now() < self.next || app.busy || app.rendering {
            return;
        }
        input.focused = true;
        let advance;
        let mut delay = 120;
        match &self.steps[self.index] {
            Step::ChooseRollFolder { path } => {
                app.choose_import_folder(path.clone());
                app.new_roll_open = true;
                advance = true;
            }
            Step::Click {
                x,
                y,
                command,
                shift,
            } => {
                let pos = egui::pos2(*x, *y);
                let modifiers = Modifiers {
                    ctrl: *command,
                    command: *command,
                    shift: *shift,
                    ..Default::default()
                };
                input.modifiers = modifiers;
                input.events.push(Event::PointerMoved(pos));
                input.events.push(Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: self.phase == 0,
                    modifiers,
                });
                advance = self.phase == 1;
                self.phase += 1;
            }
            Step::Drag { x, y, to_x, to_y } => {
                let pos = egui::pos2(
                    *x + (*to_x - *x) * f32::from(self.phase.min(6)) / 6.0,
                    *y + (*to_y - *y) * f32::from(self.phase.min(6)) / 6.0,
                );
                input.events.push(Event::PointerMoved(pos));
                if self.phase == 0 || self.phase == 7 {
                    input.events.push(Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: self.phase == 0,
                        modifiers: Modifiers::default(),
                    });
                }
                advance = self.phase == 7;
                self.phase += 1;
            }
            Step::Key {
                name,
                command,
                shift,
            } => {
                let key = match name.as_str() {
                    "R" => egui::Key::R,
                    "X" => egui::Key::X,
                    "C" => egui::Key::C,
                    "E" => egui::Key::E,
                    "Z" => egui::Key::Z,
                    "ArrowRight" => egui::Key::ArrowRight,
                    "ArrowLeft" => egui::Key::ArrowLeft,
                    _ => egui::Key::Escape,
                };
                let modifiers = Modifiers {
                    ctrl: *command,
                    command: *command,
                    shift: *shift,
                    ..Default::default()
                };
                input.modifiers = modifiers;
                input.events.push(Event::Key {
                    key,
                    physical_key: None,
                    pressed: self.phase == 0,
                    repeat: false,
                    modifiers,
                });
                advance = self.phase == 1;
                self.phase += 1;
            }
            Step::Capture { path } => {
                app.screenshot = Some(path.clone());
                app.screenshot_requested = false;
                let _ = super::model::save(&path.with_extension("json"), &app.library);
                advance = true;
                delay = 700;
            }
            Step::Wait { ms } => {
                delay = *ms;
                advance = true;
            }
        }
        if advance {
            self.index += 1;
            self.phase = 0;
            delay = delay.max(250);
            eprintln!("Native UI replay: step {}/{}", self.index, self.steps.len());
        }
        self.next = Instant::now() + Duration::from_millis(delay);
    }
}
