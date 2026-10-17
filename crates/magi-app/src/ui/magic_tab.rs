//! The Magic tab: preparing small AI vision models for macros.
//!
//! A model is imported (or opened as a `.magic` file), configured here – how
//! pictures are prepared for it, which part of the screen it looks at, what
//! its answers mean and what to do about them, which settings a macro may
//! adjust – tried out live, and handed to the Macro tab with "Use in Macro".

use egui::{DragValue, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::geom::{Displays, Rect};
use magi_core::magic::{
    CaptureArea, ClassSpec, ColorOrder, IMAGENET_MEAN, IMAGENET_STD, Layout, MagicModel, ModelSetting, SettingKind,
    SettingTarget,
};

use super::widgets::{self, card};
use crate::magic::{MagicDoc, Tester};
use crate::theme;

pub enum MagicEvent {
    New,
    Open,
    /// Import a project folder, or a file (`.zip` or `.onnx`).
    Import {
        folder: bool,
    },
    Save,
    SaveAs,
    UseInMacro,
    ReplaceModel,
    /// Pick the fixed screen area the model looks at.
    PickArea,
    /// Add pictures to the training (or fine-tuning) data, for a class.
    AddData {
        finetune: bool,
        class: usize,
    },
    /// Run the model on a picture file.
    TryPicture,
    StartTest,
    StopTest,
}

pub struct MagicEnv<'a> {
    pub displays: &'a Displays,
    /// Where models run, as a sentence.
    pub engines: String,
    pub dry_run: bool,
}

#[derive(Default)]
pub struct MagicTab {
    pub doc: Option<MagicDoc>,
    pub tester: Option<Tester>,
    /// Whether the live test presses the model's key.
    pub press: bool,
    preview: Option<(u64, egui::TextureHandle)>,
    /// The result of "Try a picture": its name and the class probabilities.
    pub picture: Option<(String, Vec<f32>)>,
}

/// Normalisations people know by name.
const NORMALISATIONS: [(&str, [f32; 3], [f32; 3]); 3] = [
    ("ImageNet (most models)", IMAGENET_MEAN, IMAGENET_STD),
    ("0 to 1", [0.0; 3], [1.0; 3]),
    ("−1 to 1", [0.5; 3], [0.5; 3]),
];

impl MagicTab {
    pub fn show(&mut self, ui: &mut Ui, env: &MagicEnv<'_>) -> Vec<MagicEvent> {
        let mut events = Vec::new();
        egui::Panel::top("magic-toolbar")
            .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| self.toolbar(ui, env, &mut events));

        if self.doc.is_none() {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::bg_panel())).show(ui, |ui| {
                empty_state(ui, &mut events);
            });
            return events;
        }

        egui::Panel::right("magic-test")
            .resizable(true)
            .default_size(330.0)
            .size_range(260.0..=560.0)
            .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::same(12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.test_panel(ui, env, &mut events);
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::same(12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let doc = self.doc.as_mut().expect("checked above");
                    editor(ui, doc, env, &mut events);
                });
            });

        // Changes take effect in a running test at once.
        if let (Some(tester), Some(doc)) = (&self.tester, &self.doc) {
            let mut config = tester.config.lock().unwrap_or_else(|p| p.into_inner());
            if *config != doc.file.model {
                *config = doc.file.model.clone();
            }
        }
        events
    }

    fn toolbar(&mut self, ui: &mut Ui, env: &MagicEnv<'_>, events: &mut Vec<MagicEvent>) {
        widgets::button_row(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.menu_button(format!("{}  Import", icon::DOWNLOAD_SIMPLE), |ui| {
                ui.set_width(250.0);
                if ui.button(format!("{}  A project folder…", icon::FOLDER_OPEN)).clicked() {
                    events.push(MagicEvent::Import { folder: true });
                    ui.close();
                }
                if ui.button(format!("{}  A .zip or .onnx file…", icon::FILE_ZIP)).clicked() {
                    events.push(MagicEvent::Import { folder: false });
                    ui.close();
                }
                widgets::note(ui, "The model is read, and its settings worked out from the project's files.");
            })
            .response
            .on_hover_text("Import a model made elsewhere");
            if ui.button(format!("{}  Open", icon::FOLDER_OPEN)).on_hover_text("Open a .magic file").clicked() {
                events.push(MagicEvent::Open);
            }
            let has_doc = self.doc.is_some();
            if ui.add_enabled(has_doc, egui::Button::new(format!("{}  Save", icon::FLOPPY_DISK))).clicked() {
                events.push(MagicEvent::Save);
            }
            if ui.add_enabled(has_doc, egui::Button::new(format!("{}  Save as…", icon::FLOPPY_DISK_BACK))).clicked() {
                events.push(MagicEvent::SaveAs);
            }
            if ui.add_enabled(has_doc, egui::Button::new(format!("{}  New", icon::FILE_PLUS))).clicked() {
                events.push(MagicEvent::New);
            }
            ui.add_space(6.0);
            let ready = self.doc.as_ref().is_some_and(|d| !d.file.onnx.is_empty());
            // The button sits in its own scope, which would not move to the next line by itself.
            if ui.available_size_before_wrap().x < 130.0 {
                ui.end_row();
            }
            ui.add_enabled_ui(ready, |ui| {
                if widgets::accent_button(
                    ui,
                    format!("{}  Use in Macro", icon::TREE_STRUCTURE),
                    theme::accent(),
                    vec2(0.0, 28.0),
                )
                .on_hover_text("Put the model into the open macro, with blocks that run it")
                .clicked()
                {
                    events.push(MagicEvent::UseInMacro);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{}  {}", icon::CPU, env.engines)).size(11.5).color(theme::text_faint()),
                )
                .on_hover_text("Where models run. Change it under Settings › AI.");
                if let Some(doc) = &self.doc {
                    let name = if doc.is_dirty() { format!("{} •", doc.title()) } else { doc.title() };
                    ui.add(egui::Label::new(RichText::new(name).color(theme::text_dim())).truncate());
                }
            });
        });
    }

    fn test_panel(&mut self, ui: &mut Ui, env: &MagicEnv<'_>, events: &mut Vec<MagicEvent>) {
        let doc = self.doc.as_ref().expect("checked by the caller");
        ui.label(RichText::new("TRY IT").size(10.5).color(theme::text_faint()));
        let running = self.tester.is_some();
        widgets::button_row(ui, |ui| {
            let (text, color) = if running {
                (format!("{}  Stop", icon::STOP), theme::danger())
            } else {
                (format!("{}  Watch the screen", icon::PLAY), theme::accent())
            };
            ui.add_enabled_ui(!doc.file.onnx.is_empty(), |ui| {
                if widgets::accent_button(ui, text, color, vec2(0.0, 28.0)).clicked() {
                    events.push(if running { MagicEvent::StopTest } else { MagicEvent::StartTest });
                }
            });
            if ui
                .button(format!("{}  Picture…", icon::IMAGE))
                .on_hover_text("Run the model on a picture file")
                .clicked()
            {
                events.push(MagicEvent::TryPicture);
            }
        });
        ui.add_enabled_ui(!running, |ui| {
            widgets::toggle_row(
                ui,
                "Press the key when it acts",
                "Off: the test only shows what the model would do. On: it presses the key, as a macro would.",
                &mut self.press,
            );
        });
        if self.press && env.dry_run {
            widgets::notice(ui, icon::INFO, "Dry run is on (Options): nothing is pressed.", theme::text_faint());
        }

        let mut shown_probabilities: Option<Vec<f32>> = None;
        if let Some(tester) = &self.tester {
            let state = tester.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(error) = &state.error {
                widgets::notice(ui, icon::WARNING, error.clone(), theme::danger());
            }
            ui.label(
                RichText::new(format!(
                    "{} · {:.0} looks per second · acted {} times",
                    state.engine, state.looks_per_second, state.acted
                ))
                .size(11.5)
                .color(theme::text_dim()),
            );
            if let Some((rgba, w, h)) = &state.picture
                && self.preview.as_ref().is_none_or(|(frame, _)| *frame != state.frame)
            {
                let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], rgba);
                let texture = ui.ctx().load_texture("magic-preview", image, egui::TextureOptions::LINEAR);
                self.preview = Some((state.frame, texture));
            }
            if let Some((_, texture)) = &self.preview {
                let side = ui.available_width().min(240.0);
                ui.vertical_centered(|ui| {
                    ui.image((
                        texture.id(),
                        vec2(side, side * texture.size()[1] as f32 / texture.size()[0].max(1) as f32),
                    ));
                });
                ui.label(RichText::new("What the model sees").size(10.5).color(theme::text_faint()));
            }
            if let Some(decision) = &state.decision {
                let (text, color) = if decision.act {
                    (
                        format!("{}  {} – act ({:.0}%)", icon::LIGHTNING, decision.label, decision.confidence * 100.0),
                        theme::success(),
                    )
                } else {
                    (format!("{} ({:.0}%)", decision.label, decision.confidence * 100.0), theme::text())
                };
                ui.label(RichText::new(text).strong().color(color));
            }
            shown_probabilities = Some(state.probabilities.clone());
        } else if let Some((name, probabilities)) = &self.picture {
            ui.label(RichText::new(format!("{}  {name}", icon::IMAGE)).color(theme::text_dim()));
            let decision = doc.file.model.decide(probabilities);
            ui.label(
                RichText::new(format!(
                    "{} ({:.0}%){}",
                    decision.label,
                    decision.confidence * 100.0,
                    if decision.act { " – act" } else { "" }
                ))
                .strong(),
            );
            shown_probabilities = Some(probabilities.clone());
        } else {
            widgets::note(
                ui,
                "Watch the screen to see what the model sees and answers, live – or try it on a picture file.",
            );
        }

        if let Some(probabilities) = shown_probabilities {
            ui.add_space(4.0);
            let best = probabilities.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i);
            for (class, p) in probabilities.iter().enumerate() {
                let label = doc.file.model.label(class);
                let acts = doc.file.model.classes.get(class).is_some_and(|c| c.act);
                let bar = egui::ProgressBar::new(*p)
                    .desired_height(14.0)
                    .text(RichText::new(format!("{label}  {:.0}%", p * 100.0)).size(11.0))
                    .fill(if Some(class) == best { theme::accent() } else { theme::bg_widget_active() });
                ui.add(bar).on_hover_text(if acts { "Acting on this class is on" } else { "Not acted on" });
            }
        }

        if !doc.notes.is_empty() {
            ui.add_space(10.0);
            egui::CollapsingHeader::new(RichText::new("WHAT THE IMPORT FOUND").size(10.5).color(theme::text_faint()))
                .default_open(true)
                .show(ui, |ui| {
                    for note in &doc.notes {
                        ui.add(
                            egui::Label::new(RichText::new(format!("•  {note}")).size(11.5).color(theme::text_dim()))
                                .wrap(),
                        );
                    }
                });
        }
    }
}

fn empty_state(ui: &mut Ui, events: &mut Vec<MagicEvent>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.18);
        ui.label(RichText::new(icon::MAGIC_WAND).size(46.0).color(theme::accent()));
        ui.label(RichText::new("Magic").size(22.0).strong());
        ui.add_space(4.0);
        ui.set_max_width(ui.available_width().min(460.0));
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Prepare small AI vision models for your macros: import one, set up what part of the screen it \
                     looks at and what to do with its answers, try it live, and use it in the Macro tab.",
                )
                .color(theme::text_dim()),
            )
            .wrap(),
        );
        ui.add_space(12.0);
        let buttons = |ui: &mut Ui, events: &mut Vec<MagicEvent>| {
            let folder = format!("{}  Import a project folder…", icon::FOLDER_OPEN);
            if widgets::accent_button(ui, folder, theme::accent(), vec2(0.0, 32.0)).clicked() {
                events.push(MagicEvent::Import { folder: true });
            }
            let file = egui::Button::new(format!("{}  Import a .zip or .onnx…", icon::FILE_ZIP));
            if ui.add(file.min_size(vec2(0.0, 32.0))).clicked() {
                events.push(MagicEvent::Import { folder: false });
            }
            let open = egui::Button::new(format!("{}  Open .magic…", icon::FOLDER_OPEN));
            if ui.add(open.min_size(vec2(0.0, 32.0))).clicked() {
                events.push(MagicEvent::Open);
            }
        };
        // Side by side when they fit, else one under the other.
        if ui.available_width() >= 560.0 {
            ui.horizontal(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.add_space((ui.available_width() - 520.0).max(0.0) / 2.0);
                buttons(ui, events);
            });
        } else {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            buttons(ui, events);
        }
        ui.add_space(10.0);
        widgets::note(ui, "Training and fine-tuning your own models here is coming. PyTorch, which they need, can already be installed under Settings › AI.");
    });
}

/// The configuration, card by card.
fn editor(ui: &mut Ui, doc: &mut MagicDoc, env: &MagicEnv<'_>, events: &mut Vec<MagicEvent>) {
    let size_mb = doc.file.onnx.len() as f64 / (1024.0 * 1024.0);
    let classes = doc.shape.as_ref().and_then(|s| s.classes());
    let model = &mut doc.file.model;

    card(ui, "Model", |ui| {
        ui.horizontal(|ui| {
            widgets::caption(ui, "Name");
            ui.add(egui::TextEdit::singleline(&mut model.name).desired_width(f32::INFINITY));
        });
        ui.add(
            egui::TextEdit::multiline(&mut model.description)
                .hint_text("What it does")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            widgets::caption(ui, "Author");
            ui.add(egui::TextEdit::singleline(&mut model.author).desired_width(f32::INFINITY));
        });
        let facts = if size_mb > 0.0 {
            format!(
                "Reads {}×{} pictures · {} answers · {size_mb:.1} MB",
                model.input.width,
                model.input.height,
                classes.map_or_else(|| "?".to_owned(), |n| n.to_string())
            )
        } else {
            "No model file yet".to_owned()
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(facts).size(11.5).color(theme::text_dim()));
            if ui.small_button(format!("{}  Replace model file…", icon::ARROWS_CLOCKWISE)).clicked() {
                events.push(MagicEvent::ReplaceModel);
            }
        });
        if !model.source.is_empty() {
            ui.label(RichText::new(&model.source).size(11.0).color(theme::text_faint()));
        }
        if !model.license.is_empty() {
            ui.label(RichText::new(format!("Licence: {}", model.license)).size(11.0).color(theme::text_faint()));
        }
    });
    ui.add_space(6.0);

    card(ui, "Screen area", |ui| capture_card(ui, model, env, events));
    ui.add_space(6.0);
    card(ui, "Answers", |ui| classes_card(ui, model));
    ui.add_space(6.0);
    card(ui, "When it acts", |ui| {
        ui.horizontal(|ui| {
            widgets::fixed_label(ui, RichText::new("Press").color(theme::text_dim()), 120.0);
            if let Some(picked) = widgets::input_picker(ui, "magic-key", model.action.key, true) {
                model.action.key = picked;
            }
        });
        ui.horizontal(|ui| {
            widgets::fixed_label(ui, RichText::new("Hold for").color(theme::text_dim()), 120.0);
            widgets::duration_edit(ui, &mut model.action.hold_ms);
        });
        ui.horizontal(|ui| {
            widgets::fixed_label(ui, RichText::new("Then ignore it for").color(theme::text_dim()), 120.0)
                .on_hover_text("So one event is not acted on twice");
            widgets::duration_edit(ui, &mut model.action.cooldown_ms);
        });
    });
    ui.add_space(6.0);
    card(ui, "Adjustable settings", |ui| settings_card(ui, model));
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("PICTURE PREPARATION").size(10.5).color(theme::text_faint()))
        .default_open(false)
        .show(ui, |ui| input_card(ui, model));
    ui.add_space(6.0);
    let data = (doc.file.model.training.len(), doc.file.model.finetune.len());
    let model = &mut doc.file.model;
    egui::CollapsingHeader::new(RichText::new("TRAINING DATA").size(10.5).color(theme::text_faint()))
        .default_open(false)
        .show(ui, |ui| data_card(ui, model, data, events));
    ui.add_space(6.0);
    card(ui, "The .magic file includes", |ui| {
        let include = &mut model.include;
        ui.checkbox(&mut include.model, "The model").on_hover_text("Without it, the file is only a configuration");
        ui.checkbox(&mut include.blocks, "Blocks to use it with");
        ui.checkbox(&mut include.training, format!("Training pictures ({})", data.0));
        ui.checkbox(&mut include.finetune, format!("Fine-tuning pictures ({})", data.1));
        widgets::note(ui, "Macros keep the model and its configuration, never the pictures.");
    });
}

fn capture_card(ui: &mut Ui, model: &mut MagicModel, env: &MagicEnv<'_>, events: &mut Vec<MagicEvent>) {
    let mut centred = matches!(model.capture.area, CaptureArea::Center { .. });
    if widgets::segmented(ui, &mut centred, &[(true, "Middle of the display"), (false, "A fixed area")]) {
        model.capture.area = if centred {
            CaptureArea::Center { height_share: f64::from(model.input.height) / 1080.0 }
        } else {
            let rect = model.capture.rect(env.displays, None).unwrap_or(Rect::new(
                0,
                0,
                model.input.width,
                model.input.height,
            ));
            CaptureArea::Region { rect: env.displays.rect_to_local(rect) }
        };
    }
    match &mut model.capture.area {
        CaptureArea::Center { height_share } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("A square of").color(theme::text_dim()));
                let mut pixels = *height_share * 1080.0;
                if ui.add(DragValue::new(&mut pixels).range(8.0..=1080.0).max_decimals(0).suffix(" px")).changed() {
                    *height_share = pixels / 1080.0;
                }
                ui.label(RichText::new("on a 1080-pixel-high display").color(theme::text_dim()));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("On").color(theme::text_dim()));
                let mut display = Some(model.capture.display);
                if widgets::display_picker(ui, "magic-display", &mut display, env.displays, false) {
                    model.capture.display = display.unwrap_or(0);
                }
            });
            if let Some(rect) = model.capture.rect(env.displays, None) {
                widgets::note(
                    ui,
                    format!(
                        "Here: {}×{} pixels at {}, {}. It grows and shrinks with the display.",
                        rect.w, rect.h, rect.x, rect.y
                    ),
                );
            }
        }
        CaptureArea::Region { rect } => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add(DragValue::new(&mut rect.x).prefix("X "));
                ui.add(DragValue::new(&mut rect.y).prefix("Y "));
                ui.add(DragValue::new(&mut rect.w).range(1..=100_000).prefix("W "));
                ui.add(DragValue::new(&mut rect.h).range(1..=100_000).prefix("H "));
                let mut display = rect.display;
                if widgets::display_picker(ui, "magic-area-display", &mut display, env.displays, true) {
                    *rect = widgets::rect_on(*rect, display, env.displays);
                }
                if ui.button(format!("{}  Select area…", icon::SELECTION)).clicked() {
                    events.push(MagicEvent::PickArea);
                }
            });
        }
    }
}

fn classes_card(ui: &mut Ui, model: &mut MagicModel) {
    widgets::note(ui, "What each answer of the model means, and whether to act on it.");
    if model.classes.is_empty() {
        ui.weak("The model's answers are not known yet.");
    }
    egui::Grid::new("magic-classes").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label(RichText::new("#").size(11.0).color(theme::text_faint()));
        ui.label(RichText::new("Name").size(11.0).color(theme::text_faint()));
        ui.label(RichText::new("Act").size(11.0).color(theme::text_faint()));
        ui.label(RichText::new("Sure enough at · then wait").size(11.0).color(theme::text_faint()));
        ui.end_row();
        for (index, class) in model.classes.iter_mut().enumerate() {
            ui.label(RichText::new(index.to_string()).color(theme::text_faint()));
            // Sized outright: a grid cell only offers the width its column had last frame.
            ui.add_sized([200.0, 22.0], egui::TextEdit::singleline(&mut class.label));
            ui.checkbox(&mut class.act, "").on_hover_text("Act when the model gives this answer");
            ui.horizontal(|ui| {
                ui.add_enabled_ui(class.act, |ui| {
                    let mut percent = class.threshold * 100.0;
                    if ui.add(DragValue::new(&mut percent).range(0.0..=100.0).max_decimals(0).suffix("%")).changed() {
                        class.threshold = percent / 100.0;
                    }
                    widgets::duration_edit(ui, &mut class.delay_ms);
                });
            });
            ui.end_row();
        }
    });
    widgets::button_row(ui, |ui| {
        if ui.small_button(format!("{}  Add answer", icon::PLUS)).clicked() {
            let n = model.classes.len();
            model.classes.push(ClassSpec { label: format!("Class {n}"), threshold: 0.5, ..ClassSpec::default() });
        }
        if !model.classes.is_empty() && ui.small_button(format!("{}  Remove the last", icon::MINUS)).clicked() {
            model.classes.pop();
        }
    });
}

fn settings_card(ui: &mut Ui, model: &mut MagicModel) {
    widgets::note(
        ui,
        "Sliders for the values worth adjusting. Each changes one or more of the values above, and every block \
         that uses the model can set them its own way.",
    );
    let labels: Vec<String> = model.classes.iter().map(|c| c.label.clone()).collect();
    let mut remove = None;
    for (index, setting) in model.settings.iter_mut().enumerate() {
        ui.push_id(("setting", index), |ui| {
            egui::Frame::new().fill(theme::bg_raised()).corner_radius(theme::RADIUS).inner_margin(6).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&setting.label).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, icon::TRASH, "Remove this setting").clicked() {
                            remove = Some(index);
                        }
                        if widgets::reset_button(ui, setting.value != setting.default) {
                            setting.value = setting.default;
                        }
                    });
                });
                let suffix = match setting.kind {
                    SettingKind::Percent => "%",
                    SettingKind::Milliseconds => " ms",
                };
                let (min, max) = (setting.min.min(setting.max), setting.max.max(setting.min));
                ui.add(egui::Slider::new(&mut setting.value, min..=max).max_decimals(0).suffix(suffix));
                egui::CollapsingHeader::new(RichText::new("Change what it does").size(11.0).color(theme::text_faint()))
                    .id_salt(("edit", index))
                    .show(ui, |ui| edit_setting(ui, setting, &labels));
            });
        });
        ui.add_space(3.0);
    }
    if let Some(index) = remove {
        model.settings.remove(index);
    }
    if ui.button(format!("{}  Add a setting", icon::PLUS)).clicked() {
        let n = model.settings.len() + 1;
        model.settings.push(ModelSetting {
            key: format!("setting_{n}"),
            label: format!("Setting {n}"),
            kind: SettingKind::Percent,
            min: 0.0,
            max: 100.0,
            default: 50.0,
            value: 50.0,
            targets: Vec::new(),
            ..ModelSetting::default()
        });
    }
    // The values above follow the sliders.
    *model = model.configured(&serde_json::Map::new());
}

fn edit_setting(ui: &mut Ui, setting: &mut ModelSetting, labels: &[String]) {
    ui.horizontal(|ui| {
        widgets::caption(ui, "Name");
        ui.add(egui::TextEdit::singleline(&mut setting.label).desired_width(f32::INFINITY));
    });
    ui.horizontal(|ui| {
        widgets::caption(ui, "Kind");
        let mut kind = setting.kind;
        if widgets::segmented(
            ui,
            &mut kind,
            &[(SettingKind::Percent, "Confidence %"), (SettingKind::Milliseconds, "Time (ms)")],
        ) {
            setting.kind = kind;
            setting.targets.clear();
        }
    });
    ui.horizontal(|ui| {
        widgets::caption(ui, "From");
        ui.add(DragValue::new(&mut setting.min).max_decimals(0));
        widgets::caption(ui, "to");
        ui.add(DragValue::new(&mut setting.max).max_decimals(0));
        widgets::caption(ui, "default");
        ui.add(DragValue::new(&mut setting.default).max_decimals(0));
    });
    widgets::caption(ui, "Changes");
    let mut toggle = |ui: &mut Ui, target: SettingTarget, text: String| {
        let mut on = setting.targets.contains(&target);
        if ui.checkbox(&mut on, text).changed() {
            if on {
                setting.targets.push(target);
            } else {
                setting.targets.retain(|t| *t != target);
            }
        }
    };
    match setting.kind {
        SettingKind::Percent => {
            for (class, label) in labels.iter().enumerate() {
                toggle(ui, SettingTarget::Threshold { class }, format!("How sure “{label}” must be"));
            }
        }
        SettingKind::Milliseconds => {
            for (class, label) in labels.iter().enumerate() {
                toggle(ui, SettingTarget::Delay { class }, format!("The wait before acting on “{label}”"));
            }
            toggle(ui, SettingTarget::Cooldown, "The time ignored after acting".into());
            toggle(ui, SettingTarget::Hold, "How long the key is held".into());
        }
    }
}

fn input_card(ui: &mut Ui, model: &mut MagicModel) {
    let input = &mut model.input;
    widgets::note(
        ui,
        "How a picture of the screen is turned into what the model reads. Importing works this out; change it only if you know the model needs something else.",
    );
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, RichText::new("Size").color(theme::text_dim()), 110.0);
        ui.add(DragValue::new(&mut input.width).range(1..=4096).suffix(" px"));
        ui.label("×");
        ui.add(DragValue::new(&mut input.height).range(1..=4096).suffix(" px"));
    });
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, RichText::new("Layout").color(theme::text_dim()), 110.0);
        widgets::segmented(ui, &mut input.layout, &[(Layout::Nchw, "Channels first"), (Layout::Nhwc, "Channels last")]);
    });
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, RichText::new("Colors").color(theme::text_dim()), 110.0);
        widgets::segmented(ui, &mut input.color, &[(ColorOrder::Rgb, "RGB"), (ColorOrder::Bgr, "BGR")]);
    });
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, RichText::new("Normalisation").color(theme::text_dim()), 110.0);
        let current = NORMALISATIONS.iter().find(|(_, m, s)| *m == input.mean && *s == input.std).map(|(n, ..)| *n);
        egui::ComboBox::from_id_salt("normalisation").selected_text(current.unwrap_or("Custom")).show_ui(ui, |ui| {
            for (name, mean, std) in NORMALISATIONS {
                if ui.selectable_label(current == Some(name), name).clicked() {
                    (input.mean, input.std) = (mean, std);
                }
            }
        });
    });
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, RichText::new("Mean").color(theme::text_dim()), 110.0);
        for value in &mut input.mean {
            ui.add(DragValue::new(value).speed(0.001).range(-10.0..=10.0).max_decimals(4));
        }
    });
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, RichText::new("Spread").color(theme::text_dim()), 110.0);
        for value in &mut input.std {
            ui.add(DragValue::new(value).speed(0.001).range(0.0001..=10.0).max_decimals(4));
        }
    });
    ui.checkbox(&mut input.softmax, "Turn the model's scores into probabilities")
        .on_hover_text("On for most classifiers. Off when the model already gives probabilities.");
}

fn data_card(ui: &mut Ui, model: &mut MagicModel, counts: (usize, usize), events: &mut Vec<MagicEvent>) {
    widgets::note(
        ui,
        "Pictures that show what each answer looks like. They are kept in the .magic file for training and \
         fine-tuning, which are coming to this tab.",
    );
    for (finetune, title, count) in [(false, "Training pictures", counts.0), (true, "Fine-tuning pictures", counts.1)] {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{title}: {count}")).color(theme::text_dim()));
            ui.menu_button(format!("{}  Add…", icon::PLUS), |ui| {
                ui.label(RichText::new("Pictures of").size(11.0).color(theme::text_faint()));
                for (class, spec) in model.classes.iter().enumerate() {
                    if ui.button(&spec.label).clicked() {
                        events.push(MagicEvent::AddData { finetune, class });
                        ui.close();
                    }
                }
            });
            let items = if finetune { &mut model.finetune } else { &mut model.training };
            if !items.is_empty() && ui.small_button("Remove all").clicked() {
                items.clear();
            }
        });
    }
    let per_class: Vec<String> = model
        .classes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let n = model.training.iter().chain(&model.finetune).filter(|d| d.class == i).count();
            format!("{}: {n}", c.label)
        })
        .collect();
    if counts.0 + counts.1 > 0 {
        widgets::note(ui, per_class.join(" · "));
    }
}
