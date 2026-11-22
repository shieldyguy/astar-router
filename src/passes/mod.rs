//! The passes: the planning and finishing steps of a run, each with a name, in a fixed order, timed,
//! and switchable (docs/hooks.md, "Pass"). One file a pass; the list is `PASSES`.

pub mod cross;
pub mod drops;
pub mod egress;
pub mod exits;
pub mod fences;
pub mod ground;
pub mod pairs;
pub mod pour;

use crate::route::{Cfg, Node, Router};
use crate::scene::Scene;
use std::time::Instant;

pub struct Pass {
    pub name: &'static str,
    /// one line for `--passes`
    pub about: &'static str,
    pub step: Step,
}

/// What a pass does, and so when it runs. A run goes through the stages in this order; within a stage
/// the passes run in the order of `PASSES`.
#[derive(Clone, Copy)]
pub enum Step {
    /// Before anything else: draws on the board what its designer left to the router (the ground's
    /// copper, the planes the run names). It returns what it gave to nets, as (net, layer): a plane on an
    /// inner layer, a pour on an outer one; they are written to the plan. The board is routed with it.
    Draw(fn(&mut Scene, &Cfg) -> Vec<(u16, usize)>),
    /// Before the field is built: may change the board the router is given. It returns the planes it
    /// gave to nets, as (net, layer); they are written to the plan. A board that a board pass changed is
    /// routed both ways, with the change and without (`route::drive`).
    Board(fn(&mut Scene, &Cfg) -> Vec<(u16, usize)>),
    /// The field stands and nothing is routed yet: fixed copper, cost terms, jobs.
    Plan(fn(&mut Router)),
    /// Lays and lifts copper.
    Route(fn(&mut Router)),
    /// Called by the route with every path it finds, before the path is laid: (router, net, path) to
    /// the path to lay. A path that runs through another net's copper is laid as found and not shown.
    Path(fn(&Router, u16, &[Node]) -> Vec<Node>),
    /// The copper stands on the board that is kept: redraws it.
    Finish(fn(&mut Router)),
}

impl Step {
    pub fn stage(&self) -> &'static str {
        match self {
            Step::Draw(_) => "board",
            Step::Board(_) => "board",
            Step::Plan(_) => "plan",
            Step::Route(_) => "route",
            Step::Path(_) => "path",
            Step::Finish(_) => "finish",
        }
    }
}

/// Every pass, in the order they run.
pub const PASSES: &[Pass] = &[ground::PASS, pour::PASS, fences::PASS, exits::PASS, cross::PASS, drops::PASS, pairs::PASS, crate::route::PASS, crate::finish::tight::PASS, crate::finish::arcs::PASS];

/// The clock entry for what no pass owns: the field, the exact index, the pads' access nodes.
pub const FIELD: usize = PASSES.len();

/// A pass's place in `PASSES` (and on the clock).
pub fn index(name: &str) -> Option<usize> {
    PASSES.iter().position(|p| p.name == name)
}

/// Seconds per pass. A pass that starts while another runs stops the other's clock until it is done,
/// so the seconds add up to the run's.
pub struct Clock {
    secs: Vec<f64>,
    stack: Vec<(usize, Instant)>,
}

impl Clock {
    pub fn new() -> Clock {
        Clock { secs: vec![0.0; PASSES.len() + 1], stack: Vec::new() }
    }
    pub fn enter(&mut self, id: usize) {
        let now = Instant::now();
        if let Some(top) = self.stack.last() {
            self.secs[top.0] += (now - top.1).as_secs_f64();
        }
        self.stack.push((id, now));
    }
    /// Seconds spent before the clock was there to see them.
    pub fn add(&mut self, id: usize, secs: f64) {
        self.secs[id] += secs;
    }
    pub fn leave(&mut self) {
        let now = Instant::now();
        if let Some((id, t)) = self.stack.pop() {
            self.secs[id] += (now - t).as_secs_f64();
        }
        if let Some(top) = self.stack.last_mut() {
            top.1 = now;
        }
    }
    /// (name, seconds), the field first, then the passes in order.
    pub fn lines(&self) -> Vec<(&'static str, f64)> {
        let mut out = vec![("field", self.secs[FIELD])];
        out.extend(PASSES.iter().enumerate().map(|(i, p)| (p.name, self.secs[i])));
        out
    }
    pub fn secs(&self, name: &str) -> f64 {
        index(name).map_or(0.0, |i| self.secs[i])
    }
}

impl Default for Clock {
    fn default() -> Clock {
        Clock::new()
    }
}
