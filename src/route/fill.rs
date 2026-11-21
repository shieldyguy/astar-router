//! The pour model: where a pour's copper will stand once KiCad has filled it round the copper there is,
//! and which pads of its net that copper joins.
//!
//! A model, kept on the safe side: where it is in doubt whether copper stands, it says none does (the
//! net then gets a track it may not have needed). A node is in the fill when a disc of half the pour's
//! least width fits there at the pour's clearance from everything of another net: the fill's spine. Two
//! spine nodes side by side are one piece of copper.

use super::{Router, Track};
use crate::field::{DynObj, Field, Kind, ALL_LAYERS};
use crate::geom::{hypot, Shape};
use crate::scene::{Zone, ZonePads};
use std::collections::HashMap;

/// kept beyond every distance the fill needs, mm: the model errs toward no copper
const EPS: f64 = 0.03;

#[derive(Clone, Copy)]
enum Join {
    Solid,
    /// a thermal relief with this gap
    Spokes(f64),
}

/// What never changes about a net's pours.
pub(crate) struct PourBase {
    /// the layers the net has a pour on
    layers: Vec<usize>,
    /// per node, a bit: far enough from every fixed thing of another net
    open: Vec<u64>,
    /// per node, a bit: in the gap a thermal relief leaves round a pad of the net
    gap: Vec<u64>,
    /// per layer: the clearance the pour keeps from other nets' copper, and half its least width
    clr: Vec<f64>,
    half: Vec<f64>,
    /// per pad of the net, in the net's order: the layers a pour joins it on, and how
    joins: Vec<Vec<(usize, Join)>>,
}

/// A net's pours as they would be filled now.
pub(crate) struct Pour {
    /// per node: the piece of fill it is in (0: none)
    pub comp: Vec<u32>,
    /// the nodes of each piece
    pub cells: Vec<Vec<u32>>,
    /// per piece, and per pad of the net (in the net's order): its group. Pads and pieces in one group
    /// are joined by the pour.
    pub comp_group: Vec<u32>,
    pub pad_group: Vec<u32>,
    /// every thermal relief: (the pad, in the net's order; the layer; its gap; the piece each of its
    /// spokes lands in)
    pub reliefs: Vec<(u32, u8, f64, Vec<u32>)>,
    /// every (pad, piece) the pour joins
    pub joined: Vec<(u32, u32)>,
}

#[inline]
fn get(b: &[u64], i: usize) -> bool {
    b[i >> 6] & (1 << (i & 63)) != 0
}
#[inline]
pub(crate) fn set(b: &mut [u64], i: usize) {
    b[i >> 6] |= 1 << (i & 63);
}
#[inline]
fn unset(b: &mut [u64], i: usize) {
    b[i >> 6] &= !(1 << (i & 63));
}

/// Every node inside a polygon, a row at a time.
pub(crate) fn scan(f: &Field, pts: &[(f64, f64)], mut each: impl FnMut(usize, usize, usize)) {
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for p in pts {
        lo = lo.min(p.1);
        hi = hi.max(p.1);
    }
    let ya = ((lo - f.y0) / f.pitch).ceil().max(0.0) as usize;
    let yb = ((hi - f.y0) / f.pitch).floor().min((f.h - 1) as f64);
    if yb < 0.0 {
        return;
    }
    let mut xs: Vec<f64> = Vec::new();
    for y in ya..=yb as usize {
        let py = f.wy(y);
        xs.clear();
        let mut j = pts.len() - 1;
        for i in 0..pts.len() {
            let ((xi, yi), (xj, yj)) = (pts[i], pts[j]);
            if (yi > py) != (yj > py) {
                xs.push((xj - xi) * (py - yi) / (yj - yi) + xi);
            }
            j = i;
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in xs.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let xa = ((pair[0] - f.x0) / f.pitch).ceil().max(0.0);
            let xb = ((pair[1] - f.x0) / f.pitch).floor().min((f.w - 1) as f64);
            if xa <= xb {
                each(y, xa as usize, xb as usize);
            }
        }
    }
}

/// Every node nearer a shape than `need`.
fn near(f: &Field, shape: &Shape, need: f64, mut each: impl FnMut(usize)) {
    let (bx0, by0, bx1, by1) = shape.bbox();
    let Some((x0, y0, x1, y1)) = f.range(bx0 - need, by0 - need, bx1 + need, by1 + need) else { return };
    for y in y0..=y1 {
        let py = f.wy(y);
        for x in x0..=x1 {
            if shape.dist(f.wx(x), py) < need {
                each(y * f.w + x);
            }
        }
    }
}

/// Every node nearer a closed line than `need`.
fn ring(f: &Field, pts: &[(f64, f64)], need: f64, mut each: impl FnMut(usize)) {
    for k in 0..pts.len() {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        near(f, &Shape::Capsule { ax: a.0, ay: a.1, bx: b.0, by: b.1, r: 0.0 }, need, &mut each);
    }
}

/// Where the four spokes of a pad's thermal relief end, as KiCad builds them: from the middle of the pad's
/// shape, at `angle` (degrees) in the pad's own frame, out to its box grown by `grow`. Each end with the
/// unit vector the spoke runs along.
fn spoke_ends(shape: &Shape, angle: f64, grow: f64) -> Vec<((f64, f64), (f64, f64))> {
    // the pad's frame: its middle, its x axis, its half sizes; and whether it is round
    let (c, u, hx, hy, round) = match shape {
        Shape::Circle { x, y, r } => ((*x, *y), (1.0, 0.0), *r, *r, true),
        Shape::Capsule { ax, ay, bx, by, r } => {
            let d = hypot(bx - ax, by - ay);
            if d < 1e-9 {
                ((*ax, *ay), (1.0, 0.0), *r, *r, true)
            } else {
                (((ax + bx) / 2.0, (ay + by) / 2.0), ((bx - ax) / d, (by - ay) / d), d / 2.0 + r, *r, false)
            }
        }
        Shape::Poly { pts, r } => {
            let side = if pts.len() == 4 { hypot(pts[1].0 - pts[0].0, pts[1].1 - pts[0].1) } else { 0.0 };
            if side > 1e-9 {
                let c = (pts.iter().map(|p| p.0).sum::<f64>() / 4.0, pts.iter().map(|p| p.1).sum::<f64>() / 4.0);
                (c, ((pts[1].0 - pts[0].0) / side, (pts[1].1 - pts[0].1) / side), side / 2.0 + r, hypot(pts[2].0 - pts[1].0, pts[2].1 - pts[1].1) / 2.0 + r, false)
            } else {
                let (x0, y0, x1, y1) = shape.bbox();
                (((x0 + x1) / 2.0, (y0 + y1) / 2.0), (1.0, 0.0), (x1 - x0) / 2.0, (y1 - y0) / 2.0, false)
            }
        }
    };
    let (hx, hy) = (hx + grow, hy + grow);
    (0..4)
        .map(|k| {
            let a = (angle + 90.0 * k as f64).to_radians();
            let (dx, dy) = (a.cos(), a.sin());
            // to the side of the box the spoke meets first; a round pad's spokes are all one length
            let t = if round {
                hx
            } else if dx.abs() < 1e-9 {
                hy
            } else if dy.abs() < 1e-9 {
                hx
            } else {
                (hx / dx.abs()).min(hy / dy.abs())
            };
            let (px, py) = (dx * t, dy * t);
            ((c.0 + px * u.0 - py * u.1, c.1 + px * u.1 + py * u.0), (dx * u.0 - dy * u.1, dx * u.1 + dy * u.0))
        })
        .collect()
}

impl Router {
    /// The pours of a net that the board fills again round new copper: not a plane (the drops' business),
    /// not a shape drawn in copper.
    fn pour_zones(&self, net: u16) -> Vec<&Zone> {
        self.sc.copper.zones.iter().filter(|z| z.net == net && z.outline.len() >= 3 && !(z.locked && z.min_width == Some(0.0)) && self.routable.contains(&z.layer)).collect()
    }

    /// What never changes about a net's pours; None for a net without one.
    pub(crate) fn pour_base(&self, net: u16) -> Option<PourBase> {
        let zones = self.pour_zones(net);
        if zones.is_empty() || net == 0 {
            return None;
        }
        let f = &self.f;
        let (w, per, nl) = (f.w, f.w * f.h, f.nl);
        let mut open = vec![0u64; per * nl / 64 + 1];
        let mut gap = vec![0u64; per * nl / 64 + 1];
        let (mut clr, mut half) = (vec![0.0f64; nl], vec![0.0f64; nl]);
        let mut layers: Vec<usize> = Vec::new();
        let board = self.sc.rules.clearance;
        let pads: Vec<usize> = self.sc.nets[net as usize].pads.clone();
        let mut joins: Vec<Vec<(usize, Join)>> = vec![Vec::new(); pads.len()];
        for z in &zones {
            let l = z.layer;
            let zc = z.clearance.unwrap_or(0.0).max(self.net_clr(net));
            let hw = z.min_width.unwrap_or(0.25) / 2.0;
            clr[l] = clr[l].max(zc);
            half[l] = half[l].max(hw);
            if !layers.contains(&l) {
                layers.push(l);
            }
            // inside its outline, half its least width from the line itself
            let mut zb = vec![0u64; per / 64 + 1];
            scan(f, &z.outline, |y, xa, xb| {
                for x in xa..=xb {
                    set(&mut zb, y * w + x);
                }
            });
            ring(f, &z.outline, hw + EPS, |i| unset(&mut zb, i));
            // not in its holes, in an area that keeps pours out, or in another net's pour that goes first
            let cut = |pts: &[(f64, f64)], need: f64, zb: &mut Vec<u64>| {
                scan(f, pts, |y, xa, xb| {
                    for x in xa..=xb {
                        unset(zb, y * w + x);
                    }
                });
                ring(f, pts, need, |i| unset(zb, i));
            };
            for h in &z.holes {
                if h.len() >= 3 {
                    cut(h, hw + EPS, &mut zb);
                }
            }
            for k in &self.sc.keepouts {
                if k.forbid.zones && k.layers.contains(&l) && k.poly.len() >= 3 {
                    cut(&k.poly, hw + EPS, &mut zb);
                }
            }
            for o in &self.sc.copper.zones {
                if o.net != net && o.net != 0 && o.layer == l && o.priority > z.priority && o.outline.len() >= 3 && !(o.locked && o.min_width == Some(0.0)) {
                    cut(&o.outline, zc.max(o.clearance.unwrap_or(0.0)) + hw + EPS, &mut zb);
                }
            }
            // far enough from every fixed thing of another net: its copper by the pour's clearance, the
            // edge and the holes by their own
            for o in &self.statics.objs {
                if o.lmask & (1 << l) == 0 || o.kind == Kind::Keepout {
                    continue;
                }
                let copper = !matches!(o.kind, Kind::Edge | Kind::Hole);
                if o.net == net && (copper || o.net != 0) {
                    continue;
                }
                let need = if copper { zc.max(board + o.extra) } else { board + o.extra } + hw + EPS;
                near(f, &o.shape, need, |i| unset(&mut zb, i));
            }
            // how this zone joins the net's pads that stand in it
            for (i, &p) in pads.iter().enumerate() {
                let pad = &self.sc.pads[p];
                let on = if pad.tht { true } else { l == if pad.back { nl - 1 } else { 0 } };
                if !on || joins[i].iter().any(|&(ll, _)| ll == l) {
                    continue;
                }
                let inside = Shape::Poly { pts: z.outline.clone(), r: 0.0 }.dist(pad.x, pad.y) == 0.0 && !z.holes.iter().any(|h| h.len() >= 3 && Shape::Poly { pts: h.clone(), r: 0.0 }.dist(pad.x, pad.y) == 0.0);
                if !inside {
                    continue;
                }
                let thermal = match pad.pour.unwrap_or(z.pads) {
                    ZonePads::Solid => false,
                    ZonePads::Thermal => true,
                    ZonePads::ThermalTht => pad.tht,
                    ZonePads::None => {
                        // the pour keeps off the pad as off another net's
                        near(f, &pad.shape, zc.max(board + pad.clearance) + hw + EPS, |i| set(&mut gap, l * per + i));
                        continue;
                    }
                };
                if thermal {
                    let g = pad.thermal_gap.or(z.thermal_gap).unwrap_or(0.5);
                    near(f, &pad.shape, g + hw + EPS, |i| set(&mut gap, l * per + i));
                    // a spoke is no wider than the pad is across, and KiCad draws none narrower than the
                    // pour's least width: such a pad stands in its gap, joined to nothing
                    let across = match &pad.shape {
                        Shape::Circle { r, .. } | Shape::Capsule { r, .. } => 2.0 * r,
                        Shape::Poly { pts, r } if pts.len() == 4 => hypot(pts[1].0 - pts[0].0, pts[1].1 - pts[0].1).min(hypot(pts[2].0 - pts[1].0, pts[2].1 - pts[1].1)) + 2.0 * r,
                        s => {
                            let (x0, y0, x1, y1) = s.bbox();
                            (x1 - x0).min(y1 - y0)
                        }
                    };
                    if z.spoke_width.unwrap_or(0.5).min(across) + 1e-6 >= 2.0 * hw {
                        joins[i].push((l, Join::Spokes(g)));
                    }
                } else {
                    joins[i].push((l, Join::Solid));
                }
            }
            for i in 0..per {
                if get(&zb, i) {
                    set(&mut open, l * per + i);
                }
            }
        }
        Some(PourBase { layers, open, gap, clr, half, joins })
    }

    /// A net's pours as KiCad would fill them round the copper that stands now; None for a net without one.
    pub(crate) fn pour(&self, net: u16) -> Option<Pour> {
        let base = self.pours.get(&net)?;
        let f = &self.f;
        let (w, h, per) = (f.w, f.h, f.w * f.h);
        let mut open = base.open.clone();
        for o in &f.objs {
            if !o.alive || o.net == net {
                continue;
            }
            let real = o.r - self.more_clr(o.net);
            let shape = Shape::Capsule { ax: o.ax, ay: o.ay, bx: o.bx, by: o.by, r: 0.0 };
            for &l in &base.layers {
                if o.layer != ALL_LAYERS && o.layer as usize != l {
                    continue;
                }
                let need = real + base.clr[l].max(self.net_clr(o.net)) + base.half[l] + EPS;
                near(f, &shape, need, |i| unset(&mut open, l * per + i));
            }
        }
        let fill = |i: usize| get(&open, i) && !get(&base.gap, i);
        // the pieces
        let mut comp = vec![0u32; per * f.nl];
        let mut cells: Vec<Vec<u32>> = vec![Vec::new()];
        let mut stack: Vec<usize> = Vec::new();
        for &l in &base.layers {
            for i0 in l * per..(l + 1) * per {
                if comp[i0] != 0 || !fill(i0) {
                    continue;
                }
                let id = cells.len() as u32;
                let mut mine: Vec<u32> = Vec::new();
                comp[i0] = id;
                stack.push(i0);
                while let Some(i) = stack.pop() {
                    mine.push(i as u32);
                    let (x, y) = ((i - l * per) % w, (i - l * per) / w);
                    let mut go = |k: usize, stack: &mut Vec<usize>| {
                        if comp[k] == 0 && fill(k) {
                            comp[k] = id;
                            stack.push(k);
                        }
                    };
                    if x > 0 {
                        go(i - 1, &mut stack);
                    }
                    if x + 1 < w {
                        go(i + 1, &mut stack);
                    }
                    if y > 0 {
                        go(i - w, &mut stack);
                    }
                    if y + 1 < h {
                        go(i + w, &mut stack);
                    }
                }
                cells.push(mine);
            }
        }
        // pads and pieces into groups
        let pads = &self.sc.nets[net as usize].pads;
        let n = pads.len();
        let mut up: Vec<u32> = (0..(n + cells.len()) as u32).collect();
        fn find(up: &mut Vec<u32>, mut a: u32) -> u32 {
            while up[a as usize] != a {
                up[a as usize] = up[up[a as usize] as usize];
                a = up[a as usize];
            }
            a
        }
        let join = |up: &mut Vec<u32>, a: u32, b: u32| {
            let (ra, rb) = (find(up, a), find(up, b));
            if ra != rb {
                up[ra.max(rb) as usize] = ra.min(rb);
            }
        };
        let mut reliefs: Vec<(u32, u8, f64, Vec<u32>)> = Vec::new();
        let mut joined: Vec<(u32, u32)> = Vec::new();
        for (i, &p) in pads.iter().enumerate() {
            let pad = &self.sc.pads[p];
            for &(l, how) in &base.joins[i] {
                if let Join::Spokes(g) = how {
                    reliefs.push((i as u32, l as u8, g, Vec::new()));
                }
                match how {
                    Join::Solid => {
                        // the pour runs over the pad: any fill at the pad's copper is one with it
                        let mut hit: Vec<u32> = Vec::new();
                        near(f, &pad.shape, f.pitch * 0.75, |k| {
                            let c = comp[l * per + k];
                            if c != 0 && !hit.contains(&c) {
                                hit.push(c);
                            }
                        });
                        for c in hit {
                            join(&mut up, i as u32, n as u32 + c);
                            joined.push((i as u32, c));
                        }
                    }
                    Join::Spokes(g) => {
                        let angle = pad.spoke_angle.unwrap_or(90.0);
                        for ((ex, ey), (dx, dy)) in spoke_ends(&pad.shape, angle, g + base.half[l] + 0.005) {
                            // KiCad keeps a spoke whose end is in the fill; here the end must be clear of
                            // other nets on the four nodes round it, and the way to it from the pad too
                            let (x0, y0) = (((ex - f.x0) / f.pitch).floor(), ((ey - f.y0) / f.pitch).floor());
                            if x0 < 1.0 || y0 < 1.0 || x0 + 2.0 >= w as f64 || y0 + 2.0 >= h as f64 {
                                continue;
                            }
                            let (x0, y0) = (x0 as usize, y0 as usize);
                            let at = |x: usize, y: usize| l * per + y * w + x;
                            if ![at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1)].iter().all(|&k| get(&open, k)) {
                                continue;
                            }
                            let len = hypot(ex - pad.x, ey - pad.y);
                            let mut t = 0.0;
                            let mut clear = true;
                            while t < len {
                                let (px, py) = (ex - dx * t, ey - dy * t);
                                if pad.shape.dist(px, py) <= 0.0 {
                                    break;
                                }
                                let (x, y) = f.node_near(px, py);
                                clear &= get(&open, at(x, y));
                                t += f.pitch / 2.0;
                            }
                            if !clear {
                                continue;
                            }
                            // the fill the spoke runs into: the first there is from its end outward
                            let mut t = 0.0;
                            while t <= 3.0 * f.pitch {
                                let (x, y) = f.node_near(ex + dx * t, ey + dy * t);
                                let c = comp[at(x, y)];
                                if c != 0 {
                                    join(&mut up, i as u32, n as u32 + c);
                                    joined.push((i as u32, c));
                                    reliefs.last_mut().unwrap().3.push(c);
                                    break;
                                }
                                t += f.pitch / 2.0;
                            }
                        }
                    }
                }
            }
        }
        let pad_group: Vec<u32> = (0..n as u32).map(|i| find(&mut up, i)).collect();
        let comp_group: Vec<u32> = (0..cells.len() as u32).map(|c| find(&mut up, n as u32 + c)).collect();
        Some(Pour { comp, cells, comp_group, pad_group, reliefs, joined })
    }

    /// Does job j stand on fill that is no longer there: pads it took as joined by the pour that the pour
    /// no longer joins, or a track that ends in fill since cut off?
    pub(crate) fn pour_stale(&self, j: usize) -> bool {
        let job = &self.jobs[j];
        if job.pour_pads.is_empty() || !job.done {
            return false;
        }
        let Some(pm) = self.pour(job.net) else { return false };
        let mut now: HashMap<u32, u32> = HashMap::new();
        for (i, &old) in job.pour_pads.iter().enumerate() {
            if *now.entry(old).or_insert(pm.pad_group[i]) != pm.pad_group[i] {
                return true;
            }
        }
        // (an end leans on the piece it is in being one with what the piece was one with: the pads of its
        // group then, and the other ends in it, which is all there is where the pour touches no pad)
        job.pour_ends.iter().any(|&(cell, old)| {
            let c = pm.comp[cell as usize];
            c == 0 || *now.entry(old).or_insert(pm.comp_group[c as usize]) != pm.comp_group[c as usize]
        })
    }

    /// Is the straight piece a-b of this net, `half` wide each side, clear of everything of another net
    /// on layer l, by the exact shapes?
    pub(crate) fn piece_clear(&self, a: (f64, f64), b: (f64, f64), half: f64, net: u16, l: usize) -> bool {
        self.statics.track_clear(a, b, half, self.net_clr(net), 1 << l, net) && self.piece_off(a, b, half, net, l)
    }

    /// Does the piece keep its clearance from every other net's routed copper on layer l? (To within
    /// `SLACK`, as everywhere.)
    pub(crate) fn piece_off(&self, a: (f64, f64), b: (f64, f64), half: f64, net: u16, l: usize) -> bool {
        self.tight().routed(&self.f, a, b, half, net, l)
    }

    /// A thermal relief that other nets' copper has left with too few spokes, or whose spokes all run
    /// into fill that reaches nothing else, is what KiCad's DRC calls starved. Such a pad gets a short
    /// track of its own from its middle into the fill its spokes land in: KiCad counts that track as a
    /// spoke, and the fill then joins two things. Returns how many were laid.
    pub(crate) fn relieve(&mut self) -> usize {
        let min = self.sc.rules.min_spokes.unwrap_or(2.0).round() as usize;
        let mut laid = 0;
        if min < 2 {
            return 0;
        }
        let mut nets: Vec<u16> = self.pours.keys().copied().collect();
        nets.sort();
        let (w, per) = (self.f.w, self.f.w * self.f.h);
        for net in nets {
            let Some(j) = (0..self.jobs.len()).find(|&j| self.jobs[j].net == net && !self.jobs[j].pour_pads.is_empty() && !self.jobs[j].locked) else { continue };
            let Some(pm) = self.pour(net) else { continue };
            let pads: Vec<usize> = self.sc.nets[net as usize].pads.clone();
            // what each piece of fill joins: pads, and ends of the net's tracks
            let mut things = vec![0u32; pm.cells.len()];
            let mut seen = pm.joined.clone();
            seen.sort();
            seen.dedup();
            for &(_, c) in &seen {
                things[c as usize] += 1;
            }
            let piece_at = |l: usize, p: (f64, f64)| {
                let (x, y) = self.f.node_near(p.0, p.1);
                pm.comp[l * per + y * w + x] as usize
            };
            for t in &self.jobs[j].tracks {
                for &p in &t.pts {
                    things[piece_at(t.layer as usize, p)] += 1;
                }
            }
            let half = self.w_min(net) / 2.0;
            let mut stubs: Vec<(usize, (f64, f64), (f64, f64))> = Vec::new();
            for (i, l, g, lands) in &pm.reliefs {
                let (l, pad) = (*l as usize, &self.sc.pads[pads[*i as usize]]);
                let good = lands.iter().filter(|&&c| things[c as usize] >= 2).count();
                if good >= min {
                    continue;
                }
                // a track of the net that leaves the pad counts as a spoke when its far end lies over this
                // layer's fill, whatever layer the track is on (so KiCad counts them)
                let (mut manual, mut leaves) = (0, false);
                for t in self.jobs[j].tracks.iter().filter(|t| pad.tht || t.layer as usize == l) {
                    for s in t.pts.windows(2) {
                        let far = if pad.shape.dist(s[0].0, s[0].1) <= g / 2.0 {
                            s[1]
                        } else if pad.shape.dist(s[1].0, s[1].1) <= g / 2.0 {
                            s[0]
                        } else {
                            continue;
                        };
                        leaves = true;
                        manual += (piece_at(l, far) != 0) as usize;
                    }
                }
                // enough for sure; or spokes into fill that joins nothing else and no track that might be
                // counted beside them, which KiCad leaves to its connectivity check
                if good + manual >= min || (good == 0 && !leaves && lands.len() >= min) {
                    continue;
                }
                // the nearest node well inside that fill which a straight track from the pad's middle
                // reaches clear of every other net
                let (bx0, by0, bx1, by1) = pad.shape.bbox();
                let reach = g + 1.0;
                let Some((x0, y0, x1, y1)) = self.f.range(bx0 - reach, by0 - reach, bx1 + reach, by1 + reach) else { continue };
                // (a relief the model sees no spoke of may have one all the same, where the fill comes up to
                // its gap: the model errs toward none. KiCad passes over a relief with no spoke at all, so
                // a track too many there does no harm)
                let mut best: Option<(f64, (f64, f64))> = None;
                let mut at_gap = false;
                for y in y0.max(1)..=y1.min(self.f.h - 2) {
                    for x in x0.max(1)..=x1.min(w - 2) {
                        let k = l * per + y * w + x;
                        let c = pm.comp[k];
                        if c == 0 {
                            continue;
                        }
                        let e = (self.f.wx(x), self.f.wy(y));
                        at_gap |= pad.shape.dist(e.0, e.1) <= g + 0.6;
                        if ![k - 1, k + 1, k - w, k + w].iter().all(|&q| pm.comp[q] == c) {
                            continue;
                        }
                        // into the fill its spokes land in if there is a way, else into any
                        let d = hypot(e.0 - pad.x, e.1 - pad.y) + if lands.contains(&c) { 0.0 } else { 100.0 };
                        if best.map_or(true, |(bd, _)| d < bd) && self.piece_clear((pad.x, pad.y), e, half, net, l) {
                            best = Some((d, e));
                        }
                    }
                }
                if lands.is_empty() && !at_gap {
                    continue;
                }
                if let Some((_, e)) = best {
                    stubs.push((l, (pad.x, pad.y), e));
                }
            }
            let more = self.more_clr(net);
            for (l, a, b) in stubs {
                let id = self.f.add_obj(DynObj { kind: Kind::Track, net, job: j as u32, locked: false, layer: l as u8, ax: a.0, ay: a.1, bx: b.0, by: b.1, r: half + more, alive: true });
                self.jobs[j].objs.push(id);
                self.jobs[j].tracks.push(Track { layer: l as u8, width: 2.0 * half, pts: vec![a, b], arcs: Vec::new() });
                laid += 1;
            }
        }
        laid
    }

    /// (for looking at the model) Every net's pours as the model has them, one grey image a net and layer:
    /// `DIR/NET.LAYER.pgm`, a pixel a node; fill is light, lighter the bigger its group.
    pub(crate) fn dump_pours(&self, dir: &str) {
        let (w, h, per) = (self.f.w, self.f.h, self.f.w * self.f.h);
        let mut nets: Vec<u16> = self.pours.keys().copied().collect();
        nets.sort();
        for net in nets {
            let Some(pm) = self.pour(net) else { continue };
            let base = &self.pours[&net];
            let mut size: HashMap<u32, usize> = HashMap::new();
            for &g in &pm.pad_group {
                *size.entry(g).or_insert(0) += 1;
            }
            eprintln!("pour {}: {} pads in {} groups, {} pieces of fill; x0 {} y0 {} pitch {}", self.sc.nets[net as usize].name, pm.pad_group.len(), size.len(), pm.cells.len() - 1, self.f.x0, self.f.y0, self.f.pitch);
            for (i, &p) in self.sc.nets[net as usize].pads.iter().enumerate() {
                let pad = &self.sc.pads[p];
                let mine: Vec<usize> = (1..pm.cells.len()).filter(|&c| pm.comp_group[c] == pm.pad_group[i]).collect();
                eprintln!("  pad {}.{} at {:.2} {:.2}: group {}, {} joins, pieces {:?} ({} nodes)", pad.ref_, pad.num, pad.x, pad.y, pm.pad_group[i], base.joins[i].len(), &mine[..mine.len().min(6)], mine.iter().map(|&c| pm.cells[c].len()).sum::<usize>());
            }
            for (j, job) in self.jobs.iter().enumerate() {
                if job.net != net {
                    continue;
                }
                eprintln!("  job {}: stale {}, open {:?}", j, self.pour_stale(j), job.open);
                for &(cell, old) in &job.pour_ends {
                    let k = cell as usize;
                    let (x, y) = ((k % per) % w, (k % per) / w);
                    let c = pm.comp[k] as usize;
                    eprintln!("    end at {:.2} {:.2} layer {}: piece {} group {} (group {} when laid)", self.f.wx(x), self.f.wy(y), k / per, c, pm.comp_group[c], old);
                }
            }
            for &l in &base.layers {
                let mut img = format!("P5\n{} {}\n255\n", w, h).into_bytes();
                for i in l * per..(l + 1) * per {
                    let c = pm.comp[i] as usize;
                    img.push(if c == 0 { 0 } else { 90 + (size.get(&pm.comp_group[c]).copied().unwrap_or(0).min(11) * 15) as u8 });
                }
                let name: String = self.sc.nets[net as usize].name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                let _ = std::fs::write(format!("{}/{}.{}.pgm", dir, name, self.sc.layers[l]), img);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::route::{route, Cfg};
    use crate::scene::Scene;

    /// A 20 x 10 mm board of two layers, GND poured over the whole front and joining its pads solid, three
    /// GND pads, and (with `wall`) a signal whose two pads stand at the top and the bottom edge, so that
    /// its track cuts the front's fill in two.
    fn board(wall: bool) -> crate::route::Router {
        let pad = |r: &str, net: &str, x: f64, y: f64| format!(r#"{{"ref": "{r}", "num": "1", "net": "{net}", "at": [{x}, {y}], "tht": false, "shape": {{"poly": [[{}, {}], [{}, {}], [{}, {}], [{}, {}]], "r": 0}}}}"#, x - 0.4, y - 0.4, x + 0.4, y - 0.4, x + 0.4, y + 0.4, x - 0.4, y + 0.4);
        let mut pads = vec![pad("C1", "GND", 3.0, 5.0), pad("C2", "GND", 7.0, 3.0), pad("C3", "GND", 17.0, 5.0)];
        if wall {
            pads.push(pad("R1", "SIG", 10.0, 0.9));
            pads.push(pad("R2", "SIG", 10.0, 9.1));
        }
        let text = format!(
            r#"{{"version": 2, "units": "mm", "stack": {{"copper": ["F.Cu", "B.Cu"]}},
                "outline": [{{"line": [0, 0, 20, 0]}}, {{"line": [20, 0, 20, 10]}}, {{"line": [20, 10, 0, 10]}}, {{"line": [0, 10, 0, 0]}}],
                "rules": {{"track": 0.2, "clearance": 0.2, "via": 0.6, "drill": 0.3, "edge": 0.3, "hole_clearance": 0.25}},
                "nets": [{{"name": "GND", "role": "ground"}}, {{"name": "SIG"}}],
                "pads": [{}],
                "copper": {{"zones": [{{"net": "GND", "layer": "F.Cu", "outline": [[0, 0], [20, 0], [20, 10], [0, 10]], "pads": "solid", "min_width": 0.25}}]}}}}"#,
            pads.join(", ")
        );
        route(Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap(), &Cfg { two_ways: false, ..Cfg::default() }).unwrap().router
    }

    #[test]
    fn pads_a_pour_joins_get_no_track() {
        let r = board(false);
        let gnd = r.jobs.iter().find(|j| r.sc.nets[j.net as usize].name == "GND").unwrap();
        assert!(gnd.open.is_empty() && gnd.tracks.is_empty() && gnd.vias.is_empty(), "the pour joins the three pads: nothing to lay");
        let pm = r.pour(gnd.net).unwrap();
        assert!(pm.pad_group.iter().all(|&g| g == pm.pad_group[0]));
    }

    #[test]
    fn a_pour_another_net_cuts_is_bridged() {
        let r = board(true);
        assert!(r.jobs.iter().all(|j| j.open.is_empty()), "both nets are joined");
        let gnd = r.jobs.iter().find(|j| r.sc.nets[j.net as usize].name == "GND").unwrap();
        // the signal's track cuts the fill between C2 and C3: the pour no longer joins them, and GND has
        // copper of its own across the cut
        let pm = r.pour(gnd.net).unwrap();
        assert_eq!(pm.pad_group[0], pm.pad_group[1]);
        assert_ne!(pm.pad_group[1], pm.pad_group[2]);
        assert!(!gnd.tracks.is_empty() || !gnd.vias.is_empty());
        assert!((0..r.jobs.len()).all(|j| !r.pour_stale(j)), "no net leans on fill that is not there");
    }

    /// The pour on the back, the pads on the front: no pad touches the fill, each goes down to it by a via.
    /// A track then laid across the back cuts the fill between two of those vias, and the net is found
    /// to stand on fill that is no longer one piece.
    #[test]
    fn a_pour_no_pad_touches_is_watched_too() {
        use crate::field::{DynObj, Kind};
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/pour_bottom.json")).unwrap();
        let mut r = route(Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap(), &Cfg::default()).unwrap().router;
        let j = (0..r.jobs.len()).find(|&j| r.sc.nets[r.jobs[j].net as usize].name == "GND").unwrap();
        assert!(r.jobs[j].open.is_empty() && r.jobs[j].vias.len() >= 2 && !r.jobs[j].pour_ends.is_empty());
        assert!(!r.pour_stale(j));
        let (a, b) = (r.jobs[j].vias[0], r.jobs[j].vias[1]);
        let (ax, ay, bx, by) = (r.f.wx(a.0 as usize), r.f.wy(a.1 as usize), r.f.wx(b.0 as usize), r.f.wy(b.1 as usize));
        // a ring of another net's track round the first via, on the pour's layer
        let (mx, my) = ((bx - ax) / 2.0, (by - ay) / 2.0);
        let d = (mx.abs().max(my.abs())).min(1.5).max(0.9);
        let sig = (1..r.sc.nets.len()).find(|&n| r.sc.nets[n].name != "GND").unwrap() as u16;
        let ring = [(ax - d, ay - d), (ax + d, ay - d), (ax + d, ay + d), (ax - d, ay + d), (ax - d, ay - d)];
        for s in ring.windows(2) {
            r.f.add_obj(DynObj { kind: Kind::Track, net: sig, job: u32::MAX, locked: true, layer: (r.f.nl - 1) as u8, ax: s[0].0, ay: s[0].1, bx: s[1].0, by: s[1].1, r: 0.1, alive: true });
        }
        assert!(r.pour_stale(j), "the fill round one via is cut off from the rest");
    }
}
