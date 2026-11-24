//! Symphony core: the document model, kept free of UI, GPU and file IO so
//! it can be tested and scripted headless (ENGINEERING E1.2).
//!
//! - [`id`]: stable IDs (E2.2).
//! - [`doc`]: the scene document: layers, xsheet, graph, drawings, palette.
//! - [`history`]: undoable edits (E3).
//! - [`xsheet`], [`graph`], [`stroke`], [`palette`], [`anim`], [`math`]: parts.

pub mod anim;
pub mod b64;
pub mod doc;
pub mod graph;
pub mod history;
pub mod id;
pub mod math;
pub mod palette;
pub mod puppet;
pub mod rig;
pub mod stroke;
pub mod warp;
pub mod xsheet;
