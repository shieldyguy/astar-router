//! Ground and the planes: the copper a board's designer left to the router.
//!
//! What a net's copper should be is read off the board first: a zone drawn for a net is its pour, a
//! layer given to a net is its plane. What the board leaves undrawn of its ground the router draws as
//! the house does: a free inner layer as the ground's plane and a pour on each outer layer over it; on
//! a board with no inner layer a pour on the side with fewer parts. `--plane` says a net's layers outright, for ground or any net;
//! `--ground tracks` leaves the ground to be wired as a net.

use super::{Pass, Step};
use crate::route::Cfg;
use crate::scene::{Class, Scene};

pub const PASS: Pass = Pass {
    name: "ground",
    about: "the ground gets what the board does not draw for it: a free inner layer as its plane and a pour on each outer layer over it, or with no inner layer a pour on the side with fewer parts (--ground tracks: wired as a net, nothing drawn); --plane NET=LAYER gives any net a plane or a pour",
    step: Step::Draw(draw),
};

/// The inner layers no net has a plane on and the board pours nothing on itself, lowest first.
pub fn free_inner(sc: &Scene) -> Vec<usize> {
    let mut taken: Vec<usize> = sc.nets.iter().filter_map(|n| n.plane).collect();
    taken.extend(sc.copper.zones.iter().filter(|z| z.net != 0 && !(z.locked && z.min_width == Some(0.0))).map(|z| z.layer));
    (1..sc.layers.len().saturating_sub(1)).filter(|l| !taken.contains(l)).collect()
}

/// The ground the router draws copper for: the ground net with the most pads that is to be routed here;
/// None when there is none, or the run wants ground wired (`--ground tracks`).
pub fn ground_to_draw(sc: &Scene, cfg: &Cfg) -> Option<usize> {
    if cfg.ground_tracks {
        return None;
    }
    (1..sc.nets.len())
        .filter(|&i| sc.nets[i].class == Class::Ground && sc.nets[i].wanted() && sc.nets[i].pads.len() >= 3 && !cfg.drops.contains(&sc.nets[i].name))
        .max_by_key(|&i| (sc.nets[i].pads.len(), std::cmp::Reverse(i)))
}

/// The layers the ground is given, where the run does not say them itself (`--plane`). On a board with
/// inner layers: the first free one as its plane when the board gives it none, and, under a plane, a
/// pour on each outer layer when the board pours the ground on neither (an outer layer where another
/// net's pour lies that a pour over the whole board could not go under is left). With no plane to be
/// had and no pour drawn: one pour, on the side with fewer surface pads (the back when they are even).
fn ground_layers(sc: &Scene, cfg: &Cfg, g: usize) -> Vec<usize> {
    if cfg.planes.iter().any(|(n, _)| *n == sc.nets[g].name) {
        return Vec::new();
    }
    let nl = sc.layers.len();
    let drawn = |z: &crate::scene::Zone| z.outline.len() >= 3 && !(z.locked && z.min_width == Some(0.0));
    let outer = |l: usize| l == 0 || l + 1 == nl;
    let mine = |z: &crate::scene::Zone| z.net as usize == g && drawn(z);
    let has_inner = sc.nets[g].plane.is_some() || sc.copper.zones.iter().any(|z| mine(z) && !outer(z.layer));
    let has_outer = sc.copper.zones.iter().any(|z| mine(z) && outer(z.layer));
    let mut layers: Vec<usize> = Vec::new();
    if !has_inner {
        layers.extend(free_inner(sc).first().copied());
    }
    if has_outer {
        return layers;
    }
    if has_inner || !layers.is_empty() {
        for l in [0, nl - 1] {
            if !layers.contains(&l) && !sc.copper.zones.iter().any(|z| z.layer == l && drawn(z) && z.net != 0 && z.priority == 0) {
                layers.push(l);
            }
        }
    } else {
        let back = sc.pads.iter().filter(|p| !p.tht && p.back).count();
        let front = sc.pads.iter().filter(|p| !p.tht && !p.back).count();
        layers.push(if front < back { 0 } else { nl - 1 });
    }
    layers
}

/// A layer given to a net: an inner layer is its plane, an outer one gets a pour. Either is drawn over
/// the whole board as a zone that meets the net's pads by reliefs (`Scene::draw_pour`).
pub fn give(sc: &mut Scene, net: usize, layer: usize) {
    if layer > 0 && layer + 1 < sc.layers.len() {
        sc.nets[net].plane = Some(layer);
    }
    sc.draw_pour(net, layer);
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
        for layer in ground_layers(sc, cfg, g) {
            give(sc, g, layer);
            out.push((g as u16, layer));
        }
    }
    out
}
