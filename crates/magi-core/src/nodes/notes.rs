//! Notes: comment blocks. They have no ports and never run; they are there to
//! explain a macro to whoever reads it.

use serde::{Deserialize, Serialize};

use super::{Category, NodeDef, ParamDef, ParamKind, Registry, ids};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommentParams {
    pub text: String,
    /// Size on the canvas, in canvas units.
    pub width: f64,
    pub height: f64,
    /// Folded to its title line.
    pub collapsed: bool,
    /// `#RRGGBB`, or empty for the theme's colour.
    pub color: String,
}

impl Default for CommentParams {
    fn default() -> Self {
        Self {
            text: "A note about this part of the macro.".into(),
            width: 240.0,
            height: 120.0,
            collapsed: false,
            color: String::new(),
        }
    }
}

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::COMMENT,
            "Comment",
            Category::Notes,
            "A note on the canvas. It does nothing when the macro runs. Resize it by its corner, \
             fold it by its arrow.",
        )
        .keywords(&["comment", "note", "text", "label", "explain", "annotation", "sticky"])
        .param(ParamDef::new("text", "Text", ParamKind::Text { multiline: true }))
        .param(ParamDef::new("width", "Width", ParamKind::float(80.0, 4000.0, "")))
        .param(ParamDef::new("height", "Height", ParamKind::float(40.0, 4000.0, "")))
        .param(ParamDef::new("collapsed", "Folded", ParamKind::Bool))
        .param(ParamDef::new("color", "Colour", ParamKind::Text { multiline: false }).help("#RRGGBB, or empty"))
        .defaults(&CommentParams::default()),
    );
}
