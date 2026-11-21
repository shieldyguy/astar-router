//! Board I/O: a KiCad board file read into a scene, and a plan written back onto the board.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let board = astar::io::read("board.kicad_pcb")?;          // the file, and the scene it makes
//! let routed = astar::route(board.to_scene()?, &astar::Cfg::default())?;
//! astar::io::write(&board, &routed.plan(), "routed.kicad_pcb", 0)?;
//! # Ok(())
//! # }
//! ```
//!
//! The reader is `scene::read`: `board` opens the file (`sexpr`, lossless), `foot` reads the
//! footprints, `copper` the rest of the board, `project` the rules beside it. The writer is `write`.

mod board;
pub mod cli;
mod copper;
mod foot;
mod num;
mod project;
mod scene;
mod shapes;
pub mod sexpr;
mod write;

#[cfg(test)]
mod tests;

pub use scene::Read;
pub use write::{copy_siblings, lay, Laid};

use crate::{Error, Scene};

/// A board file read: its scene (`Read::scene`, JSON), its tree, the reader's notes.
pub fn read(path: &str) -> Result<Read, Error> {
    scene::read(path).map_err(|e| Error::BadInput(format!("{}: {}", path, e)))
}

impl Read {
    /// The scene as the router takes it, checked.
    pub fn to_scene(&self) -> Result<Scene, Error> {
        Scene::from_json(&self.scene).map_err(Error::BadInput)
    }

    /// The scene as JSON text (docs/scene.md, format 2).
    pub fn scene_json(&self) -> String {
        crate::json::write(&self.scene)
    }
}

/// A plan (its JSON text) laid on the board that was read, written to `out` with the board's sibling
/// files beside it. `seed` changes the new items' ids and nothing else.
pub fn write(rd: &Read, plan: &str, out: &str, seed: u64) -> Result<Laid, Error> {
    let bad = |e: String| Error::BadInput(e);
    let plan = crate::json::parse(plan).map_err(|e| bad(format!("the plan: {}", e)))?;
    let (text, laid) = lay(rd, &plan, seed).map_err(bad)?;
    std::fs::write(out, text).map_err(|e| bad(format!("cannot write {}: {}", out, e)))?;
    Ok(laid)
}
