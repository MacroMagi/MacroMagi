//! Importing a model made elsewhere: a bare `.onnx` file, or a project folder
//! (or `.zip`) that contains one.
//!
//! Besides the model, a project usually says in its files how the model is
//! meant to be used. The importer reads – never runs – those files and fills
//! in what it can recognise:
//!
//! * the class names, from a Markdown table with a "Class" column, or from a
//!   dictionary of numbered entries in code (`3: {"desc": "Hit", "hit": True}`),
//!   which may also say which classes call for action;
//! * the color normalisation, from `mean`/`std` lists of three numbers;
//! * the screen area, from a crop size given relative to a 1080-pixel screen;
//! * adjustable settings, from a `settings.json` of numbers, or else from
//!   typed declarations in code (`thresh: int = 60`, with named numbers looked
//!   up): `thresh…` keys become confidence thresholds, `delay…` keys delays,
//!   and a suffix such as `_frontier` limits a setting to the classes with
//!   that word in their name;
//! * the name, description and licence, from the README and LICENSE.
//!
//! Everything found is listed as a note, so the person importing can check it.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use magi_core::input::Key;
use magi_core::magic::{
    CaptureArea, CaptureSpec, ClassSpec, IMAGENET_MEAN, IMAGENET_STD, InputSpec, MagicFile, MagicModel, ModelSetting,
    SettingKind, SettingTarget,
};

/// Files larger than this are not read for hints (models excepted).
const MAX_TEXT: u64 = 2 * 1024 * 1024;
/// Models larger than this are not imported.
const MAX_MODEL: u64 = 512 * 1024 * 1024;
/// Folders never worth looking into.
const SKIP: [&str; 8] =
    [".git", "site-packages", "python-embed", "__pycache__", "node_modules", ".venv", "venv", "fonts"];

/// Words in a class name that suggest the class calls for action, and words that suggest it does not.
const ACT_WORDS: [&str; 8] = ["great", "perfect", "hit", "press", "now", "frontier", "success", "target"];
const IDLE_WORDS: [&str; 7] = ["none", "out", "miss", "fail", "nothing", "background", "idle"];

pub struct Imported {
    pub file: MagicFile,
    /// What was found, and what was guessed.
    pub notes: Vec<String>,
}

/// A file found while importing: its path relative to the import, and its bytes.
struct Found {
    path: String,
    bytes: Vec<u8>,
}

fn skipped(path: &str) -> bool {
    path.split(['/', '\\']).any(|part| SKIP.contains(&part.to_lowercase().as_str()))
}

fn interesting(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".onnx")
        || lower.ends_with(".md")
        || lower.ends_with(".json")
        || lower.ends_with(".py")
        || lower.ends_with(".txt")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with("license")
}

fn limit(path: &str) -> u64 {
    if path.to_lowercase().ends_with(".onnx") { MAX_MODEL } else { MAX_TEXT }
}

/// Every interesting file of a folder, a `.zip`, or a single model.
fn gather(path: &Path) -> Result<Vec<Found>, String> {
    let mut found = Vec::new();
    let lower = path.to_string_lossy().to_lowercase();
    if path.is_dir() {
        // A model inside a `models` folder: its project is the folder above.
        walk(path, path, 0, &mut found)?;
    } else if lower.ends_with(".zip") {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|_| "this is not a ZIP archive".to_owned())?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.into_owned();
            if entry.is_dir() || skipped(&name) || !interesting(&name) || entry.size() > limit(&name) {
                continue;
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            found.push(Found { path: name, bytes });
        }
    } else if lower.ends_with(".onnx") {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        found.push(Found {
            path: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            bytes,
        });
        // Hints next to the model, or in the project it sits in.
        if let Some(parent) = path.parent() {
            let project = if parent.file_name().is_some_and(|n| n.eq_ignore_ascii_case("models")) {
                parent.parent().unwrap_or(parent)
            } else {
                parent
            };
            let mut hints = Vec::new();
            walk(project, project, 0, &mut hints)?;
            found.extend(hints.into_iter().filter(|f| !f.path.to_lowercase().ends_with(".onnx")));
        }
    } else {
        return Err("choose an .onnx model, a folder or a .zip file".to_owned());
    }
    Ok(found)
}

fn walk(root: &Path, dir: &Path, depth: usize, found: &mut Vec<Found>) -> Result<(), String> {
    if depth > 4 {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        if skipped(&relative) {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, depth + 1, found)?;
        } else if interesting(&relative)
            && entry.metadata().is_ok_and(|m| m.len() <= limit(&relative))
            && let Ok(bytes) = std::fs::read(&path)
        {
            found.push(Found { path: relative, bytes });
        }
    }
    Ok(())
}

fn text(found: &Found) -> Option<&str> {
    std::str::from_utf8(&found.bytes).ok()
}

/// Imports a model and as much of its configuration as its files reveal.
pub fn import(path: &Path) -> Result<Imported, String> {
    let found = gather(path)?;
    let mut notes = Vec::new();
    let models: Vec<&Found> = found.iter().filter(|f| f.path.to_lowercase().ends_with(".onnx")).collect();
    let model = models.first().ok_or("no .onnx model was found here")?;
    if models.len() > 1 {
        notes.push(format!(
            "{} models were found; “{}” is used. Import another one by choosing its file.",
            models.len(),
            model.path
        ));
    }
    let shape = magi_ml::inspect(&model.bytes).map_err(|e| e.0)?;
    let mut magic = MagicModel { name: name_of(path), ..MagicModel::default() };
    magic.input = shape.suggested(InputSpec::default());
    notes.push(format!(
        "The model reads {}×{} pictures and gives {} answers.",
        magic.input.width,
        magic.input.height,
        shape.classes().map_or_else(|| "a number of".to_owned(), |n| n.to_string())
    ));
    magic.source = format!("Imported from {}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());

    let texts: Vec<(&str, &str)> = found.iter().filter_map(|f| Some((f.path.as_str(), text(f)?))).collect();

    // Name, description and licence.
    if let Some((_, readme)) = texts.iter().find(|(p, _)| p.to_lowercase().ends_with("readme.md")) {
        if let Some(title) = readme.lines().find_map(|l| l.strip_prefix("# ")) {
            magic.name = title.trim().to_owned();
        }
        if let Some(paragraph) = first_paragraph(readme) {
            magic.description = paragraph;
        }
    }
    if let Some((_, license)) = texts.iter().find(|(p, _)| {
        let name = p.rsplit('/').next().unwrap_or(p).to_lowercase();
        name == "license" || name.starts_with("license.")
    }) {
        let first = license.lines().map(str::trim).filter(|l| !l.is_empty()).take(2).collect::<Vec<_>>().join(" ");
        magic.license = first.clone();
        notes.push(format!("Licence: {first}. Keep its terms in mind before sharing the model."));
    }

    // Normalisation.
    let all_text: String = texts
        .iter()
        .filter(|(p, _)| !p.to_lowercase().ends_with(".md"))
        .map(|(_, t)| *t)
        .collect::<Vec<_>>()
        .join("\n");
    match (find_triple(&all_text, "mean"), find_triple(&all_text, "std")) {
        (Some(mean), Some(std)) => {
            magic.input.mean = mean;
            magic.input.std = std;
            notes.push(format!("Color normalisation found: mean {mean:?}, std {std:?}."));
        }
        _ => {
            magic.input.mean = IMAGENET_MEAN;
            magic.input.std = IMAGENET_STD;
            notes.push("No color normalisation was found; the common ImageNet one is used.".into());
        }
    }

    // Classes: names from a table, or from code, which may also say which act.
    let count = shape.classes().unwrap_or(0);
    let table = texts.iter().find_map(|(_, t)| class_table(t));
    let code =
        texts.iter().filter(|(p, _)| !p.to_lowercase().ends_with(".md")).find_map(|(p, t)| Some((*p, class_dict(t)?)));
    let labels: Vec<String> = match (&table, &code) {
        (Some(table), _) => table.clone(),
        (None, Some((_, entries))) => entries.iter().map(|(label, _)| capitalised(label)).collect(),
        (None, None) => Vec::new(),
    };
    let stated: Vec<Option<bool>> =
        code.as_ref().map(|(_, entries)| entries.iter().map(|(_, act)| *act).collect()).unwrap_or_default();
    let acts_stated = stated.iter().any(Option::is_some);
    magic.classes = (0..count.max(labels.len()))
        .map(|i| {
            let label = labels.get(i).cloned().unwrap_or_else(|| format!("Class {i}"));
            let lower = label.to_lowercase();
            let guessed = ACT_WORDS.iter().any(|w| lower.contains(w)) && !IDLE_WORDS.iter().any(|w| word_in(&lower, w));
            let act = stated.get(i).copied().flatten().unwrap_or(guessed && !acts_stated);
            ClassSpec { label, act, threshold: 0.5, delay_ms: 0.0 }
        })
        .collect();
    if labels.is_empty() {
        notes.push("No class names were found; name the classes yourself.".into());
    } else {
        let acting: Vec<&str> = magic.classes.iter().filter(|c| c.act).map(|c| c.label.as_str()).collect();
        let acting = if acting.is_empty() { "none".to_owned() } else { acting.join(", ") };
        notes.push(match (&table, &code) {
            (Some(_), _) => format!("{} class names found in a table.", labels.len()),
            (None, Some((path, _))) => format!("{} class names found in {path}.", labels.len()),
            (None, None) => unreachable!("labels come from one or the other"),
        });
        notes.push(match &code {
            Some((path, _)) if acts_stated => format!("{path} says these classes call for action: {acting}."),
            _ => format!("Guessed from their names, these classes call for action: {acting}. Check the classes."),
        });
    }

    // Screen area.
    // Sizes are taken to be meant for a 1080-pixel-high screen, and scaled with it.
    let crop = find_number_after(&all_text, "crop_size");
    let side = crop.unwrap_or(f64::from(magic.input.height));
    magic.capture = CaptureSpec { area: CaptureArea::Center { height_share: side / 1080.0 }, display: 0 };
    notes.push(format!(
        "The model looks at a square in the middle of the screen, {side:.0} pixels on a 1080-pixel-high display{}.",
        if crop.is_some() { " (found in the code)" } else { " (its input size; change it if needed)" }
    ));

    // Key and cooldown.
    let mentions_space = texts.iter().any(|(_, t)| t.to_lowercase().contains("space"));
    magic.action.key = magi_core::input::Button::Key(Key::Space);
    notes.push(if mentions_space {
        "The key to press is Space (the files mention it).".into()
    } else {
        "The key to press is Space; change it if the model needs another.".into()
    });

    // Settings.
    if let Some((settings_path, settings)) = texts.iter().find(|(p, _)| p.to_lowercase().ends_with("settings.json"))
        && let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(settings)
    {
        apply_settings(&mut magic, &map, &mut notes);
        notes.push(format!("Settings read from {settings_path}."));
    } else {
        let declared = declared_settings(&all_text);
        if !declared.is_empty() {
            let keys = declared.keys().cloned().collect::<Vec<_>>().join(", ");
            apply_settings(&mut magic, &declared, &mut notes);
            notes.push(format!("Settings found in the code: {keys}."));
        }
    }

    Ok(Imported { file: MagicFile { model: magic, onnx: model.bytes.clone(), ..MagicFile::default() }, notes })
}

fn name_of(path: &Path) -> String {
    path.file_stem().map(|n| n.to_string_lossy().replace(['_', '-'], " ")).unwrap_or_else(|| "Imported model".into())
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn word_in(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|w| w == word)
}

/// The first paragraph of a Markdown document that is plain text.
fn first_paragraph(markdown: &str) -> Option<String> {
    let mut paragraph = Vec::new();
    for line in markdown.lines().skip_while(|l| l.starts_with('#') || l.trim().is_empty()) {
        let line = line.trim();
        if line.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if line.starts_with(['#', '|', '!', '<', '>', '`', '-', '*']) {
            if paragraph.is_empty() {
                continue;
            }
            break;
        }
        paragraph.push(line.replace("**", "").replace('_', ""));
    }
    (!paragraph.is_empty()).then(|| paragraph.join(" "))
}

/// Class names from a Markdown table whose first column is "Class" (numbers)
/// and whose second column names them.
fn class_table(markdown: &str) -> Option<Vec<String>> {
    let mut lines = markdown.lines().map(str::trim);
    while let Some(line) = lines.next() {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() >= 2 && cells[0].eq_ignore_ascii_case("class") {
            let mut labels: Vec<(usize, String)> = Vec::new();
            for row in lines.by_ref() {
                if !row.starts_with('|') {
                    break;
                }
                let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
                if let (Some(id), Some(name)) = (cells.first().and_then(|c| c.parse::<usize>().ok()), cells.get(1)) {
                    labels.push((id, (*name).to_owned()));
                }
            }
            if labels.is_empty() {
                continue;
            }
            labels.sort_by_key(|(id, _)| *id);
            let size = labels.last().map_or(0, |(id, _)| id + 1);
            let mut out: Vec<String> = (0..size).map(|i| format!("Class {i}")).collect();
            for (id, name) in labels {
                out[id] = name;
            }
            return Some(out);
        }
    }
    None
}

/// Classes listed in code as numbered dictionary entries, one per line:
/// `3: {"desc": "Hit", "hit": True}`. Each names its class (`desc`,
/// `description`, `label` or `name`) and may say whether it calls for action
/// (`hit`, `act` or `press`).
fn class_dict(code: &str) -> Option<Vec<(String, Option<bool>)>> {
    let mut entries: Vec<(usize, String, Option<bool>)> = Vec::new();
    for line in code.lines() {
        let Some((index, body)) = line.trim().split_once(':') else { continue };
        let Ok(index) = index.trim().parse::<usize>() else { continue };
        let body = body.trim();
        if !body.starts_with('{') {
            continue;
        }
        let Some(label) = ["desc", "description", "label", "name"].iter().find_map(|key| string_field(body, key))
        else {
            continue;
        };
        let act = ["hit", "act", "press"].iter().find_map(|key| bool_field(body, key));
        entries.push((index, label, act));
    }
    entries.sort_by_key(|(index, ..)| *index);
    entries.dedup_by_key(|(index, ..)| *index);
    let size = entries.last()?.0 + 1;
    // A real list: a few entries, most of the numbers used.
    if entries.len() < 2 || entries.len() * 2 < size {
        return None;
    }
    let mut out: Vec<(String, Option<bool>)> = (0..size).map(|i| (format!("Class {i}"), None)).collect();
    for (index, label, act) in entries {
        out[index] = (label, act);
    }
    Some(out)
}

/// What follows `"key":` (or `'key':`) in a dictionary written in code.
fn field_after<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    ['"', '\''].into_iter().find_map(|quote| {
        let quoted = format!("{quote}{key}{quote}");
        let at = body.find(&quoted)?;
        body[at + quoted.len()..].trim_start().strip_prefix(':').map(str::trim_start)
    })
}

fn string_field(body: &str, key: &str) -> Option<String> {
    let rest = field_after(body, key)?;
    let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let inner = &rest[1..];
    Some(inner[..inner.find(quote)?].to_owned())
}

fn bool_field(body: &str, key: &str) -> Option<bool> {
    let rest = field_after(body, key)?;
    match rest.split(|c: char| !c.is_alphabetic()).next()? {
        "True" | "true" => Some(true),
        "False" | "false" => Some(false),
        _ => None,
    }
}

/// Settings declared with a type and a default in code, such as
/// `thresh: int = 60` or `delay_far: int = FAR_DELAY` where `FAR_DELAY = 5`
/// is written elsewhere. Only `thresh…` and `delay…` names count.
fn declared_settings(code: &str) -> serde_json::Map<String, serde_json::Value> {
    let value_of = |text: &str| text.split('#').next().unwrap_or_default().trim().to_owned();
    let constants: HashMap<&str, f64> = code
        .lines()
        .filter_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            let name = name.trim();
            let constant =
                !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
            Some((name, value_of(value).parse().ok().filter(|_| constant)?))
        })
        .collect();
    let mut out = serde_json::Map::new();
    for line in code.lines() {
        let Some((declaration, value)) = line.trim().split_once('=') else { continue };
        let Some((name, kind)) = declaration.split_once(':') else { continue };
        let name = name.trim();
        let setting = (name.starts_with("thresh") || name.starts_with("delay"))
            && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !setting || !matches!(kind.trim(), "int" | "float") {
            continue;
        }
        let value = value_of(value);
        if let Some(number) = value.parse::<f64>().ok().or_else(|| constants.get(value.as_str()).copied()) {
            out.insert(name.to_owned(), serde_json::json!(number));
        }
    }
    out
}

/// Three numbers in brackets after a name such as `MEAN = [0.485, 0.456, 0.406]`.
fn find_triple(text: &str, name: &str) -> Option<[f32; 3]> {
    let lower = text.to_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name) {
        let start = from + at + name.len();
        from = start;
        let end = (start + 120).min(text.len());
        let Some(rest) = text.get(start..end) else { continue };
        let Some(open) = rest.find(['[', '(']) else { continue };
        // Only an assignment counts: `mean = [...]`, `"mean": [...]`.
        if !rest[..open].contains(['=', ':']) {
            continue;
        }
        let inner = rest[open..].trim_start_matches(['[', '(']);
        let Some(close) = inner.find([']', ')']) else { continue };
        let numbers: Vec<f32> = inner[..close].split(',').filter_map(|n| n.trim().parse().ok()).collect();
        if numbers.len() == 3 && numbers.iter().all(|n| (0.0..=1.0).contains(n)) {
            return Some([numbers[0], numbers[1], numbers[2]]);
        }
    }
    None
}

/// The first number given to a name, as in `crop_size=224` or `crop_size: 224`.
fn find_number_after(text: &str, name: &str) -> Option<f64> {
    let mut from = 0;
    while let Some(at) = text[from..].find(name) {
        let start = from + at + name.len();
        from = start;
        let rest = text[start..].trim_start_matches(|c: char| c.is_whitespace() || c == '=' || c == ':');
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if let Ok(value) = digits.parse::<f64>()
            && value > 0.0
        {
            return Some(value);
        }
    }
    None
}

/// Turns the numbers of a `settings.json` into adjustable settings.
fn apply_settings(magic: &mut MagicModel, map: &serde_json::Map<String, serde_json::Value>, notes: &mut Vec<String>) {
    // Settings limited to classes with a word in their name first, so the
    // general ones can skip those classes.
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.contains('_')));
    let mut covered_threshold: Vec<usize> = Vec::new();
    let mut covered_delay: Vec<usize> = Vec::new();
    for key in keys {
        let value = &map[key];
        let lower = key.to_lowercase();
        if lower.contains("monitor") || lower.contains("display") {
            // Monitors are often numbered from 1; none means the main one.
            let display = value.as_u64().map_or(0, |n| n.saturating_sub(1)) as u8;
            magic.capture.display = display;
            continue;
        }
        let Some(number) = value.as_f64() else { continue };
        let (field, word) = match lower.split_once('_') {
            Some((field, word)) => (field.to_owned(), Some(word.to_owned())),
            None => (lower.clone(), None),
        };
        let classes: Vec<usize> = magic
            .classes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.act)
            .filter(|(_, c)| word.as_ref().is_none_or(|w| c.label.to_lowercase().contains(w.as_str())))
            .map(|(i, _)| i)
            .collect();
        let pretty = |base: &str| match &word {
            Some(w) => format!("{base} ({w})"),
            None => base.to_owned(),
        };
        let setting = if field.starts_with("thresh") || field == "confidence" {
            let targets: Vec<usize> =
                classes.into_iter().filter(|c| word.is_some() || !covered_threshold.contains(c)).collect();
            covered_threshold.extend(&targets);
            let percent = if number <= 1.0 { number * 100.0 } else { number };
            ModelSetting {
                key: key.clone(),
                label: pretty(if word.is_some() { "Confidence" } else { "Confidence (others)" }),
                help: "How sure the model must be before acting. Raise it if it acts too early or on the wrong thing."
                    .into(),
                kind: SettingKind::Percent,
                min: if percent >= 50.0 { 50.0 } else { 0.0 },
                max: 100.0,
                default: percent,
                value: percent,
                targets: targets.into_iter().map(|class| SettingTarget::Threshold { class }).collect(),
            }
        } else if field.starts_with("delay") {
            let targets: Vec<usize> =
                classes.into_iter().filter(|c| word.is_some() || !covered_delay.contains(c)).collect();
            covered_delay.extend(&targets);
            ModelSetting {
                key: key.clone(),
                label: pretty("Delay before pressing"),
                help: "A pause before the key is pressed. Raise it if the model acts too early.".into(),
                kind: SettingKind::Milliseconds,
                min: 0.0,
                max: (number * 4.0).max(50.0),
                default: number,
                value: number,
                targets: targets.into_iter().map(|class| SettingTarget::Delay { class }).collect(),
            }
        } else if field.starts_with("cooldown") {
            ModelSetting {
                key: key.clone(),
                label: "Pause after acting".into(),
                kind: SettingKind::Milliseconds,
                min: 0.0,
                max: (number * 4.0).max(2000.0),
                default: number,
                value: number,
                targets: vec![SettingTarget::Cooldown],
                ..ModelSetting::default()
            }
        } else {
            continue;
        };
        if setting.targets.is_empty() {
            notes.push(format!("The setting “{key}” matches no class, so it is not used."));
            continue;
        }
        magic.settings.push(setting);
    }
    // Present them in a stable order: general first.
    magic.settings.sort_by_key(|s| (s.key.contains('_'), s.key.clone()));
    *magic = magic.configured(&serde_json::Map::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_tables_and_normalisation_are_recognised() {
        let readme = "# Thing\n\nIt watches.\n\n| Class | Description |\n|---:|---|\n| 0 | None |\n| 2 | Hit (late) |\n| 1 | Hit (great) |\n";
        assert_eq!(class_table(readme).unwrap(), ["None", "Hit (great)", "Hit (late)"]);
        assert_eq!(first_paragraph(readme).as_deref(), Some("It watches."));
        let code = "class M:\n    MEAN = np.array([0.5, 0.25, 0.125])\n    STD = [0.2, 0.2, 0.2]\nx = f(crop_size=300)";
        assert_eq!(find_triple(code, "mean"), Some([0.5, 0.25, 0.125]));
        assert_eq!(find_triple(code, "std"), Some([0.2, 0.2, 0.2]));
        assert_eq!(find_number_after(code, "crop_size"), Some(300.0));
    }

    #[test]
    fn settings_become_sliders_for_the_classes_they_name() {
        let mut magic = MagicModel {
            classes: ["none", "repair (great)", "repair (frontier)", "wiggle (great)"]
                .iter()
                .map(|l| ClassSpec { label: (*l).into(), act: *l != "none", threshold: 0.5, delay_ms: 0.0 })
                .collect(),
            ..MagicModel::default()
        };
        let settings = serde_json::json!({ "monitor_id": 2, "thresh": 60, "thresh_frontier": 70, "delay_frontier": 5 });
        let mut notes = Vec::new();
        apply_settings(&mut magic, settings.as_object().unwrap(), &mut notes);
        assert_eq!(magic.capture.display, 1);
        assert_eq!(magic.settings.len(), 3);
        assert_eq!(magic.classes[1].threshold, 0.6);
        assert_eq!(magic.classes[2].threshold, 0.7, "the frontier class has its own threshold");
        assert_eq!(magic.classes[3].threshold, 0.6);
        assert_eq!((magic.classes[2].delay_ms, magic.classes[1].delay_ms), (5.0, 0.0));
        assert_eq!(magic.classes[0].threshold, 0.5, "a class that does not act is left alone");
    }

    #[test]
    fn classes_and_settings_written_in_code_are_recognised() {
        let code = r#"
LATE_DELAY = 12  # ms
LIMIT = 70
table = {
    0: {"desc": "nothing", "hit": False, "threshold": 0.0},
    1: {'label': 'hit (late)', 'hit': True, "hit_delay": LATE_DELAY},
    2: {"desc": "hit (great)", "hit": True},
}
x = {1: 2}

class Settings:
    thresh: int = LIMIT
    delay_late: int = LATE_DELAY
    window: int = 4
    thresh_other = 9
"#;
        let classes = class_dict(code).unwrap();
        assert_eq!(classes.len(), 3);
        assert_eq!(classes[1], ("hit (late)".to_owned(), Some(true)));
        assert_eq!(classes[0].1, Some(false));
        let settings = declared_settings(code);
        assert_eq!(settings.len(), 2, "{settings:?}");
        assert_eq!(settings["thresh"], serde_json::json!(70.0));
        assert_eq!(settings["delay_late"], serde_json::json!(12.0));
        assert!(class_dict("x = {1: 2}\ny = {3: {}}").is_none());
    }

    /// Imports real projects and shows what was found. Name them in
    /// `MAGI_IMPORT`, separated by `;`, and run with `--ignored --nocapture`.
    /// With `MAGI_IMPORT_OUT` set to a folder, each is also saved there as a
    /// `.magic` file.
    #[test]
    #[ignore = "needs projects to import, named in MAGI_IMPORT"]
    fn real_projects_import() {
        let paths = std::env::var("MAGI_IMPORT").expect("MAGI_IMPORT names the projects to import");
        for path in paths.split(';').filter(|p| !p.is_empty()) {
            let imported = import(Path::new(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
            let model = &imported.file.model;
            println!("== {path}\n{}", serde_json::to_string_pretty(model).unwrap());
            for note in &imported.notes {
                println!("note: {note}");
            }
            assert!(!imported.file.onnx.is_empty(), "{path}: no model");
            assert!(!model.classes.is_empty(), "{path}: no classes");
            assert!(magi_ml::inspect(&imported.file.onnx).is_ok(), "{path}: the model does not load");
            if let Some(out) = std::env::var_os("MAGI_IMPORT_OUT") {
                let name = Path::new(path).file_stem().unwrap_or_default().to_string_lossy().into_owned();
                let target = Path::new(&out).join(format!("{name}.magic"));
                imported.file.save(&target).unwrap_or_else(|e| panic!("{}: {e}", target.display()));
                println!("saved {}", target.display());
            }
        }
    }
}
