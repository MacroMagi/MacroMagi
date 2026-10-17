//! Vision nodes: looking at the screen. These are value nodes – they have no
//! place in the execution order and are evaluated whenever a block reads them.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids};
use crate::geom::Rect;

/// One of the images an Image Detection block looks for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageTarget {
    /// Id of the image asset.
    pub image: String,
    /// The group it belongs to (empty: none).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Its own search area, instead of its group's or the block's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<Rect>,
}

/// Images of one block that share a search area.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageGroup {
    pub name: String,
    /// The group's search area; `None` uses the block's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<Rect>,
}

/// When a block that looks for several things counts them as found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindMode {
    /// As soon as one of them is there.
    #[default]
    Any,
    /// Only when every one of them is there.
    All,
}

/// How much detail image matching uses. Less detail is faster.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    #[default]
    Full,
    Half,
    Third,
    Quarter,
}

impl Detail {
    /// The screen and the images are shrunk by this factor before matching.
    pub fn factor(self) -> u32 {
        match self {
            Detail::Full => 1,
            Detail::Half => 2,
            Detail::Third => 3,
            Detail::Quarter => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindImageParams {
    /// The images to look for, in order of priority.
    pub images: Vec<ImageTarget>,
    pub groups: Vec<ImageGroup>,
    /// The one image of blocks made before a block could look for several.
    pub image: String,
    /// Where to look. `None` searches the whole of `display`.
    pub region: Option<Rect>,
    /// The display searched when there is no region (0 = primary).
    pub display: u8,
    /// Minimum similarity, `0.0..=1.0`, for an image to count as found.
    pub threshold: f64,
    pub mode: FindMode,
    pub detail: Detail,
}

impl Default for FindImageParams {
    fn default() -> Self {
        Self {
            images: Vec::new(),
            groups: Vec::new(),
            image: String::new(),
            region: None,
            display: 0,
            threshold: 0.8,
            mode: FindMode::Any,
            detail: Detail::Full,
        }
    }
}

impl FindImageParams {
    /// Every image to look for, the one of an older block included.
    pub fn targets(&self) -> Vec<ImageTarget> {
        let mut targets = self.images.clone();
        if !self.image.is_empty() && !targets.iter().any(|t| t.image == self.image) {
            targets.insert(0, ImageTarget { image: self.image.clone(), ..ImageTarget::default() });
        }
        targets.retain(|t| !t.image.is_empty());
        targets
    }

    /// Where an image is looked for: its own area, else its group's, else the block's.
    pub fn region_of(&self, target: &ImageTarget) -> Option<Rect> {
        target
            .region
            .or_else(|| self.groups.iter().find(|g| !target.group.is_empty() && g.name == target.group)?.region)
            .or(self.region)
    }
}

/// How much a Text Scan block enlarges the picture before reading it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enlarge {
    #[default]
    Off,
    Double,
    Triple,
}

impl Enlarge {
    pub fn factor(self) -> u32 {
        match self {
            Enlarge::Off => 1,
            Enlarge::Double => 2,
            Enlarge::Triple => 3,
        }
    }
}

/// Settings of a **Text Scan** block.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadTextParams {
    /// Where to read. `None` reads the whole of `display`.
    pub region: Option<Rect>,
    pub display: u8,
    /// Text to look for; empty: any text counts as found.
    pub find: String,
    pub match_case: bool,
    pub enlarge: Enlarge,
    /// A language tag such as `en-US`; empty for the system's languages.
    pub language: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindColorParams {
    /// `#RRGGBB`.
    pub color: String,
    /// How far each of red, green and blue may differ (0–255) and still match.
    pub tolerance: u8,
    pub region: Option<Rect>,
    /// The display searched when there is no region (0 = primary).
    pub display: u8,
}

impl Default for FindColorParams {
    fn default() -> Self {
        Self { color: "#FF3B30".into(), tolerance: 16, region: None, display: 0 }
    }
}

fn region_line(region: Option<Rect>, display: u8) -> String {
    let on = |d: u8| if d == 0 { String::new() } else { format!(" on display {}", d + 1) };
    match region {
        Some(r) => format!("In {}×{} at {}, {}{}", r.w, r.h, r.x, r.y, on(r.display.unwrap_or(0))),
        None if display == 0 => "Whole screen".into(),
        None => format!("Whole display {}", display + 1),
    }
}

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::FIND_IMAGE,
            "Image Detection",
            Category::Vision,
            "Looks for an image on screen. “Found” is true when a match at least as similar as \
             the accuracy setting exists; “Position” is the centre of the best match. Wire \
             “Found” into a Decision or Wait Until, and “Position” into an Input to click it.",
        )
        .keywords(&["find", "see", "template", "match", "screenshot", "picture", "detect", "look", "images", "group"])
        .data_out("found", "Found", DataType::Bool)
        .data_out("position", "Position", DataType::Point)
        .data_out("score", "Similarity", DataType::Number)
        .data_out("name", "Which image", DataType::Text)
        .data_out("group", "Its group", DataType::Text)
        .data_out("count", "How many", DataType::Number)
        .param(
            ParamDef::new("images", "Images", ParamKind::Images).help(
                "Several images can be looked for at once, in order of priority: the first one found is the one \
                 reported. Put images in a group to give them a search area of their own.",
            ),
        )
        .param(ParamDef::new("image", "Image", ParamKind::Image).hidden())
        .param(
            ParamDef::new("mode", "Found when", ParamKind::choice(&[("any", "Any image is there"), ("all", "Every image is there")])),
        )
        .param(
            ParamDef::new("region", "Search area", ParamKind::Region)
                .help("For every image without an area of its own or of its group. A smaller area is faster and avoids false matches elsewhere on screen."),
        )
        .param(
            ParamDef::new("display", "Display", ParamKind::Display)
                .help("The display searched when there is no search area."),
        )
        .param(
            ParamDef::new("threshold", "Accuracy", ParamKind::Percent)
                .help("How closely the screen must match the image. 100% demands a pixel-perfect match."),
        )
        .param(
            ParamDef::new(
                "detail",
                "Detail",
                ParamKind::choice(&[
                    ("full", "Full – most accurate"),
                    ("half", "Half – about 4× faster"),
                    ("third", "A third – about 9× faster"),
                    ("quarter", "A quarter – fastest"),
                ]),
            )
            .help("Shrinks the screen and the images before comparing them. Less detail is much faster but can miss small or fine images."),
        )
        .defaults(&FindImageParams::default())
        .summarize(|node, assets| {
            let p: FindImageParams = node.params_or_default();
            let targets = p.targets();
            let headline = match targets.as_slice() {
                [] => "No image selected".to_owned(),
                [one] => assets.get(&one.image).map_or_else(|| "A missing image".to_owned(), |a| a.name.clone()),
                many => {
                    let groups = p.groups.iter().filter(|g| many.iter().any(|t| t.group == g.name)).count();
                    match (p.mode, groups) {
                        (FindMode::Any, 0) => format!("Any of {} images", many.len()),
                        (FindMode::All, 0) => format!("All of {} images", many.len()),
                        (FindMode::Any, g) => format!("Any of {} images in {g} groups", many.len()),
                        (FindMode::All, g) => format!("All of {} images in {g} groups", many.len()),
                    }
                }
            };
            let mut lines = vec![headline, format!("Accuracy ≥ {:.0}%", p.threshold * 100.0), region_line(p.region, p.display)];
            if p.detail != Detail::Full {
                lines.push(format!("Detail 1/{}", p.detail.factor()));
            }
            lines
        }),
    );

    registry.register(
        NodeDef::new(
            ids::READ_TEXT,
            "Text Scan",
            Category::Vision,
            "Reads the text in an area of the screen (optical character recognition). “Text” is what it \
             reads, line by line. With text to look for, “Found” says whether it is there and “Position” \
             is where; “Number” is the first number in the text. Uses the text recognition built into \
             Windows.",
        )
        .keywords(&["ocr", "read", "text", "scan", "recognise", "recognize", "number", "words", "letters"])
        .data_out("found", "Found", DataType::Bool)
        .data_out("text", "Text", DataType::Text)
        .data_out("position", "Position", DataType::Point)
        .data_out("number", "Number", DataType::Number)
        .param(
            ParamDef::new("region", "Area", ParamKind::Region)
                .help("The area to read. A small area around the text is faster and reads more reliably."),
        )
        .param(ParamDef::new("display", "Display", ParamKind::Display).help("The display read when there is no area."))
        .param(
            ParamDef::new("find", "Look for", ParamKind::Text { multiline: false })
                .help("Text that makes “Found” true. Empty: any text does."),
        )
        .param(ParamDef::new("match_case", "Match upper and lower case", ParamKind::Bool))
        .param(
            ParamDef::new(
                "enlarge",
                "Enlarge",
                ParamKind::choice(&[
                    ("off", "No – the text is a normal size"),
                    ("double", "Twice – small text"),
                    ("triple", "Three times – tiny text"),
                ]),
            )
            .help("Text recognition needs letters at least about 10 pixels tall."),
        )
        .param(
            ParamDef::new("language", "Language", ParamKind::Text { multiline: false })
                .help("A language tag such as en-US or de-DE. Empty: the languages of your Windows."),
        )
        .defaults(&ReadTextParams::default())
        .summarize(|node, _| {
            let p: ReadTextParams = node.params_or_default();
            let headline = if p.find.trim().is_empty() {
                "Reads text".to_owned()
            } else {
                format!("Looks for “{}”", p.find.trim())
            };
            vec![headline, region_line(p.region, p.display)]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::FIND_COLOR,
            "Color Detection",
            Category::Vision,
            "Looks for a color on screen. “Found” is true when at least one pixel in the search \
             area matches; “Position” is the first matching pixel.",
        )
        .keywords(&["pixel", "color", "colour", "rgb", "detect"])
        .data_out("found", "Found", DataType::Bool)
        .data_out("position", "Position", DataType::Point)
        .param(ParamDef::new("color", "Color", ParamKind::Color))
        .param(
            ParamDef::new("tolerance", "Tolerance", ParamKind::int(0, 255, ""))
                .help("How far red, green and blue may each be off and still count as a match."),
        )
        .param(ParamDef::new("region", "Search area", ParamKind::Region))
        .param(
            ParamDef::new("display", "Display", ParamKind::Display)
                .help("The display searched when there is no search area."),
        )
        .defaults(&FindColorParams::default())
        .summarize(|node, _| {
            let p: FindColorParams = node.params_or_default();
            vec![format!("{} ± {}", p.color.to_uppercase(), p.tolerance), region_line(p.region, p.display)]
        }),
    );
}
