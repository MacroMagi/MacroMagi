//! The user interface. Each tab and each larger widget is its own module.
//!
//! UI modules edit the data they are handed and never reach for application
//! state. When something needs the wider app – picking a point on screen,
//! listening for a hotkey, opening a file dialog – they return a
//! [`UiRequest`] and the app carries it out.

pub mod autoclicker;
pub mod graph;
pub mod images;
pub mod inspector;
pub mod macro_tab;
pub mod recording_editor;
pub mod settings_window;
pub mod snip;
pub mod widgets;

use magi_core::graph::NodeId;

/// A hotkey that can be rebound.
#[derive(Clone, Debug, PartialEq)]
pub enum HotkeyTarget {
    Clicker,
    StopEverything,
    Record,
    /// A hotkey parameter of a block (the parameter's key).
    Node(NodeId, String),
}

/// What the screen picker should ask the user for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnipMode {
    /// A single position.
    Point,
    /// A rectangle.
    Region,
    /// A rectangle, returned together with the pixels inside it.
    Image,
    /// The colour of a single pixel.
    Color,
}

/// Where the result of the screen picker goes.
#[derive(Clone, Debug, PartialEq)]
pub enum SnipTarget {
    /// A parameter of a block.
    NodeParam(NodeId, String),
    /// The position of one input inside an Input block.
    Action(NodeId, usize),
    /// The Auto Clicker's fixed click position.
    ClickerPosition,
}

pub enum UiRequest {
    Snip(SnipMode, SnipTarget),
    CaptureHotkey(HotkeyTarget),
    /// Load an image file into a block's image parameter.
    LoadImage(NodeId, String),
    /// Start (or stop) recording; the result is linked to this block's recording parameter.
    RecordInto(NodeId, String),
}
