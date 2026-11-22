//! The search: weighted A* over (layer, node) on the clearance field.
//!
//! Moves are the eight compass steps and a via in place. A node keeps its best score and the heading it
//! was reached by, and a bend is charged against that heading. Scores live in 16 x 16 tiles handed out
//! on first touch, so a search pays for the ground it covers and no more. The queue is a bucket queue.
//!
//! A search runs in two phases over the same scores. First it may not touch other nets' routed copper;
//! every step that would have is set aside with its price. If that phase runs dry or runs long, the
//! steps set aside join the queue and the search carries on, now allowed through such copper at a
//! price (the router then has two nets in one place, and settles that by routing them again at a
//! rising price). So the ground covered in the first phase is never covered twice.

mod cost;
mod queue;

pub use cost::{Costs, Price, Term, TermId, SEVERAL, SEVERAL_NETS};

use crate::field::{ddist, sdist, Dist, Field, NO_DIR};
use queue::{Entry, Queue};

pub const DX: [i32; 8] = [1, 1, 0, -1, -1, -1, 0, 1];
pub const DY: [i32; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
const TSHIFT: usize = 4;
const TCELLS: usize = (1 << TSHIFT) * (1 << TSHIFT);
const INF: u32 = u32::MAX;
const PAR_SOURCE: u8 = 255;
const PAR_VIA: u8 = 16;

/// A step set aside in the first phase: it would have crossed another net's copper.
#[derive(Clone, Copy)]
struct Aside {
    g: u32,
    x: u16,
    y: u16,
    l: u8,
    par: u8,
    since: u8,
}
/// A net kept to layers: where its tracks may run.
pub struct Keep {
    /// the layers it may run on, as a mask
    pub layers: u32,
    /// its pads: the layers each is on, and its copper
    pub pads: Vec<(u32, crate::geom::Shape)>,
    /// how far from the copper of a pad of its own it may run on that pad's layer
    pub reach: f64,
}

impl Keep {
    /// Whether a node on layer l is closed to the net: the layer is not one of its own, and no pad of the
    /// net on that layer is near enough for this to be the way from the pad to its via.
    #[inline]
    pub fn off(&self, l: usize, x: f64, y: f64) -> bool {
        self.layers & (1 << l) == 0 && !self.pads.iter().any(|(m, pad)| m & (1 << l) != 0 && pad.dist(x, y) <= self.reach)
    }
}

pub struct Query<'a> {
    pub net: u16,
    /// the net is kept to layers (`Keep`)
    pub keep: Option<&'a Keep>,
    /// thresholds in ticks: the narrowest track, the net's own width, a via
    pub t_min: Dist,
    /// below `t_low` a node is surely too near other copper; from there up to `t_min` the grid cannot
    /// tell (`Router::t_low`), and the step is measured by `tight`, at half-width `half`
    pub t_low: Dist,
    pub half: f64,
    pub tight: &'a crate::route::exact::Tight<'a>,
    /// nets whose routed copper this search may not go through (`Router::mend`)
    pub shun: &'a [u16],
    pub t_nom: Dist,
    pub t_via: Dist,
    /// when not 0: the threshold of the smaller via the board allows, used where the full one does not
    /// fit (`small` of `run` says where fixed copper lets it stand), and what such a via costs more
    pub t_via_s: Dist,
    pub small_cost: u32,
    /// when not 0: the via must also be this far from fixed copper (a net with more clearance than the
    /// board's, for which the board's via sites are not enough)
    pub t_via_fixed: Dist,
    /// (layer, y, x, starting cost)
    pub sources: &'a [(u8, u16, u16, u32)],
    /// where the heuristic aims: a node box and the layers the target sits on
    pub tbox: (i32, i32, i32, i32),
    pub tlayers: u32,
    /// Some(layer): the goal is to reach this plane layer by a via.
    pub plane_goal: Option<u8>,
    /// start in the second phase: other nets' routed copper may be crossed from the first step
    pub soft: bool,
    /// go on to the second phase when the first runs dry or passes `hard_pops`
    pub then_soft: bool,
    pub hard_pops: u64,
    /// what standing in another net's copper costs, per node
    pub rip_cell: u32,
    /// charge that price flat, not more where nets have clashed before
    pub flat: bool,
    pub via_cost: u32,
    /// how many default track widths this net is wide: what it pays of a price by the width
    /// (`cost::Price::per_width`)
    pub toll_mul: u32,
    /// the heuristic's weight, x256: 256 is exact A*, more is greedier; and the weight from the second
    /// phase on
    pub weight256: u64,
    pub weight256_soft: u64,
    /// nodes (x, y) where this net already goes through the board: a layer change there is free
    pub own_vias: &'a [(u16, u16)],
    /// nodes (layer, x, y) on pads of this net that already reach its plane: for a plane goal, arriving
    /// there is as good as a via
    pub own_pads: &'a [(u8, u16, u16)],
    /// two vias of one net keep this many nodes apart, squared
    pub via_gap2: i64,
    /// and two vias of one path keep this many steps apart
    pub via_steps: u8,
    pub max_pops: u64,
}

pub enum Outcome {
    /// nodes (layer, y, x) from source to goal, the cost, and whether the path may cross other nets'
    /// copper (it was found in the second phase); a change of layer shows as two consecutive nodes at
    /// one place
    Found(Vec<(u8, u16, u16)>, u32, bool),
    /// every reachable node was tried: there is no way
    Exhausted,
    /// stopped at the query's limit
    Capped,
}

pub struct Search {
    tw: usize,
    th: usize,
    tile_ref: Vec<u64>,
    stamp: u32,
    used: usize,
    g: Vec<u32>,
    par: Vec<u8>,
    /// steps since the path to this node last changed layer (255: never, or long ago)
    since: Vec<u8>,
    queue: Queue,
    aside: Vec<Aside>,
    tmark: Vec<u64>,
    tlist: Vec<usize>,
    pub pops: u64,
    pub searches: u64,
}

impl Search {
    pub fn new(f: &Field, bshift: u32) -> Search {
        let tw = (f.w >> TSHIFT) + 1;
        let th = (f.h >> TSHIFT) + 1;
        Search {
            tw,
            th,
            tile_ref: vec![0; tw * th * f.nl],
            stamp: 0,
            used: 0,
            g: Vec::new(),
            par: Vec::new(),
            since: Vec::new(),
            queue: Queue::new(bshift),
            aside: Vec::new(),
            tmark: vec![0; (f.w * f.h * f.nl) / 64 + 1],
            tlist: Vec::new(),
            pops: 0,
            searches: 0,
        }
    }

    pub fn mark_target(&mut self, f: &Field, l: usize, y: usize, x: usize) {
        let i = f.idx(l, y, x);
        if self.tmark[i >> 6] & (1 << (i & 63)) == 0 {
            self.tmark[i >> 6] |= 1 << (i & 63);
            self.tlist.push(i);
        }
    }

    /// The same by the node's index in the field.
    pub fn mark_target_at(&mut self, i: usize) {
        if self.tmark[i >> 6] & (1 << (i & 63)) == 0 {
            self.tmark[i >> 6] |= 1 << (i & 63);
            self.tlist.push(i);
        }
    }

    pub fn is_target(&self, i: usize) -> bool {
        self.tmark[i >> 6] & (1 << (i & 63)) != 0
    }

    pub fn clear_targets(&mut self) {
        for &i in &self.tlist {
            self.tmark[i >> 6] = 0;
        }
        self.tlist.clear();
    }

    #[inline(always)]
    fn node(&mut self, l: usize, y: usize, x: usize) -> usize {
        let t = (l * self.th + (y >> TSHIFT)) * self.tw + (x >> TSHIFT);
        let r = self.tile_ref[t];
        let slot = if (r >> 32) as u32 == self.stamp { (r & 0xffff_ffff) as usize } else { self.new_tile(t) };
        slot * TCELLS + (((y & 15) << 4) | (x & 15))
    }

    #[cold]
    fn new_tile(&mut self, t: usize) -> usize {
        let s = self.used;
        self.used += 1;
        let need = (s + 1) * TCELLS;
        if self.g.len() < need {
            self.g.resize(need, INF);
            self.par.resize(need, 0);
            self.since.resize(need, 255);
        } else {
            self.g[s * TCELLS..need].fill(INF);
        }
        self.tile_ref[t] = ((self.stamp as u64) << 32) | s as u64;
        s
    }

    pub fn run(&mut self, f: &Field, c: &Costs, wear: &[u8], via_ok: &[u64], small: &[u64], q: &Query) -> Outcome {
        self.searches += 1;
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.tile_ref.fill(0);
            self.stamp = 1;
        }
        self.used = 0;
        self.queue.clear();
        self.aside.clear();

        let nl = f.nl;
        let net = q.net;
        let (tx0, ty0, tx1, ty1) = q.tbox;
        let plane = q.plane_goal.map(|p| p as usize);
        let weight = std::cell::Cell::new(if q.soft { q.weight256_soft } else { q.weight256 });
        let heur = |l: usize, x: i32, y: i32| -> u64 {
            if plane.is_some() {
                return 0;
            }
            let dx = if x < tx0 { tx0 - x } else if x > tx1 { x - tx1 } else { 0 } as u64;
            let dy = if y < ty0 { ty0 - y } else if y > ty1 { y - ty1 } else { 0 } as u64;
            let (mn, mx) = if dx < dy { (dx, dy) } else { (dy, dx) };
            let hh = ((mn * c.h_diag + (mx - mn) * c.h_orth) * weight.get()) >> 8;
            if q.tlayers & (1 << l) == 0 {
                hh + q.via_cost as u64
            } else {
                hh
            }
        };

        // (the search may stand only on nodes that have all eight neighbours: the grid's outermost nodes
        // are closed to every step, and a source there is not taken)
        let inside = |l: u8, y: u16, x: u16| (l as usize) < nl && x >= 1 && y >= 1 && (x as usize) < f.w - 1 && (y as usize) < f.h - 1;
        if !q.sources.iter().any(|&(l, y, x, _)| inside(l, y, x)) {
            return Outcome::Exhausted;
        }
        for &(l, y, x, c0) in q.sources {
            if !inside(l, y, x) {
                continue;
            }
            let b = self.node(l as usize, y as usize, x as usize);
            if c0 < self.g[b] {
                self.g[b] = c0;
                self.par[b] = PAR_SOURCE;
                self.since[b] = 255;
                let ff = c0 as u64 + heur(l as usize, x as i32, y as i32);
                self.queue.push(ff, Entry { g: c0, x, y, l });
            }
        }

        // what a step in heading `nd` costs in bends after arriving in heading `pd` (row 8: no heading);
        // u32::MAX: a turn back, never taken
        let mut turn_tab = [[0u32; 8]; 9];
        for pd in 0..8usize {
            for nd in 0..8usize {
                let k = (nd as i32 - pd as i32).rem_euclid(8);
                let k = if k > 4 { 8 - k } else { k };
                turn_tab[pd][nd] = if k == 4 { u32::MAX } else { c.turn[k as usize] };
            }
        }
        let wide = q.t_nom > q.t_min;
        // the cost terms' prices for this net, by price class
        let tolls = c.price_table(q.toll_mul);
        let mut soft = q.soft;
        let mut pops = 0u64;
        let mut goal: Option<Entry> = None;
        let mut capped = false;
        loop {
            let popped = self.queue.pop();
            // the first phase is over when it runs dry or has run long: the steps set aside come in
            if !soft && q.then_soft && (popped.is_none() || pops >= q.hard_pops) {
                soft = true;
                weight.set(q.weight256_soft);
                let mut rest: Vec<Entry> = Vec::new();
                if let Some(e) = popped {
                    rest.push(e);
                }
                self.queue.drain(&mut rest);
                let aside = std::mem::take(&mut self.aside);
                let mut lo = u64::MAX;
                for e in &rest {
                    lo = lo.min(e.g as u64 + heur(e.l as usize, e.x as i32, e.y as i32));
                }
                for a in &aside {
                    lo = lo.min(a.g as u64 + heur(a.l as usize, a.x as i32, a.y as i32));
                }
                if lo == u64::MAX {
                    break;
                }
                for e in rest {
                    let ff = e.g as u64 + heur(e.l as usize, e.x as i32, e.y as i32);
                    self.queue.push(ff, e);
                }
                for a in &aside {
                    let nb = self.node(a.l as usize, a.y as usize, a.x as usize);
                    if a.g < self.g[nb] {
                        self.g[nb] = a.g;
                        self.par[nb] = a.par;
                        self.since[nb] = a.since;
                        let ff = a.g as u64 + heur(a.l as usize, a.x as i32, a.y as i32);
                        self.queue.push(ff, Entry { g: a.g, x: a.x, y: a.y, l: a.l });
                    }
                }
                self.aside = aside;
                self.aside.clear();
                continue;
            }
            let Some(e) = popped else { break };
            let (l, y, x) = (e.l as usize, e.y as usize, e.x as usize);
            let b = self.node(l, y, x);
            if self.g[b] != e.g {
                continue;
            }
            pops += 1;
            if pops > q.max_pops {
                capped = true;
                break;
            }
            match plane {
                Some(pl) => {
                    if l == pl {
                        goal = Some(e);
                        break;
                    }
                }
                None => {
                    let i = (l * f.h + y) * f.w + x;
                    if self.tmark[i >> 6] & (1 << (i & 63)) != 0 {
                        goal = Some(e);
                        break;
                    }
                }
            }
            let pd = self.par[b];
            let since = self.since[b];
            let step = c.step[l];
            if step[0] != 0 {
                // a track that starts at a fence's end carries on the way the fence was going
                let mut head = if pd < 8 { pd as usize } else { 8 };
                if pd == PAR_SOURCE {
                    let here = &f.cells[(l * f.h + y) * f.w + x];
                    if here.fdir != NO_DIR && here.sn1 == net {
                        head = here.fdir as usize;
                    }
                }
                let turns = &turn_tab[head];
                let tile = b & !(TCELLS - 1);
                let layer_base = l * f.h * f.w;
                // where the step starts: in a pad of the net's own, and whether the full width fits there
                let hi = layer_base + y * f.w + x;
                let here = unsafe { f.cells.get_unchecked(hi) };
                let here_own = here.sn1 == net && here.sd1 == 0;
                // (a step from or to a node the grid is not sure of is measured: `Query::t_low`)
                let (here_ts, here_td) = (sdist(here, net) < q.t_min, ddist(here, net) < q.t_min);
                let at = (f.wx(x), f.wy(y));
                let here_narrow = wide && !here_own && (sdist(here, net) < q.t_nom || ddist(here, net) < q.t_nom);
                let stub = q.keep.map_or(false, |k| k.layers & (1 << l) == 0);
                for nd in 0..8usize {
                    let mut turn = turns[nd];
                    if turn == u32::MAX {
                        continue;
                    }
                    // (the grid's outermost nodes are closed to every net, so a neighbour of a node a
                    // search stands on is always on the grid)
                    let nx = (x as i32 + DX[nd]) as usize;
                    let ny = (y as i32 + DY[nd]) as usize;
                    let ci = layer_base + ny * f.w + nx;
                    debug_assert!(ci < f.cells.len());
                    let cell = unsafe { f.cells.get_unchecked(ci) };
                    let sd = sdist(cell, net);
                    if sd < q.t_low {
                        continue;
                    }
                    let to = (f.wx(nx), f.wy(ny));
                    if q.keep.map_or(false, |k| k.off(l, to.0, to.1)) {
                        continue;
                    }
                    if (here_ts || sd < q.t_min) && !q.tight.fixed(at, to, q.half, net, l) {
                        continue;
                    }
                    let dd = ddist(cell, net);
                    // through another net's routed copper?
                    let through = dd < q.t_low || ((here_td || dd < q.t_min) && !q.tight.routed(f, at, to, q.half, net, l));
                    // (off the layers a net is kept to, it runs only from a pad to that pad's via: the
                    // shorter that is the better, so a step there costs three)
                    let mut cost = if stub { step[nd & 1] * 3 } else { step[nd & 1] };
                    let own = cell.sn1 == net && cell.sd1 == 0;
                    if own {
                        // inside a pad of its own: the copper is there already
                        cost /= 4;
                        turn /= 4;
                    } else if wide && (sd < q.t_nom || dd < q.t_nom) {
                        // the full width does not fit here: a neck, dear to begin and dear to go on with
                        cost *= c.neck_mul;
                        if !here_narrow && !through {
                            cost += c.neck_in;
                        }
                    }
                    if own != here_own {
                        // over the edge of a pad that has a front and a back: by one of them, or at a price
                        if let Some(&m) = c.exits.get(&((if here_own { hi } else { ci }) as u32)) {
                            if m & (1 << nd) == 0 {
                                cost += c.exit_price;
                            }
                        }
                    }
                    if cell.toll != 0 {
                        // the cost terms: what this node costs every net but its owner
                        if cell.toll == SEVERAL {
                            cost = cost.saturating_add(c.several_price(ci, net, q.toll_mul));
                        } else if cell.toll_net != net {
                            cost += tolls[cell.toll as usize];
                        }
                    }
                    if cell.fdir != NO_DIR && cell.sn1 == net {
                        // arriving at a fence's end of its own net, the track turns into the fence:
                        // never back on itself, and a sharp turn there is as dear as anywhere
                        let into = turn_tab[nd][(cell.fdir as usize + 4) & 7];
                        if into == u32::MAX {
                            continue;
                        }
                        turn += into;
                    }
                    let mut ng = e.g.saturating_add(cost).saturating_add(turn);
                    if through {
                        if cell.dn1 != net && q.shun.contains(&cell.dn1) {
                            continue;
                        }
                        // through another net's copper: the price, more where nets have clashed before
                        ng = ng.saturating_add(q.rip_cell.saturating_mul(if q.flat { 1 } else { 1 + wear[ci] as u32 }));
                        if !soft {
                            if q.then_soft {
                                self.aside.push(Aside { g: ng, x: nx as u16, y: ny as u16, l: l as u8, par: nd as u8, since: since.saturating_add(1) });
                            }
                            continue;
                        }
                    }
                    // most neighbours lie in the tile the node itself is in
                    let nb = if (nx >> TSHIFT) == (x >> TSHIFT) && (ny >> TSHIFT) == (y >> TSHIFT) { tile + (((ny & 15) << 4) | (nx & 15)) } else { self.node(l, ny, nx) };
                    debug_assert!(nb < self.g.len());
                    let gn = unsafe { self.g.get_unchecked_mut(nb) };
                    if ng < *gn {
                        *gn = ng;
                        unsafe {
                            *self.par.get_unchecked_mut(nb) = nd as u8;
                            *self.since.get_unchecked_mut(nb) = since.saturating_add(1);
                        }
                        let ff = ng as u64 + heur(l, nx as i32, ny as i32);
                        self.queue.push(ff, Entry { g: ng, x: nx as u16, y: ny as u16, l: l as u8 });
                    }
                }
            }
            // a via: not straight after another, and not within a via's width of the last one (a place
            // where the net already goes through the board is free of both rules)
            if pd >= PAR_VIA && pd != PAR_SOURCE {
                continue;
            }
            let vi = y * f.w + x;
            let own = q.own_vias.iter().any(|&(vx, vy)| vx as usize == x && vy as usize == y)
                || (plane.is_some() && q.own_pads.iter().any(|&(pl, vx, vy)| pl as usize == l && vx as usize == x && vy as usize == y));
            if !own && since < q.via_steps {
                continue;
            }
            let mut pen = 0u32;
            let mut crosses = false;
            if !own {
                // where the full via may stand as far as fixed copper goes, and where only the small one
                let big = via_ok[vi >> 6] & (1 << (vi & 63)) != 0;
                let sm = q.t_via_s != 0 && q.t_via_fixed == 0 && !small.is_empty() && small[vi >> 6] & (1 << (vi & 63)) != 0;
                if !big && !sm {
                    continue;
                }
                // a site a cost term reserved for other nets is closed; one reserved for this net and
                // another's (the shared mouth of two fan lanes) is open at a price
                let vl = c.via_net[vi];
                if vl != 0 && vl != net {
                    match c.via_price(vi, net) {
                        Some(p) => pen = pen.saturating_add(p),
                        None => continue,
                    }
                }
                let mut ok = true;
                for &(vx, vy) in q.own_vias {
                    let ddx = vx as i64 - x as i64;
                    let ddy = vy as i64 - y as i64;
                    if ddx * ddx + ddy * ddy < q.via_gap2 {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                if q.t_via_fixed != 0 && (0..nl).any(|ll| sdist(&f.cells[(ll * f.h + y) * f.w + x], net) < q.t_via_fixed) {
                    continue;
                }
                // the largest via that stands clear of routed copper; when none does, the largest the
                // fixed copper allows, through it (`Router::via_is_small` lays the same)
                let free = |t: Dist| (0..nl).all(|ll| ddist(&f.cells[(ll * f.h + y) * f.w + x], net) >= t);
                let t_via = if big && free(q.t_via) {
                    q.t_via
                } else if sm && free(q.t_via_s) {
                    pen = pen.saturating_add(q.small_cost);
                    q.t_via_s
                } else if big {
                    q.t_via
                } else {
                    pen = pen.saturating_add(q.small_cost);
                    q.t_via_s
                };
                for ll in 0..nl {
                    let ci = (ll * f.h + y) * f.w + x;
                    if ddist(&f.cells[ci], net) < t_via {
                        if f.cells[ci].dn1 != net && q.shun.contains(&f.cells[ci].dn1) {
                            ok = false;
                        }
                        crosses = true;
                        pen = pen.saturating_add(q.rip_cell.saturating_mul(3).saturating_mul(if q.flat { 1 } else { 1 + wear[ci] as u32 }));
                    }
                }
                // nor may its hole stand too near another net's via's (one of its own net is `via_gap2`'s):
                // that is a clash like copper too near, settled the same way
                if f.holes[vi] != 0 {
                    crosses = true;
                    pen = pen.saturating_add(q.rip_cell.saturating_mul(3));
                }
                if !ok || (crosses && !soft && !q.then_soft) {
                    continue;
                }
            }
            let vc = if own { 0 } else { q.via_cost.saturating_add(pen) };
            let ng = e.g.saturating_add(vc);
            let par = PAR_VIA + l as u8;
            let put_aside = crosses && !soft;
            match plane {
                Some(pl) => {
                    if put_aside {
                        self.aside.push(Aside { g: ng, x: x as u16, y: y as u16, l: pl as u8, par, since: 0 });
                    } else {
                        let nb = self.node(pl, y, x);
                        if ng < self.g[nb] {
                            self.g[nb] = ng;
                            self.par[nb] = par;
                            self.since[nb] = 0;
                            self.queue.push(ng as u64, Entry { g: ng, x: x as u16, y: y as u16, l: pl as u8 });
                        }
                    }
                }
                None => {
                    for l2 in 0..nl {
                        if l2 == l || c.step[l2][0] == 0 || q.keep.map_or(false, |k| k.off(l2, f.wx(x), f.wy(y))) {
                            continue;
                        }
                        if put_aside {
                            self.aside.push(Aside { g: ng, x: x as u16, y: y as u16, l: l2 as u8, par, since: 0 });
                            continue;
                        }
                        let nb = self.node(l2, y, x);
                        if ng < self.g[nb] {
                            self.g[nb] = ng;
                            self.par[nb] = par;
                            self.since[nb] = 0;
                            let ff = ng as u64 + heur(l2, x as i32, y as i32);
                            self.queue.push(ff, Entry { g: ng, x: x as u16, y: y as u16, l: l2 as u8 });
                        }
                    }
                }
            }
        }
        self.pops += pops;

        let Some(e) = goal else {
            return if capped { Outcome::Capped } else { Outcome::Exhausted };
        };
        let mut path = Vec::new();
        let (mut l, mut y, mut x) = (e.l as usize, e.y as usize, e.x as usize);
        loop {
            path.push((l as u8, y as u16, x as u16));
            let b = self.node(l, y, x);
            let p = self.par[b];
            if p == PAR_SOURCE {
                break;
            }
            if p >= PAR_VIA {
                l = (p - PAR_VIA) as usize;
            } else {
                x = (x as i32 - DX[p as usize]) as usize;
                y = (y as i32 - DY[p as usize]) as usize;
            }
            if path.len() > 4_000_000 {
                break;
            }
        }
        path.reverse();
        Outcome::Found(path, e.g, soft)
    }
}
