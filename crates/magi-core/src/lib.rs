//! # magi-core
//!
//! The MacroMagi data model. This crate has no operating-system or UI
//! dependencies: it describes *what a macro is*, not how it is drawn or run.
//!
//! * [`graph`] – nodes and wires.
//! * [`nodes`] – the registry of node definitions (ports, parameters, docs).
//! * [`input`], [`hotkey`], [`action`], [`recording`] – the input vocabulary.
//! * [`assets`] – images and recordings linked from nodes.
//! * [`project`] / [`preset`] – the `.magi` and `.mp` file formats.
//! * [`autoclicker`] – the Auto Clicker settings and their graph equivalent.
//! * [`host`] – the interface engines use to reach the real machine.

pub mod action;
pub mod assets;
pub mod autoclicker;
pub mod error;
pub mod geom;
pub mod graph;
pub mod host;
pub mod hotkey;
pub mod input;
pub mod nodes;
pub mod preset;
pub mod project;
pub mod recording;
pub mod text;

pub use error::{Error, Result};

/// Human-facing application name.
pub const APP_NAME: &str = "MacroMagi";
/// Version of the workspace, recorded in saved files.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
