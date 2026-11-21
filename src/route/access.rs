//! Where a pad's track may start: its access nodes; and where a via may stand.

use super::Router;
use crate::field::sdist;
use std::collections::HashMap;
use crate::geom::{hypot, Shape};

/// A node a pad's track may start from or end on.
#[derive(Clone, Copy)]
pub struct Access {
    pub x: u16,
    pub y: u16,
    /// Some(point): the node is the end of a fence, which runs from the pad's centre through this point
    /// (where it leaves the pad, on the pad's centre line). None: straight from the centre.
    pub via: Option<(f64, f64)>,
    /// a fence that stands at its net's full width (false: at the narrowest the net may be drawn)
    pub wide: bool,
}

impl Router {
    pub(crate) fn pad_layers(&self, p: usize) -> Vec<usize> {
        let pad = &self.sc.pads[p];
        if pad.tht {
            self.routable.clone()
        } else if pad.back {
            vec![self.sc.layers.len() - 1]
        } else {
            vec![0]
        }
    }

    /// For each pad of a net to join: the grid nodes round its centre that lie in its copper and where a
    /// track of its net may stand.
    pub(crate) fn find_pad_nodes(&mut self) {
        let f = &self.f;
        let mut out: Vec<Vec<Access>> = vec![Vec::new(); self.sc.pads.len()];
        for (pi, pad) in self.sc.pads.iter().enumerate() {
            if pad.net == 0 || self.sc.nets[pad.net as usize].pads.len() < 2 {
                continue;
            }
            let t = self.t_min(pad.net);
            let l = if pad.tht { self.routable[0] } else if pad.back { f.nl - 1 } else { 0 };
            let fx = (pad.x - f.x0) / f.pitch;
            let fy = (pad.y - f.y0) / f.pitch;
            let mut cand: Vec<(f64, usize, usize)> = Vec::new();
            for reach in [1i32, 3] {
                let (cx, cy) = (fx.round() as i32, fy.round() as i32);
                for y in (cy - reach).max(0)..=(cy + reach).min(f.h as i32 - 1) {
                    for x in (cx - reach).max(0)..=(cx + reach).min(f.w as i32 - 1) {
                        let d = hypot(x as f64 - fx, y as f64 - fy);
                        if reach == 1 && d > 0.999 {
                            continue;
                        }
                        let (x, y) = (x as usize, y as usize);
                        if pad.shape.dist(f.wx(x), f.wy(y)) > 0.0 {
                            continue;
                        }
                        if sdist(&f.cells[f.idx(l, y, x)], pad.net) < t {
                            continue;
                        }
                        cand.push((d, x, y));
                    }
                }
                if !cand.is_empty() {
                    break;
                }
            }
            cand.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            // the nodes nearest the centre: all within one node's reach of the best
            if let Some(best) = cand.first().map(|c| c.0) {
                for &(d, x, y) in &cand {
                    if d <= best + 0.75 {
                        out[pi].push(Access { x: x as u16, y: y as u16, via: None, wide: false });
                    }
                }
            }
        }
        self.acc = out;
    }

    /// Where a via of this size may stand as far as fixed copper goes: clear of every pad and fence, its
    /// own net's too. Returns the sites, the nodes too near another hole, and each plane net's sites.
    fn sites_for(&self, t_via: crate::field::Dist, size: f64, drill: f64) -> (Vec<u64>, Vec<u64>, HashMap<u16, Vec<u64>>) {
        let f = &self.f;
        let mut planes: HashMap<u16, Vec<u64>> = HashMap::new();
        let mut via_ok = vec![0u64; f.w * f.h / 64 + 1];
        for y in 0..f.h {
            for x in 0..f.w {
                if (0..f.nl).all(|l| f.cells[f.idx(l, y, x)].sd1 >= t_via) {
                    let i = y * f.w + x;
                    via_ok[i >> 6] |= 1 << (i & 63);
                }
            }
        }
        // an area that forbids vias and not tracks: no via stands in it, on whatever layer it is drawn
        let vr = size / 2.0;
        for k in self.sc.keepouts.iter().filter(|k| k.forbid.vias && !k.forbid.tracks && k.poly.len() >= 3) {
            let area = Shape::Poly { pts: k.poly.clone(), r: vr };
            let b = area.bbox();
            let ((x0, y0), (x1, y1)) = (self.f.node_near(b.0, b.1), self.f.node_near(b.2, b.3));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if area.dist(self.f.wx(x), self.f.wy(y)) <= 0.0 {
                        let i = y * self.f.w + x;
                        via_ok[i >> 6] &= !(1 << (i & 63));
                    }
                }
            }
        }
        // a via's hole keeps the board's hole-to-hole distance from every other hole: a bare hole, a pad's
        let mut hole_near = vec![0u64; via_ok.len()];
        if let Some(h2h) = self.sc.rules.hole_to_hole {
            let mut holes: Vec<(f64, f64, f64)> = self.sc.holes.clone();
            holes.extend(self.sc.pads.iter().filter_map(|p| p.drill.filter(|d| *d > 0.0).map(|d| (p.x, p.y, d))));
            holes.extend(self.sc.copper.vias.iter().map(|v| (v.x, v.y, v.drill)));
            for (hx, hy, d) in holes {
                let need = d / 2.0 + drill / 2.0 + h2h + 0.005;
                let ((x0, y0), (x1, y1)) = (self.f.node_near(hx - need, hy - need), self.f.node_near(hx + need, hy + need));
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        if hypot(self.f.wx(x) - hx, self.f.wy(y) - hy) < need {
                            let i = y * self.f.w + x;
                            via_ok[i >> 6] &= !(1 << (i & 63));
                            hole_near[i >> 6] |= 1 << (i & 63);
                        }
                    }
                }
            }
        }
        // a plane a zone draws reaches as far as the zone does: a drop's via must stand inside it, a via
        // and a little more from its edge, and outside its cut-outs
        let reach = size / 2.0 + 0.3;
        for (ni, n) in self.sc.nets.iter().enumerate() {
            let Some(pl) = n.plane else { continue };
            let zones: Vec<&crate::scene::Zone> = self.sc.copper.zones.iter().filter(|z| z.net as usize == ni && z.layer == pl && z.outline.len() >= 3 && !(z.locked && z.min_width == Some(0.0))).collect();
            if zones.is_empty() {
                continue;
            }
            let shapes: Vec<(Shape, Vec<Shape>)> = zones.iter().map(|z| (Shape::Poly { pts: z.outline.clone(), r: 0.0 }, z.holes.iter().map(|h| Shape::Poly { pts: h.clone(), r: 0.0 }).collect())).collect();
            // and not into an area that forbids pours on its layer
            let mut barred: Vec<(Shape, f64)> = self.sc.keepouts.iter().filter(|k| k.forbid.zones && k.layers.contains(&pl) && k.poly.len() >= 3).map(|k| (Shape::Poly { pts: k.poly.clone(), r: 0.0 }, reach)).collect();
            // nor into another net's pour on its layer that is filled before it
            let first = zones.iter().map(|z| z.priority).min().unwrap_or(0);
            for o in &self.sc.copper.zones {
                if o.net as usize != ni && o.net != 0 && o.layer == pl && o.priority > first && o.outline.len() >= 3 {
                    barred.push((Shape::Poly { pts: o.outline.clone(), r: 0.0 }, reach + o.clearance.unwrap_or(0.0).max(self.sc.rules.clearance)));
                }
            }
            let mut sites = vec![0u64; via_ok.len()];
            for y in 0..self.f.h {
                for x in 0..self.f.w {
                    let i = y * self.f.w + x;
                    if via_ok[i >> 6] & (1 << (i & 63)) == 0 {
                        continue;
                    }
                    let (px, py) = (self.f.wx(x), self.f.wy(y));
                    if shapes.iter().any(|(o, holes)| o.depth(px, py) >= reach && holes.iter().all(|h| h.dist(px, py) >= reach)) && barred.iter().all(|(k, need)| k.dist(px, py) >= *need) {
                        sites[i >> 6] |= 1 << (i & 63);
                    }
                }
            }
            planes.insert(ni as u16, sites);
        }
        (via_ok, hole_near, planes)
    }

    /// The via sites of the class's via, and of the smaller one the board allows where it allows one.
    pub(crate) fn find_via_sites(&mut self) {
        let (via_ok, hole_near, planes) = self.sites_for(self.t_via, self.sc.rules.via, self.sc.rules.drill);
        self.via_ok = via_ok;
        self.hole_near = hole_near;
        self.plane_sites = planes;
        if let Some((size, drill)) = self.via_small {
            let (ok, _, planes) = self.sites_for(self.t_via_s, size, drill);
            self.via_ok_s = ok;
            self.plane_sites_s = planes;
        }
    }

    /// The pad's access entry at a node.
    pub(crate) fn access_at(&self, p: usize, x: u16, y: u16) -> Option<Access> {
        self.acc[p].iter().copied().find(|a| a.x == x && a.y == y)
    }

    pub(crate) fn pad_sources(&self, p: usize, out: &mut Vec<(u8, u16, u16, u32)>) {
        for a in &self.acc[p] {
            let d = if a.via.is_some() { 0.0 } else { hypot(self.f.wx(a.x as usize) - self.sc.pads[p].x, self.f.wy(a.y as usize) - self.sc.pads[p].y) };
            for &l in &self.pad_layers(p) {
                out.push((l as u8, a.y, a.x, (d * 1000.0) as u32));
            }
        }
    }
}
