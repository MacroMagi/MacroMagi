//! Editors for blocks that look or listen for several things at once: the
//! images of an Image Detection block and the sounds of a Sound Recognition
//! block. Each item can belong to a named group; image groups share a search
//! area and sound groups an accuracy. Items and groups live in the block's
//! `images`/`sounds` and `groups` settings.

use egui::{RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::geom::Rect;
use magi_core::graph::NodeId;
use magi_core::nodes::sound::{SoundGroup, SoundTarget};
use magi_core::nodes::vision::{FindImageParams, ImageGroup, ImageTarget};
use magi_core::project::Project;

use super::images::{ImageCache, fit_size};
use super::widgets;
use super::{SnipMode, SnipTarget, UiRequest};
use crate::theme;

/// A short description of a search area.
fn area_line(region: Option<Rect>) -> String {
    match region {
        Some(r) => match r.display {
            Some(d) if d > 0 => format!("{}×{} at {}, {} (display {})", r.w, r.h, r.x, r.y, d + 1),
            _ => format!("{}×{} at {}, {}", r.w, r.h, r.x, r.y),
        },
        None => "the block's area".to_owned(),
    }
}

/// A unique group name.
fn new_group_name(taken: &[String]) -> String {
    (1..).map(|n| format!("Group {n}")).find(|name| !taken.contains(name)).expect("an unbounded range")
}

/// The group picker of one item. Returns the chosen group when it changes.
fn group_picker(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    groups: &[String],
) -> Option<String> {
    let mut picked = None;
    let shown = if current.is_empty() { "No group".to_owned() } else { current.to_owned() };
    egui::ComboBox::from_id_salt(id).width(96.0).selected_text(shown).truncate().show_ui(ui, |ui| {
        if ui.selectable_label(current.is_empty(), "No group").clicked() {
            picked = Some(String::new());
        }
        for name in groups {
            if ui.selectable_label(current == name, name).clicked() {
                picked = Some(name.clone());
            }
        }
    });
    picked.filter(|p| p != current)
}

/// The images of an Image Detection block, with their groups.
pub fn edit_images(
    ui: &mut Ui,
    project: &mut Project,
    node_id: NodeId,
    images: &mut ImageCache,
    requests: &mut Vec<UiRequest>,
) {
    let Some(node) = project.graph.node(node_id) else { return };
    let params: FindImageParams = node.params_or_default();
    // A block from before several images were possible has its one image moved into the list.
    let mut targets = params.targets();
    let mut groups = params.groups.clone();
    let mut changed = !params.image.is_empty();
    let names: Vec<String> = groups.iter().map(|g| g.name.clone()).collect();
    let (mut remove, mut swap) = (None, None);
    let count = targets.len();

    if targets.is_empty() {
        ui.weak("No images yet. Capture one from the screen or load a file.");
    }
    for (index, target) in targets.iter_mut().enumerate() {
        ui.push_id(("image", index), |ui| {
            egui::Frame::new().fill(theme::bg_raised()).corner_radius(theme::RADIUS).inner_margin(6).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if let Some(texture) = images.get(ui.ctx(), &project.assets, &target.image) {
                        let size = fit_size(&texture, vec2(46.0, 30.0), 4.0);
                        ui.add_sized([48.0, 32.0], egui::Image::new((texture.id(), size)));
                    } else {
                        ui.add_sized([48.0, 32.0], egui::Label::new(RichText::new(icon::IMAGE_BROKEN).size(18.0)));
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let mut name = project.assets.get(&target.image).map(|a| a.name.clone()).unwrap_or_default();
                        let field = egui::TextEdit::singleline(&mut name).desired_width(ui.available_width() - 4.0);
                        if ui.add(field).on_hover_text("The image's name").changed()
                            && let Some(asset) = project.assets.get_mut(&target.image)
                        {
                            asset.name = name;
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            if let Some(group) = group_picker(ui, ("group", index), &target.group, &names) {
                                target.group = group;
                                changed = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if widgets::icon_button(ui, icon::TRASH, "Stop looking for this image").clicked() {
                                    remove = Some(index);
                                }
                                if index + 1 < count
                                    && widgets::icon_button(ui, icon::ARROW_DOWN, "Lower priority").clicked()
                                {
                                    swap = Some((index, index + 1));
                                }
                                if index > 0 && widgets::icon_button(ui, icon::ARROW_UP, "Higher priority").clicked() {
                                    swap = Some((index, index - 1));
                                }
                            });
                        });
                    });
                });
                // Its own search area.
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let mut own = target.region.is_some();
                    if ui
                        .checkbox(&mut own, "Own search area")
                        .on_hover_text("Look for this image somewhere else than its group or the block")
                        .changed()
                    {
                        if !own {
                            target.region = None;
                            changed = true;
                        } else {
                            requests.push(UiRequest::Snip(SnipMode::Region, SnipTarget::ImageRegion(node_id, index)));
                        }
                    }
                    if let Some(region) = target.region {
                        ui.label(RichText::new(area_line(Some(region))).size(11.0).color(theme::text_dim()));
                        if ui.small_button(format!("{}  Change", icon::SELECTION)).clicked() {
                            requests.push(UiRequest::Snip(SnipMode::Region, SnipTarget::ImageRegion(node_id, index)));
                        }
                    }
                });
            });
        });
        ui.add_space(3.0);
    }

    ui.horizontal_wrapped(|ui| {
        // A button that does not fit moves to the next line whole.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        if ui
            .button(format!("{}  Capture", icon::CAMERA))
            .on_hover_text("Drag a box around another thing to look for")
            .clicked()
        {
            requests.push(UiRequest::Snip(SnipMode::Image, SnipTarget::AddImage(node_id)));
        }
        if ui.button(format!("{}  Load file…", icon::FOLDER_OPEN)).clicked() {
            requests.push(UiRequest::AddImageFile(node_id));
        }
        let existing: Vec<(String, String)> = project
            .assets
            .of_kind(magi_core::assets::AssetKind::Image)
            .filter(|a| !targets.iter().any(|t| t.image == a.id))
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        if !existing.is_empty() {
            ui.menu_button(format!("{}  Existing", icon::PLUS), |ui| {
                for (id, name) in &existing {
                    if ui.button(name).clicked() {
                        targets.push(ImageTarget { image: id.clone(), ..ImageTarget::default() });
                        changed = true;
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Another image of this macro");
        }
    });

    // Groups.
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        widgets::caption_with_help(
            ui,
            "Groups",
            "Images in a group share the group's search area instead of the block's.",
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icon::PLUS, "Add a group").clicked() {
                groups.push(ImageGroup { name: new_group_name(&names), region: None });
                changed = true;
            }
        });
    });
    let mut remove_group = None;
    for (index, group) in groups.iter_mut().enumerate() {
        ui.push_id(("group", index), |ui| {
            let old = group.name.clone();
            let (field, delete) = widgets::field_with_trailing(ui, egui::TextEdit::singleline(&mut group.name), |ui| {
                widgets::icon_button(ui, icon::TRASH, "Remove the group (its images stay)").clicked()
            });
            if field.changed() {
                for target in targets.iter_mut().filter(|t| t.group == old) {
                    target.group = group.name.clone();
                }
                changed = true;
            }
            if delete {
                remove_group = Some(index);
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let members = targets.iter().filter(|t| t.group == group.name).count();
                ui.label(
                    RichText::new(format!("{members} images · searches {}", area_line(group.region)))
                        .size(11.0)
                        .color(theme::text_dim()),
                );
                if ui.small_button(format!("{}  Area…", icon::SELECTION)).clicked() {
                    requests.push(UiRequest::Snip(SnipMode::Region, SnipTarget::ImageGroupRegion(node_id, index)));
                }
                if group.region.is_some() && ui.small_button("Block's area").clicked() {
                    group.region = None;
                    changed = true;
                }
            });
        });
    }
    if let Some(index) = remove_group {
        let name = groups.remove(index).name;
        for target in targets.iter_mut().filter(|t| t.group == name) {
            target.group.clear();
        }
        changed = true;
    }
    if let Some((a, b)) = swap {
        targets.swap(a, b);
        changed = true;
    }
    if let Some(index) = remove {
        targets.remove(index);
        changed = true;
    }
    if changed && let Some(node) = project.graph.node_mut(node_id) {
        node.set_param("images", &targets);
        node.set_param("groups", &groups);
        node.set_param("image", "");
    }
}

/// The sounds of a Sound Recognition block, with their groups.
pub fn edit_sounds(ui: &mut Ui, project: &mut Project, node_id: NodeId, requests: &mut Vec<UiRequest>) {
    let Some(node) = project.graph.node(node_id) else { return };
    let params: magi_core::nodes::sound::SoundMatchParams = node.params_or_default();
    let mut targets = params.targets();
    let mut groups = params.groups.clone();
    let mut changed = !params.sound.is_empty();
    let names: Vec<String> = groups.iter().map(|g| g.name.clone()).collect();
    let (mut remove, mut swap) = (None, None);
    let count = targets.len();

    if targets.is_empty() {
        ui.weak("No sounds yet. Load a sound file to listen for.");
    }
    for (index, target) in targets.iter_mut().enumerate() {
        ui.push_id(("sound", index), |ui| {
            egui::Frame::new().fill(theme::bg_raised()).corner_radius(theme::RADIUS).inner_margin(6).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(icon::WAVEFORM).size(16.0).color(theme::text_dim()));
                    let mut name = project.assets.get(&target.sound).map(|a| a.name.clone()).unwrap_or_default();
                    let field = egui::TextEdit::singleline(&mut name).desired_width(ui.available_width() - 4.0);
                    if ui.add(field).on_hover_text("The sound's name").changed()
                        && let Some(asset) = project.assets.get_mut(&target.sound)
                    {
                        asset.name = name;
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if let Some(group) = group_picker(ui, ("group", index), &target.group, &names) {
                        target.group = group;
                        changed = true;
                    }
                    if let Some(seconds) = project.assets.audio(&target.sound).and_then(|wav| wav_seconds(wav)) {
                        ui.label(RichText::new(format!("{seconds:.1} s")).size(11.0).color(theme::text_faint()));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, icon::TRASH, "Stop listening for this sound").clicked() {
                            remove = Some(index);
                        }
                        if index + 1 < count && widgets::icon_button(ui, icon::ARROW_DOWN, "Lower priority").clicked() {
                            swap = Some((index, index + 1));
                        }
                        if index > 0 && widgets::icon_button(ui, icon::ARROW_UP, "Higher priority").clicked() {
                            swap = Some((index, index - 1));
                        }
                    });
                });
            });
        });
        ui.add_space(3.0);
    }
    ui.horizontal_wrapped(|ui| {
        // A button that does not fit moves to the next line whole.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        if ui
            .button(format!("{}  Load sound file…", icon::FOLDER_OPEN))
            .on_hover_text("A WAV, MP3, OGG or FLAC file with a sound to listen for")
            .clicked()
        {
            requests.push(UiRequest::AddSoundFile(node_id));
        }
        let existing: Vec<(String, String)> = project
            .assets
            .of_kind(magi_core::assets::AssetKind::Audio)
            .filter(|a| !targets.iter().any(|t| t.sound == a.id))
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        if !existing.is_empty() {
            ui.menu_button(format!("{}  Existing", icon::PLUS), |ui| {
                for (id, name) in &existing {
                    if ui.button(name).clicked() {
                        targets.push(SoundTarget { sound: id.clone(), ..SoundTarget::default() });
                        changed = true;
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Another sound of this macro");
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        widgets::caption_with_help(ui, "Groups", "Sounds in a group can share an accuracy of their own.");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icon::PLUS, "Add a group").clicked() {
                groups.push(SoundGroup { name: new_group_name(&names), accuracy: None });
                changed = true;
            }
        });
    });
    let mut remove_group = None;
    for (index, group) in groups.iter_mut().enumerate() {
        ui.push_id(("group", index), |ui| {
            let old = group.name.clone();
            let (field, delete) = widgets::field_with_trailing(ui, egui::TextEdit::singleline(&mut group.name), |ui| {
                widgets::icon_button(ui, icon::TRASH, "Remove the group (its sounds stay)").clicked()
            });
            if field.changed() {
                for target in targets.iter_mut().filter(|t| t.group == old) {
                    target.group = group.name.clone();
                }
                changed = true;
            }
            if delete {
                remove_group = Some(index);
            }
            ui.horizontal(|ui| {
                let mut own = group.accuracy.is_some();
                if ui.checkbox(&mut own, "Own accuracy").changed() {
                    group.accuracy = own.then_some(params.accuracy);
                    changed = true;
                }
                if let Some(accuracy) = &mut group.accuracy {
                    let mut percent = *accuracy * 100.0;
                    if ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).fixed_decimals(0).suffix("%")).changed() {
                        *accuracy = (percent / 100.0).clamp(0.0, 1.0);
                        changed = true;
                    }
                }
            });
        });
    }
    if let Some(index) = remove_group {
        let name = groups.remove(index).name;
        for target in targets.iter_mut().filter(|t| t.group == name) {
            target.group.clear();
        }
        changed = true;
    }
    if let Some((a, b)) = swap {
        targets.swap(a, b);
        changed = true;
    }
    if let Some(index) = remove {
        targets.remove(index);
        changed = true;
    }
    if changed && let Some(node) = project.graph.node_mut(node_id) {
        node.set_param("sounds", &targets);
        node.set_param("groups", &groups);
        node.set_param("sound", "");
    }
}

/// How long a stored sound (a 16-bit mono WAV) lasts, read from its header.
fn wav_seconds(wav: &[u8]) -> Option<f64> {
    let rate = u32::from_le_bytes(wav.get(24..28)?.try_into().ok()?);
    let data = u32::from_le_bytes(wav.get(40..44)?.try_into().ok()?);
    (rate > 0).then(|| f64::from(data) / 2.0 / f64::from(rate))
}
