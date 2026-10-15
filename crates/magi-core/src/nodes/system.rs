//! System nodes: the clipboard and notifications.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids, ports};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifyParams {
    pub title: String,
    pub message: String,
    /// Play a short sound.
    pub sound: bool,
    /// Show a desktop notification.
    pub popup: bool,
}

impl Default for NotifyParams {
    fn default() -> Self {
        Self { title: "MacroMagi".into(), message: "Done".into(), sound: true, popup: true }
    }
}

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::CLIPBOARD,
            "Clipboard",
            Category::System,
            "The text on the clipboard. Wire it into Set Variable to keep it, or into any block \
             that reads text.",
        )
        .keywords(&["clipboard", "copy", "paste", "text", "get"])
        .data_out(ports::OUT, "Text", DataType::Text)
        .summarize(|_, _| vec!["Text on the clipboard".into()]),
    );

    registry.register(
        NodeDef::new(
            ids::NOTIFY,
            "Notification",
            Category::System,
            "Plays a short sound and shows a desktop notification – for example when a long \
             macro has finished.",
        )
        .keywords(&["notify", "notification", "sound", "ding", "alert", "beep", "done", "toast"])
        .flow_in()
        .data_in("value", "Value", DataType::Any)
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("title", "Title", ParamKind::Text { multiline: false }))
        .param(
            ParamDef::new("message", "Message", ParamKind::Text { multiline: false })
                .help("Whatever is wired into Value is added to the end."),
        )
        .param(ParamDef::new("sound", "Play a sound", ParamKind::Bool))
        .param(ParamDef::new("popup", "Show a desktop notification", ParamKind::Bool))
        .defaults(&NotifyParams::default())
        .summarize(|node, _| {
            let p: NotifyParams = node.params_or_default();
            let how = match (p.sound, p.popup) {
                (true, true) => "Sound and notification",
                (true, false) => "Sound only",
                (false, true) => "Notification only",
                (false, false) => "Nothing (both off)",
            };
            vec![format!("“{}”", p.message), how.into()]
        }),
    );
}
