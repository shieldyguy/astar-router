//! Plane drops: a surface pad of a plane net down into its plane.

use crate::field::{ddist, sdist};
use crate::geom::{hypot, Shape};
use super::{Pass, Step};
use crate::route::{Ask, Job, JobKind, Router};

pub const PASS: Pass = Pass {
    name: "drops",
    about: "every surface pad of a net with a plane goes down into it by a short track and a via; a pad may share a via its net has",
    step: Step::Plan(plan),
};

pub const DROP: JobKind = JobKind { name: "drop", pass: "drops", route: Router::route_drop };

/// One job a surface pad of a plane net. The net's through-hole pads are in the plane already, and so
/// is a surface pad on the plane's own layer, and one that old copper joins to a via or a through-hole
/// pad that reaches the plane (`route::old`).
fn plan(r: &mut Router) {
    for ni in 1..r.sc.nets.len() {
        if !r.sc.nets[ni].wanted() || r.sc.nets[ni].plane.is_none() {
            continue;
        }
        for k in 0..r.sc.nets[ni].pads.len() {
            let p = r.sc.nets[ni].pads[k];
            if r.sc.pads[p].tht {
                if let Some(a) = r.acc[p].first() {
                    let at = (a.x, a.y);
                    r.net_tht[ni].push(at);
                }
            } else if !r.pad_in_plane(p) && !r.old_reaches_plane(p) {
                r.add_job(Job::new(DROP, ni as u16, Some(p)));
            }
        }
    }
}

impl Router {
    /// Is this pad of a plane net in its plane as it stands: a through-hole pad, or a surface pad on the
    /// plane's layer, that does not lie in an area where the plane's layer may not be poured?
    pub fn pad_in_plane(&self, p: usize) -> bool {
        let pad = &self.sc.pads[p];
        let surface = if pad.back { self.sc.layers.len() - 1 } else { 0 };
        let Some(pl) = self.sc.nets[pad.net as usize].plane else { return false };
        (pad.tht || pl == surface) && !self.sc.keepouts.iter().any(|k| k.forbid.zones && k.layers.contains(&pl) && k.poly.len() >= 3 && Shape::Poly { pts: k.poly.clone(), r: 0.0 }.dist(pad.x, pad.y) == 0.0)
    }

    /// Does copper the board came with join this pad of a plane net to its plane already: a track to a
    /// via, or to a through-hole pad, that goes through the plane's layer?
    pub(crate) fn old_reaches_plane(&self, p: usize) -> bool {
        let Some(pl) = self.sc.nets[self.sc.pads[p].net as usize].plane else { return false };
        self.old.of_pad.get(&p).map_or(false, |&k| self.old.isles[k].layers & (1 << pl) != 0)
    }

    /// One surface pad of a plane net down into its plane: a short track and a via, or a track to a via,
    /// a through-hole pad or a surface pad of its net that is in the plane already.
    pub(crate) fn route_drop(&mut self, j: usize) {
        let net = self.jobs[j].net;
        let p = self.jobs[j].pad.unwrap();
        let pl = self.sc.nets[net as usize].plane.unwrap();
        // a pad that old copper joins to another pad of the net whose own drop stands is in the plane
        // through that pad, and stands or falls with its drop
        let isle = self.old.of_pad.get(&p).copied();
        for &q in isle.map_or(&[][..], |k| &self.old.isles[k].pads[..]) {
            let Some(&jq) = self.drop_of.get(&q) else { continue };
            if q != p && self.jobs[jq].done && self.jobs[jq].open.is_empty() && !self.jobs[jq].objs.is_empty() {
                if !self.deps[jq].contains(&j) {
                    self.deps[jq].push(j);
                }
                self.jobs[j].done = true;
                return;
            }
        }
        // an exposed pad (inside its part's ring of pins) goes down by a via in itself: no thin track from
        // it to a via among the pins
        if self.ringed(p) && self.drop_in_pad(j) {
            self.jobs[j].done = true;
            return;
        }
        let mut sources = Vec::new();
        self.pad_sources(p, &mut sources);
        // (else it goes down itself, from the pad or from anywhere on that old copper)
        if let Some(k) = isle {
            sources.extend(self.old.isles[k].nodes.iter().map(|&(l, y, x)| (l, y, x, 0)));
        }
        if sources.is_empty() {
            self.jobs[j].open = vec![p];
            self.jobs[j].walled = vec![p];
            self.jobs[j].done = true;
            return;
        }
        let mut own: Vec<(u16, u16)> = self.net_vias[net as usize].clone();
        own.extend(self.net_tht[net as usize].iter().copied());
        // surface pads of the net whose own drop stands: reaching one is reaching the plane
        let mut own_pads: Vec<(u8, u16, u16)> = Vec::new();
        let mut own_pad_ids: Vec<usize> = Vec::new();
        for &q in &self.sc.nets[net as usize].pads {
            if q == p || self.sc.pads[q].tht {
                continue;
            }
            let Some(&jq) = self.drop_of.get(&q) else { continue };
            if !self.jobs[jq].done || !self.jobs[jq].open.is_empty() {
                continue;
            }
            for a in &self.acc[q] {
                for &l in &self.pad_layers(q) {
                    own_pads.push((l as u8, a.x, a.y));
                    own_pad_ids.push(q);
                }
            }
        }
        let ask = Ask {
            net,
            sources,
            tbox: (0, 0, 0, 0),
            tlayers: 0,
            plane_goal: Some(pl as u8),
            own_vias: own,
            own_pads,
            via_mm: self.cfg.plane_via_mm,
            rip_mm: self.cfg.rip_drop_mm,
            hard: 20_000,
            cap: 200_000,
            shut: false,
            what: if self.cfg.verbose { format!("drop {} {}.{}", self.sc.nets[net as usize].name, self.sc.pads[p].ref_, self.sc.pads[p].num) } else { String::new() },
        };
        let Some((path, may_clash)) = self.solve(&ask) else {
            // a big pad that cannot be left (an exposed pad inside its ring of pins) takes its via itself
            // (a drop the board itself leaves no way is not open for want of room: `Job::walled`)
            let no_way = self.no_way;
            if !self.drop_in_pad(j) {
                self.jobs[j].open = vec![p];
                if !self.strict {
                    self.jobs[j].walled = if no_way { vec![p] } else { Vec::new() };
                }
            }
            self.jobs[j].done = true;
            return;
        };
        if may_clash {
            self.note_clashes(j, &path);
        }
        // (a path laid through other copper is kept as found: pulling it tight would need that copper gone)
        let as_found = may_clash && !self.jobs[j].clash_cells.is_empty();
        let path = if as_found { path } else { self.shape(net, path) };
        // the last node is on the plane layer; the one before is where the track ends: on a through-hole
        // pad or a surface pad of the net (the track ends at that pad), on a via the net has, or on a new via
        let end = path[path.len() - 2];
        let mut to_pad = self.sc.nets[net as usize].pads.iter().copied().find(|&q| self.sc.pads[q].tht && self.acc[q].first().map(|a| (a.x, a.y)) == Some((end.2, end.1)));
        let mut leans_on: Option<usize> = None;
        if to_pad.is_none() {
            if let Some(k) = ask.own_pads.iter().position(|&(l, x, y)| l == end.0 && x == end.2 && y == end.1) {
                to_pad = Some(own_pad_ids[k]);
                leans_on = self.drop_of.get(&own_pad_ids[k]).copied();
            } else {
                leans_on = self.via_owner.get(&(net, end.2, end.1)).copied();
            }
        }
        // (a path that left from old copper starts there, not at the pad)
        let at_pad = self.pad_layers(p).contains(&(path[0].0 as usize)) && self.access_at(p, path[0].2, path[0].1).is_some();
        self.commit(j, &path, if at_pad { Some(p) } else { None }, to_pad);
        self.jobs[j].rough |= as_found;
        // a drop that ends on another drop's copper stands or falls with it
        if let Some(o) = leans_on {
            if o != j && !self.deps[o].contains(&j) {
                self.deps[o].push(j);
            }
        }
        self.jobs[j].done = true;
    }

    /// Where a via can stand inside a pad: only in a pad big enough to hold one well inside its copper,
    /// and only where that via is clear of every other net on every layer.
    pub(crate) fn via_site_in_pad(&self, net: u16, p: usize) -> Option<(usize, usize)> {
        let pad = &self.sc.pads[p];
        let (bx0, by0, bx1, by1) = pad.shape.bbox();
        if (bx1 - bx0).min(by1 - by0) < self.cfg.vip_mm {
            return None;
        }
        let kind = self.kind(net);
        let need = kind.size / 2.0 + 0.05;
        let (x0, y0) = self.f.node_near(bx0, by0);
        let (x1, y1) = self.f.node_near(bx1, by1);
        let mut cand: Vec<(f64, usize, usize)> = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (self.f.wx(x), self.f.wy(y));
                if pad.shape.depth(px, py) < need {
                    continue;
                }
                // (a pad that holds its footprint's own thermal vias: none of ours on top of them)
                let i = y * self.f.w + x;
                if kind.hole_near.get(i >> 6).map_or(false, |w| w & (1 << (i & 63)) != 0) {
                    continue;
                }
                let vl = self.costs.via_net[y * self.f.w + x];
                if vl != 0 && vl != net {
                    continue;
                }
                // (where its plane is: the plane's own sites leave out every pad, this one too)
                if !self.plane_holds(net, px, py) {
                    continue;
                }
                if !(0..self.f.nl).all(|ll| {
                    let c = &self.f.cells[self.f.idx(ll, y, x)];
                    sdist(c, net) >= kind.t && ddist(c, net) >= kind.t
                }) {
                    continue;
                }
                if self.net_vias[net as usize].iter().any(|&(vx, vy)| {
                    let (dx, dy) = (vx as i64 - x as i64, vy as i64 - y as i64);
                    dx * dx + dy * dy < kind.gap2
                }) {
                    continue;
                }
                cand.push((hypot(px - pad.x, py - pad.y), x, y));
            }
        }
        cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        cand.first().map(|&(_, x, y)| (x, y))
    }

    /// A via inside the pad of a plane drop (an exposed pad inside its ring of pins). True when it was laid.
    pub(crate) fn drop_in_pad(&mut self, j: usize) -> bool {
        let net = self.jobs[j].net;
        let p = self.jobs[j].pad.unwrap();
        let pl = self.sc.nets[net as usize].plane.unwrap();
        let Some((x, y)) = self.via_site_in_pad(net, p) else { return false };
        let l = if self.sc.pads[p].back { self.f.nl - 1 } else { 0 };
        let path = [(l as u8, y as u16, x as u16), (pl as u8, y as u16, x as u16)];
        self.commit(j, &path, Some(p), None);
        true
    }
}
