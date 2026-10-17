//! Editors for blocks that look or listen for several things at once: the
//! images of an Image Detection block and the sounds of a Sound Recognition
//! block. Groups and items are one list: each group shows the items in it,
//! and the items in no group come last. Items are dragged by their handle
//! onto a group to put them in it. Image groups share a search area and
//! sound groups an accuracy. Items and groups live in the block's
//! `images`/`sounds` and `groups` settings.

use egui::{RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::geom::Rect;
use magi_core::graph::NodeId;
use magi_core::nodes::Registry;
use magi_core::nodes::sound::{SoundGroup, SoundMatchParams, SoundTarget};
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

/// An item of a list, which may be in a group.
trait Member {
    fn group(&self) -> &str;
    fn group_mut(&mut self) -> &mut String;
}

impl Member for ImageTarget {
    fn group(&self) -> &str {
        &self.group
    }
    fn group_mut(&mut self) -> &mut String {
        &mut self.group
    }
}

impl Member for SoundTarget {
    fn group(&self) -> &str {
        &self.group
    }
    fn group_mut(&mut self) -> &mut String {
        &mut self.group
    }
}

trait Named {
    fn name_mut(&mut self) -> &mut String;
}

impl Named for ImageGroup {
    fn name_mut(&mut self) -> &mut String {
        &mut self.name
    }
}

impl Named for SoundGroup {
    fn name_mut(&mut self) -> &mut String {
        &mut self.name
    }
}

/// What a row asked for.
#[derive(Default)]
struct Row {
    changed: bool,
    remove: bool,
    up: bool,
    down: bool,
}

/// One list of groups and the items in them, then the items in no group.
/// `item` draws an item's row (after its drag handle); `group` draws a
/// group's own settings under its name. Returns whether anything changed.
fn grouped_list<T: Member, G: Named>(
    ui: &mut Ui,
    noun: &str,
    targets: &mut Vec<T>,
    groups: &mut Vec<G>,
    mut item: impl FnMut(&mut Ui, usize, &mut T) -> Row,
    mut group: impl FnMut(&mut Ui, usize, &mut G, usize) -> bool,
) -> bool {
    let mut acts = Acts::default();
    let (mut moved, mut remove_group) = (None, None);

    // The groups, each a place to drop items onto.
    for (index, entry) in groups.iter_mut().enumerate() {
        ui.push_id(("group", index), |ui| {
            let frame = egui::Frame::new()
                .stroke(egui::Stroke::new(1.0, theme::stroke_strong()))
                .corner_radius(theme::RADIUS)
                .inner_margin(6);
            let (_, dropped) = ui.dnd_drop_zone::<usize, ()>(frame, |ui| {
                ui.set_width(ui.available_width());
                let old = entry.name_mut().clone();
                let (field, delete) = ui
                    .horizontal(|ui| {
                        ui.label(RichText::new(icon::FOLDER_SIMPLE).color(theme::text_dim()));
                        widgets::field_with_trailing(ui, egui::TextEdit::singleline(entry.name_mut()), |ui| {
                            widgets::icon_button(ui, icon::TRASH, &format!("Remove the group (its {noun}s stay)"))
                                .clicked()
                        })
                    })
                    .inner;
                if field.changed() {
                    let new = entry.name_mut().clone();
                    for target in targets.iter_mut().filter(|t| t.group() == old) {
                        *target.group_mut() = new.clone();
                    }
                    acts.changed = true;
                }
                if delete {
                    remove_group = Some(index);
                }
                let name = entry.name_mut().clone();
                let count = targets.iter().filter(|t| t.group() == name).count();
                acts.changed |= group(ui, index, &mut *entry, count);
                ui.add_space(2.0);
                if rows(ui, noun, &name, targets, &mut item, &mut acts) == 0 {
                    ui.label(RichText::new(format!("Drag {noun}s here")).size(11.0).color(theme::text_faint()));
                }
            });
            if let Some(dragged) = dropped {
                moved = Some((*dragged, entry.name_mut().clone()));
            }
        });
        ui.add_space(4.0);
    }

    // The items in no group.
    if groups.is_empty() {
        rows(ui, noun, "", targets, &mut item, &mut acts);
    } else {
        let frame = egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 2));
        let (_, dropped) = ui.dnd_drop_zone::<usize, ()>(frame, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Not in a group").size(11.0).color(theme::text_faint()));
            if rows(ui, noun, "", targets, &mut item, &mut acts) == 0 {
                ui.label(
                    RichText::new(format!("Drag {noun}s here to take them out of their group"))
                        .size(11.0)
                        .color(theme::text_faint()),
                );
            }
        });
        if let Some(dragged) = dropped {
            moved = Some((*dragged, String::new()));
        }
    }

    let mut changed = acts.changed;
    if let Some((index, group)) = moved
        && let Some(target) = targets.get_mut(index)
        && target.group() != group
    {
        *target.group_mut() = group;
        changed = true;
    }
    if let Some(index) = remove_group {
        let name = groups.remove(index).name_mut().clone();
        for target in targets.iter_mut().filter(|t| t.group() == name) {
            target.group_mut().clear();
        }
        changed = true;
    }
    if let Some((a, b)) = acts.swap {
        targets.swap(a, b);
        changed = true;
    }
    if let Some(index) = acts.remove {
        targets.remove(index);
        changed = true;
    }
    changed
}

/// What the rows of a list asked for.
#[derive(Default)]
struct Acts {
    changed: bool,
    remove: Option<usize>,
    swap: Option<(usize, usize)>,
}

/// The rows of the items in one group (`section`; empty for no group), each
/// with its drag handle. Returns how many there are.
fn rows<T: Member>(
    ui: &mut Ui,
    noun: &str,
    section: &str,
    targets: &mut [T],
    item: &mut impl FnMut(&mut Ui, usize, &mut T) -> Row,
    acts: &mut Acts,
) -> usize {
    let members: Vec<usize> = (0..targets.len()).filter(|&i| targets[i].group() == section).collect();
    for (position, &index) in members.iter().enumerate() {
        ui.push_id(("item", index), |ui| {
            egui::Frame::new().fill(theme::bg_raised()).corner_radius(theme::RADIUS).inner_margin(6).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let handle = egui::Id::new(("drag", noun, index));
                    ui.dnd_drag_source(handle, index, |ui| {
                        ui.label(RichText::new(icon::DOTS_SIX_VERTICAL).size(16.0).color(theme::text_faint()))
                    })
                    .response
                    .on_hover_text(format!("Drag onto a group to put this {noun} in it"));
                    ui.vertical(|ui| {
                        let row = item(ui, index, &mut targets[index]);
                        acts.changed |= row.changed;
                        if row.remove {
                            acts.remove = Some(index);
                        }
                        if row.up && position > 0 {
                            acts.swap = Some((index, members[position - 1]));
                        }
                        if row.down && position + 1 < members.len() {
                            acts.swap = Some((index, members[position + 1]));
                        }
                    });
                });
            });
        });
        ui.add_space(3.0);
    }
    members.len()
}

/// The arrows and the bin at the end of an item's row.
fn row_buttons(ui: &mut Ui, row: &mut Row, noun: &str) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let hint = format!(
            "Delete this {noun}. It is also deleted from the macro unless another block uses it; files on your computer are kept."
        );
        if widgets::icon_button(ui, icon::TRASH, &hint).clicked() {
            row.remove = true;
        }
        if widgets::icon_button(ui, icon::ARROW_DOWN, "Lower priority").clicked() {
            row.down = true;
        }
        if widgets::icon_button(ui, icon::ARROW_UP, "Higher priority").clicked() {
            row.up = true;
        }
    });
}

/// The header of a list: a note, and the button that adds a group.
fn list_header(ui: &mut Ui, noun: &str, has_groups: bool) -> bool {
    let note = if has_groups {
        format!("Drag a {noun} by {} onto a group to put it in.", icon::DOTS_SIX_VERTICAL)
    } else {
        format!("Add a group to give several {noun}s settings of their own.")
    };
    ui.horizontal_top(|ui| {
        // The note wraps in the room the button leaves.
        let room = (ui.available_width() - 28.0).max(60.0);
        ui.allocate_ui(vec2(room, 0.0), |ui| {
            ui.add(egui::Label::new(RichText::new(note).size(11.0).color(theme::text_faint())).wrap());
        });
        widgets::icon_button(ui, icon::FOLDER_SIMPLE_PLUS, "Add a group").clicked()
    })
    .inner
}

/// The images of an Image Detection block, with their groups.
pub fn edit_images(
    ui: &mut Ui,
    project: &mut Project,
    registry: &Registry,
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

    if list_header(ui, "image", !groups.is_empty()) {
        let names: Vec<String> = groups.iter().map(|g| g.name.clone()).collect();
        groups.insert(0, ImageGroup { name: new_group_name(&names), region: None });
        changed = true;
    }
    if targets.is_empty() && groups.is_empty() {
        ui.weak("No images yet. Capture one from the screen or load a file.");
    }
    let assets = &mut project.assets;
    let (mut item_requests, mut group_requests) = (Vec::new(), Vec::new());
    changed |= grouped_list(
        ui,
        "image",
        &mut targets,
        &mut groups,
        |ui, index, target| {
            let mut row = Row::default();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if let Some(texture) = images.get(ui.ctx(), assets, &target.image) {
                    let size = fit_size(&texture, vec2(46.0, 30.0), 4.0);
                    ui.add_sized([48.0, 32.0], egui::Image::new((texture.id(), size)));
                } else {
                    ui.add_sized([48.0, 32.0], egui::Label::new(RichText::new(icon::IMAGE_BROKEN).size(18.0)));
                }
                let mut name = assets.get(&target.image).map(|a| a.name.clone()).unwrap_or_default();
                let field =
                    egui::TextEdit::singleline(&mut name).desired_width((ui.available_width() - 70.0).max(60.0));
                if ui.add(field).on_hover_text("The image's name").changed()
                    && let Some(asset) = assets.get_mut(&target.image)
                {
                    asset.name = name;
                }
                row_buttons(ui, &mut row, "image");
            });
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
                        row.changed = true;
                    } else {
                        item_requests.push(UiRequest::Snip(SnipMode::Region, SnipTarget::ImageRegion(node_id, index)));
                    }
                }
                if let Some(region) = target.region {
                    ui.label(RichText::new(area_line(Some(region))).size(11.0).color(theme::text_dim()));
                    if ui.small_button(format!("{}  Change", icon::SELECTION)).clicked() {
                        item_requests.push(UiRequest::Snip(SnipMode::Region, SnipTarget::ImageRegion(node_id, index)));
                    }
                }
            });
            row
        },
        |ui, index, group, members| {
            let mut changed = false;
            widgets::button_row(ui, |ui| {
                ui.label(
                    RichText::new(format!("{members} images · searches {}", area_line(group.region)))
                        .size(11.0)
                        .color(theme::text_dim()),
                );
                if ui.small_button(format!("{}  Area…", icon::SELECTION)).clicked() {
                    group_requests
                        .push(UiRequest::Snip(SnipMode::Region, SnipTarget::ImageGroupRegion(node_id, index)));
                }
                if group.region.is_some() && ui.small_button("Block's area").clicked() {
                    group.region = None;
                    changed = true;
                }
            });
            changed
        },
    );
    requests.extend(item_requests);
    requests.extend(group_requests);

    widgets::button_row(ui, |ui| {
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

    let before: Vec<String> = params.targets().into_iter().map(|t| t.image).collect();
    if changed && let Some(node) = project.graph.node_mut(node_id) {
        node.set_param("images", &targets);
        node.set_param("groups", &groups);
        node.set_param("image", "");
    }
    let kept: Vec<&str> = targets.iter().map(|t| t.image.as_str()).collect();
    delete_unused(project, registry, before.iter().filter(|id| !kept.contains(&id.as_str())));
}

/// The sounds of a Sound Recognition block, with their groups.
pub fn edit_sounds(
    ui: &mut Ui,
    project: &mut Project,
    registry: &Registry,
    node_id: NodeId,
    requests: &mut Vec<UiRequest>,
) {
    let Some(node) = project.graph.node(node_id) else { return };
    let params: SoundMatchParams = node.params_or_default();
    let mut targets = params.targets();
    let mut groups = params.groups.clone();
    let mut changed = !params.sound.is_empty();

    if list_header(ui, "sound", !groups.is_empty()) {
        let names: Vec<String> = groups.iter().map(|g| g.name.clone()).collect();
        groups.insert(0, SoundGroup { name: new_group_name(&names), accuracy: None });
        changed = true;
    }
    if targets.is_empty() && groups.is_empty() {
        ui.weak("No sounds yet. Load a sound file to listen for.");
    }
    let assets = &mut project.assets;
    changed |= grouped_list(
        ui,
        "sound",
        &mut targets,
        &mut groups,
        |ui, _, target| {
            let mut row = Row::default();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(icon::WAVEFORM).size(16.0).color(theme::text_dim()));
                let mut name = assets.get(&target.sound).map(|a| a.name.clone()).unwrap_or_default();
                let field =
                    egui::TextEdit::singleline(&mut name).desired_width((ui.available_width() - 70.0).max(60.0));
                if ui.add(field).on_hover_text("The sound's name").changed()
                    && let Some(asset) = assets.get_mut(&target.sound)
                {
                    asset.name = name;
                }
                row_buttons(ui, &mut row, "sound");
            });
            if let Some(seconds) = assets.audio(&target.sound).and_then(|wav| wav_seconds(wav)) {
                ui.label(RichText::new(format!("{seconds:.1} s")).size(11.0).color(theme::text_faint()));
            }
            row
        },
        |ui, _, group, members| {
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{members} sounds")).size(11.0).color(theme::text_dim()));
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
            changed
        },
    );

    widgets::button_row(ui, |ui| {
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

    let before: Vec<String> = params.targets().into_iter().map(|t| t.sound).collect();
    if changed && let Some(node) = project.graph.node_mut(node_id) {
        node.set_param("sounds", &targets);
        node.set_param("groups", &groups);
        node.set_param("sound", "");
    }
    let kept: Vec<&str> = targets.iter().map(|t| t.sound.as_str()).collect();
    delete_unused(project, registry, before.iter().filter(|id| !kept.contains(&id.as_str())));
}

/// Deletes images or sounds taken off a block from the macro, unless another
/// block still uses them. They are the macro's own copies – captured, or
/// copied in when a file was loaded – so no file on the computer is touched.
fn delete_unused<'a>(project: &mut Project, registry: &Registry, removed: impl Iterator<Item = &'a String>) {
    let removed: Vec<&String> = removed.collect();
    if removed.is_empty() {
        return;
    }
    let used = project.graph.referenced_assets(registry);
    for id in removed {
        if !used.contains(id) {
            project.assets.remove(id);
        }
    }
}

/// How long a stored sound (a 16-bit mono WAV) lasts, read from its header.
fn wav_seconds(wav: &[u8]) -> Option<f64> {
    let rate = u32::from_le_bytes(wav.get(24..28)?.try_into().ok()?);
    let data = u32::from_le_bytes(wav.get(40..44)?.try_into().ok()?);
    (rate > 0).then(|| f64::from(data) / 2.0 / f64::from(rate))
}
