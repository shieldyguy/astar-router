//! Ground and the planes: the copper a board's designer left to the router.
//!
//! What a net's copper should be is read off the board first: a zone drawn for a net is its pour, a
//! layer given to a net is its plane. Where the board draws nothing for its ground, the router draws
//! what a designer would have: a free inner layer as the ground's plane, and on a board without one a
//! pour on the side with fewer parts. `--plane` says a net's layers outright, for ground or any net;
//! `--ground tracks` leaves the ground to be wired as a net.

use super::{Pass, Step};
use crate::route::Cfg;
use crate::scene::{Class, Scene};

pub const PASS: Pass = Pass {
    name: "ground",
    about: "a ground the board draws no copper for gets its own: a free inner layer as its plane, else a pour on the side with fewer parts (--ground tracks: wired as a net); --plane NET=LAYER gives any net a plane or a pour",
    step: Step::Draw(draw),
};

/// The inner layers no net has a plane on and the board pours nothing on itself, lowest first.
pub fn free_inner(sc: &Scene) -> Vec<usize> {
    let mut taken: Vec<usize> = sc.nets.iter().filter_map(|n| n.plane).collect();
    taken.extend(sc.copper.zones.iter().filter(|z| z.net != 0 && !(z.locked && z.min_width == Some(0.0))).map(|z| z.layer));
    (1..sc.layers.len().saturating_sub(1)).filter(|l| !taken.contains(l)).collect()
}

/// Has the board (or the run) said what this net's copper is: a plane, or a pour drawn for it?
pub fn has_copper_of_its_own(sc: &Scene, net: usize) -> bool {
    sc.nets[net].plane.is_some() || sc.copper.zones.iter().any(|z| z.net as usize == net && z.outline.len() >= 3 && !(z.locked && z.min_width == Some(0.0)))
}

/// The ground the router draws copper for: the ground net with the most pads that is to be routed here
/// and has no copper of its own; None when there is none, or the run wants ground wired (`--ground tracks`).
pub fn ground_to_draw(sc: &Scene, cfg: &Cfg) -> Option<usize> {
    if cfg.ground_tracks {
        return None;
    }
    (1..sc.nets.len())
        .filter(|&i| sc.nets[i].class == Class::Ground && sc.nets[i].wanted() && sc.nets[i].pads.len() >= 3 && !cfg.drops.contains(&sc.nets[i].name))
        .max_by_key(|&i| (sc.nets[i].pads.len(), std::cmp::Reverse(i)))
        .filter(|&i| !has_copper_of_its_own(sc, i))
}

/// A layer given to a net: an inner layer is its plane, an outer one gets a pour over the whole board.
fn give(sc: &mut Scene, net: usize, layer: usize) {
    if layer > 0 && layer + 1 < sc.layers.len() {
        sc.nets[net].plane = Some(layer);
    } else {
        sc.draw_pour(net, layer);
    }
}

/// What the run says outright (`--plane`), then the ground. Returns what was given, as (net, layer).
fn draw(sc: &mut Scene, cfg: &Cfg) -> Vec<(u16, usize)> {
    let mut out = Vec::new();
    for (name, layers) in &cfg.planes {
        let Some(n) = sc.nets.iter().position(|x| x.name == *name) else { continue };
        let ls: Vec<usize> = layers.iter().filter_map(|l| sc.layers.iter().position(|x| x == l)).collect();
        for l in ls {
            give(sc, n, l);
            out.push((n as u16, l));
        }
    }
    if let Some(g) = ground_to_draw(sc, cfg) {
        let nl = sc.layers.len();
        let layer = free_inner(sc).first().copied().unwrap_or_else(|| {
            // no inner layer to give: the side with fewer surface pads, the back when they are even
            let back = sc.pads.iter().filter(|p| !p.tht && p.back).count();
            let front = sc.pads.iter().filter(|p| !p.tht && !p.back).count();
            if front < back {
                0
            } else {
                nl - 1
            }
        });
        give(sc, g, layer);
        out.push((g as u16, layer));
    }
    out
}
