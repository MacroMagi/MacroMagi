//! Vision nodes: looking at the screen. These are value nodes – they have no
//! place in the execution order and are evaluated whenever a block reads them.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids};
use crate::geom::Rect;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindImageParams {
    /// Id of the image asset to look for.
    pub image: String,
    /// Where to look. `None` searches the whole primary screen.
    pub region: Option<Rect>,
    /// Minimum similarity, `0.0..=1.0`, for the image to count as found.
    pub threshold: f64,
}

impl Default for FindImageParams {
    fn default() -> Self {
        Self { image: String::new(), region: None, threshold: 0.8 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindColorParams {
    /// `#RRGGBB`.
    pub color: String,
    /// How far each of red, green and blue may differ (0–255) and still match.
    pub tolerance: u8,
    pub region: Option<Rect>,
}

impl Default for FindColorParams {
    fn default() -> Self {
        Self { color: "#FF3B30".into(), tolerance: 16, region: None }
    }
}

fn region_line(region: Option<Rect>) -> String {
    match region {
        Some(r) => format!("In {}×{} at {}, {}", r.w, r.h, r.x, r.y),
        None => "Whole screen".into(),
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
        .keywords(&["find", "see", "template", "match", "screenshot", "picture", "detect", "look"])
        .data_out("found", "Found", DataType::Bool)
        .data_out("position", "Position", DataType::Point)
        .data_out("score", "Similarity", DataType::Number)
        .param(ParamDef::new("image", "Image", ParamKind::Image))
        .param(
            ParamDef::new("region", "Search area", ParamKind::Region)
                .help("A smaller area is faster and avoids false matches elsewhere on screen."),
        )
        .param(
            ParamDef::new("threshold", "Accuracy", ParamKind::Percent)
                .help("How closely the screen must match the image. 100% demands a pixel-perfect match."),
        )
        .defaults(&FindImageParams::default())
        .summarize(|node, assets| {
            let p: FindImageParams = node.params_or_default();
            let name = match assets.get(&p.image) {
                Some(asset) => asset.name.clone(),
                None => "No image selected".into(),
            };
            vec![name, format!("Accuracy ≥ {:.0}%", p.threshold * 100.0), region_line(p.region)]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::FIND_COLOR,
            "Color Detection",
            Category::Vision,
            "Looks for a colour on screen. “Found” is true when at least one pixel in the search \
             area matches; “Position” is the first matching pixel.",
        )
        .keywords(&["pixel", "colour", "color", "rgb", "detect"])
        .data_out("found", "Found", DataType::Bool)
        .data_out("position", "Position", DataType::Point)
        .param(ParamDef::new("color", "Color", ParamKind::Color))
        .param(
            ParamDef::new("tolerance", "Tolerance", ParamKind::int(0, 255, ""))
                .help("How far red, green and blue may each be off and still count as a match."),
        )
        .param(ParamDef::new("region", "Search area", ParamKind::Region))
        .defaults(&FindColorParams::default())
        .summarize(|node, _| {
            let p: FindColorParams = node.params_or_default();
            vec![format!("{} ± {}", p.color.to_uppercase(), p.tolerance), region_line(p.region)]
        }),
    );
}
