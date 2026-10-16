//! The user interface. Each tab and each larger widget is its own module.
//!
//! UI modules edit the data they are handed and never reach for application
//! state. When something needs the wider app – picking a point on screen,
//! listening for a hotkey, opening a file dialog – they return a
//! [`UiRequest`] and the app carries it out.

pub mod autoclicker;
pub mod clicker_form;
pub mod graph;
pub mod images;
pub mod inspector;
pub mod lists;
pub mod macro_tab;
pub mod magic_tab;
pub mod options;
pub mod overlay;
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
    Run,
    /// One of the extra stop hotkeys; `None` adds a new one.
    ExtraStop(Option<usize>),
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
    /// The position of one input in a block's list of inputs (the list's
    /// parameter key, the index).
    Action(NodeId, String, usize),
    /// The Auto Clicker tab's fixed click position.
    ClickerPosition,
    /// The fixed click position of an Auto Click block.
    ClickerNode(NodeId),
    /// A new image for an Image Detection block's list.
    AddImage(NodeId),
    /// The search area of one image of an Image Detection block.
    ImageRegion(NodeId, usize),
    /// The search area of one image group of an Image Detection block.
    ImageGroupRegion(NodeId, usize),
    /// The fixed screen area of the model open in the Magic tab.
    MagicArea,
}

pub enum UiRequest {
    Snip(SnipMode, SnipTarget),
    CaptureHotkey(HotkeyTarget),
    CancelCapture,
    /// Load an image file into a block's image parameter.
    LoadImage(NodeId, String),
    /// Start (or stop) recording; the result is linked to this block's recording parameter.
    RecordInto(NodeId, String),
    /// Open a recording in its own editor window.
    EditRecording(String),
    /// Load a sound file into a block's sound parameter.
    LoadSound(NodeId, String),
    /// Open a Rate block in the Variable Rate Editor.
    EditRate(NodeId),
    /// Load an image file into an Image Detection block's list.
    AddImageFile(NodeId),
    /// Load a sound file into a Sound Recognition block's list.
    AddSoundFile(NodeId),
    /// Load a `.magic` file into a block's model setting.
    LoadModel(NodeId, String),
    /// Open a model asset of the macro in the Magic tab.
    OpenModel(String),
}
