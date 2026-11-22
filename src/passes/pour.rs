//! The rail pour: the busiest rail gets the free inner layer.

use super::{Pass, Step};
use crate::route::Cfg;
use crate::scene::{Class, Scene};

pub const PASS: Pass = Pass {
    name: "pour",
    about: "the rail with the most pads gets a free inner layer as its plane; the board is routed with it and without, and the poured one kept when it is whole",
    step: Step::Board(pour),
};

/// The rail to pour and the inner layer to pour it on: the rail with the most pads, on a layer no plane
/// has yet; None when there is no such layer or no rail with enough pads.
pub fn pour_choice(sc: &Scene, cfg: &Cfg) -> Option<(usize, usize)> {
    if cfg.pour_pads == 0 {
        return None;
    }
    let layer = super::ground::free_inner(sc).first().copied()?;
    let best = (1..sc.nets.len()).filter(|&i| sc.nets[i].class == Class::Rail && sc.nets[i].wanted() && sc.nets[i].plane.is_none() && sc.nets[i].pads.len() >= cfg.pour_pads).max_by_key(|&i| sc.nets[i].pads.len())?;
    Some((best, layer))
}

/// A free inner layer becomes the plane of the rail with the most pads: its pads then need only a via
/// each, and no wide track has to find its way round the parts.
fn pour(sc: &mut Scene, cfg: &Cfg) -> Vec<(u16, usize)> {
    match pour_choice(sc, cfg) {
        Some((i, layer)) => {
            sc.nets[i].plane = Some(layer);
            vec![(i as u16, layer)]
        }
        None => Vec::new(),
    }
}
