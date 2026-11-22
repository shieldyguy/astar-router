//! Differential pairs side by side. The work is the route's own (`route::duo`: a pair is one thing in the
//! negotiation); this is its place among the passes, which says what it does and turns it off
//! (`--skip pairs`: every pair is two nets).

use super::{Pass, Step};
use crate::route::Router;

pub const PASS: Pass = Pass {
    name: "pairs",
    about: "a differential pair runs side by side at its width and gap on one layer between the two parts both nets have a pad on, as one thing in the negotiation; a pair with no such way is two nets, and the plan says so",
    step: Step::Plan(plan),
};

fn plan(_: &mut Router) {}
