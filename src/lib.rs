//! astar: a fast octilinear grid A* PCB autorouter. A scene goes in, a plan comes out.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let scene = astar::Scene::load("board.scene.json")?;
//! let routed = astar::route(scene, &astar::Cfg::default())?;
//! std::fs::write("board.plan.json", routed.plan())?;
//! # Ok(())
//! # }
//! ```
//!
//! `Scene` is the board as the router is given it (docs/scene.md), `Cfg` what it may be told, `route`
//! the whole run, `Routed` the board it kept: its copper (`router.jobs`), its report (`router.notes`),
//! the plan as JSON (`plan()`). An `Err` is an input the router cannot take; nets left open are not an
//! error (`Routed::open_nets`, `Routed::exit_code`). docs/hooks.md is how a track adds to a run.

pub mod check;
pub mod field;
pub mod finish;
pub mod geom;
pub mod io;
pub mod json;
pub mod passes;
pub mod report;
pub mod route;
pub mod scene;
pub mod search;

pub use route::{route, Cfg, Routed};
pub use route::like;
pub use scene::Scene;

/// Why a scene was not routed at all. What a run could not finish is not an error: it is in its
/// report (nets left open, widths narrowed). A panic is neither: it is a bug in astar.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The scene, the settings, or the two together are not something the router can take.
    BadInput(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::BadInput(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for Error {}

/// What the command exits with.
pub mod exit {
    /// every net joined
    pub const WHOLE: i32 = 0;
    /// a plan was written and nets are open
    pub const OPEN: i32 = 1;
    /// the input could not be taken: no plan
    pub const BAD_INPUT: i32 = 2;
    /// every net joined, but a rule was narrowed and `--strict` was asked
    pub const NARROWED: i32 = 3;
    /// a plan was written, and astar's own check finds fault with it: copper that breaks a rule, or a
    /// net the plan calls joined that is not. A fault in astar; the findings are printed.
    pub const FAULT: i32 = 4;
    /// every net joined, but KiCad may yet find fault where astar did not look: a custom rule that was not
    /// read and may ask more than was kept, a short the board came with. The lines say which.
    pub const UNSURE: i32 = 5;
}
