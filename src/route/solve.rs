//! One connection asked of the search.

use super::Router;
use crate::field::sdist;
use crate::search::{Outcome, Query, DX, DY};
use std::time::Instant;

/// One connection to find: what the search needs, owned, so it can be asked more than once.
pub(crate) struct Ask {
    pub(crate) net: u16,
    pub(crate) sources: Vec<(u8, u16, u16, u32)>,
    pub(crate) tbox: (i32, i32, i32, i32),
    pub(crate) tlayers: u32,
    pub(crate) plane_goal: Option<u8>,
    pub(crate) own_vias: Vec<(u16, u16)>,
    pub(crate) own_pads: Vec<(u8, u16, u16)>,
    pub(crate) via_mm: f64,
    pub(crate) rip_mm: f64,
    /// node expansions before the search may go through other nets' copper, and in all
    pub(crate) hard: u64,
    pub(crate) cap: u64,
    /// the target cannot get out without lifting: let the search lift from its first step
    pub(crate) shut: bool,
    pub(crate) what: String,
}

impl Router {
    /// Find one connection: without touching other nets' copper if that can be found soon, else through
    /// it at a price (the clash is settled in the rounds that follow). A net that finds no way at the
    /// price of the day is laid at the first day's price: a net left out moves nobody, a net laid through
    /// another makes the two of them settle it.
    ///
    /// When the search that insists finds no way either, the board is asked whether it leaves one at all
    /// (`walled_in`): `Router::no_way` then says that the pad is shut in by what stays, and not by the
    /// other nets or by the search's own limit.
    pub(crate) fn solve(&mut self, ask: &Ask) -> Option<(Vec<(u8, u16, u16)>, bool)> {
        self.no_way = false;
        let mut last = Outcome::Exhausted;
        for insist in [false, true] {
            if insist && self.strict {
                // (no way that touches nothing: the other nets' doing, as far as this search knows)
                return None;
            }
            last = self.search_for(ask, insist);
            if let Outcome::Found(path, _, may_clash) = last {
                return Some((path, may_clash));
            }
        }
        // free to go through other nets' copper, only what stays can stop a search: one that tried every
        // node has seen that, one that stopped at its limit has not
        self.no_way = matches!(last, Outcome::Exhausted) || self.walled_in(ask);
        None
    }

    fn search_for(&mut self, ask: &Ask, insist: bool) -> Outcome {
        let net = ask.net;
        let kind = self.kind_of[net as usize] as usize;
        let price = if insist { 1.0 } else { self.pressure };
        let tight = super::exact::Tight { statics: &self.statics, clr: &self.clr_tab, more: &self.more_tab };
        let q = Query {
            net,
            keep: self.keeps.get(&net),
            t_min: self.t_min(net),
            t_low: self.t_low(net),
            half: self.w_min(net) / 2.0,
            tight: &tight,
            shun: &self.shun,
            t_nom: self.t_nom(net),
            t_via: self.t_via_of(net),
            t_via_fixed: if self.t_via_of(net) > self.kinds[kind].t { self.t_via_of(net) } else { 0 },
            t_via_s: if self.kinds[kind].small { self.t_via_s } else { 0 },
            small_cost: (ask.via_mm * 500.0) as u32,
            sources: &ask.sources,
            tbox: ask.tbox,
            tlayers: ask.tlayers,
            plane_goal: ask.plane_goal,
            soft: (ask.shut || insist) && !self.strict,
            then_soft: !self.strict,
            hard_pops: ask.hard,
            rip_cell: (ask.rip_mm * price * 1000.0 * self.cfg.pitch / 0.5).min(4.0e6) as u32,
            flat: insist,
            via_cost: (ask.via_mm * 1000.0) as u32,
            share_cost: (self.cfg.share_mm * 1000.0) as u32,
            toll_mul: ((self.sc.nets[net as usize].width / self.sc.rules.track).round() as u32).clamp(1, 8),
            weight256: (self.cfg.weight * 256.0).round() as u64,
            weight256_soft: (self.cfg.weight_soft * 256.0).round() as u64,
            own_vias: &ask.own_vias,
            own_pads: &ask.own_pads,
            via_gap2: self.kinds[kind].gap2,
            via_steps: self.via_steps,
            max_pops: ask.cap,
        };
        let p0 = self.search.pops;
        let ts = Instant::now();
        // (as `sites_of`, field by field: the search is borrowed too)
        let (sites, small) = match self.plane_sites.get(&net) {
            Some(s) if ask.plane_goal.is_some() => (&s[..], self.plane_sites_s.get(&net).map_or(&[][..], |v| &v[..])),
            _ => (&self.kinds[kind].ok[..], &self.via_ok_s[..]),
        };
        let out = self.search.run(&self.f, &self.costs, &self.wear, sites, small, &q);
        self.stats.search_s += ts.elapsed().as_secs_f64();
        if self.cfg.verbose {
            eprintln!(
                "  {}{}: {} sources, {} expanded, {}",
                ask.what,
                if insist { " (insisting)" } else { "" },
                ask.sources.len(),
                self.search.pops - p0,
                match &out {
                    Outcome::Found(p, c, lifting) => format!("{} nodes, cost {}{}", p.len(), c, if *lifting { " (may clash)" } else { "" }),
                    Outcome::Exhausted => "no way".into(),
                    Outcome::Capped => "stopped at the limit".into(),
                }
            );
        }
        out
    }

    /// Where a via of this connection may stand as far as what stays goes: the full one, the small one.
    /// (A drop to a plane a zone draws lands only where that zone is.)
    fn sites_of(&self, ask: &Ask) -> (&[u64], &[u64]) {
        if ask.plane_goal.is_some() {
            if let Some(s) = self.plane_sites.get(&ask.net) {
                return (s, self.plane_sites_s.get(&ask.net).map_or(&[][..], |v| &v[..]));
            }
        }
        (&self.kind(ask.net).ok, &self.via_ok_s[..])
    }

    /// Does the board itself leave this connection no way, whatever the other nets do? A flood from its
    /// sources over every node a track of the net may stand on as far as what stays goes (pads, edges,
    /// keep-outs, fences, copper the board came with), through every via site. The flood is freer than
    /// any search (no turns to mind, no site kept for another net, no vias of its own to keep off), so
    /// where it reaches no target, no search will; and it is quick, where a search that finds no way
    /// has the whole board to try at a search's pace.
    pub(crate) fn walled_in(&self, ask: &Ask) -> bool {
        let f = &self.f;
        let (w, h, nl, per) = (f.w, f.h, f.nl, f.w * f.h);
        let net = ask.net;
        // (every node a search may stand on: down to where the grid is sure there is no room)
        let t = self.t_low(net);
        let (sites, small) = self.sites_of(ask);
        let small_too = self.t_via_s != 0 && self.kind(net).small && self.t_via_of(net) == self.kind(net).t;
        let site = |k: usize| sites[k >> 6] & (1 << (k & 63)) != 0 || (small_too && small.get(k >> 6).map_or(false, |m| m & (1 << (k & 63)) != 0));
        let plane = ask.plane_goal.map(|p| p as usize);
        let keep = self.keeps.get(&net);
        let off = |n: usize| keep.map_or(false, |k| k.off(n / per, f.wx((n % per) % w), f.wy((n % per) / w)));
        let mut seen = vec![0u64; per * nl / 64 + 1];
        let mut todo: Vec<usize> = Vec::new();
        for &(l, y, x, _) in &ask.sources {
            let i = f.idx(l as usize, y as usize, x as usize);
            if (l as usize) < nl && seen[i >> 6] & (1 << (i & 63)) == 0 {
                seen[i >> 6] |= 1 << (i & 63);
                todo.push(i);
            }
        }
        while let Some(i) = todo.pop() {
            let (l, k) = (i / per, i % per);
            let (x, y) = (k % w, k / w);
            // (the grid's outermost nodes are closed to every search)
            if x < 1 || y < 1 || x + 1 >= w || y + 1 >= h {
                continue;
            }
            if plane.is_none() && self.search.is_target(i) {
                return false;
            }
            if self.costs.step[l][0] != 0 {
                for nd in 0..8 {
                    let n = l * per + ((y as i32 + DY[nd]) as usize) * w + (x as i32 + DX[nd]) as usize;
                    if seen[n >> 6] & (1 << (n & 63)) == 0 && sdist(&f.cells[n], net) >= t && !off(n) {
                        seen[n >> 6] |= 1 << (n & 63);
                        todo.push(n);
                    }
                }
            }
            let own = ask.own_vias.iter().any(|&(vx, vy)| vx as usize == x && vy as usize == y) || ask.own_pads.iter().any(|&(pl, vx, vy)| pl as usize == l && vx as usize == x && vy as usize == y);
            if own || site(k) {
                if plane.is_some() {
                    return false;
                }
                for l2 in (0..nl).filter(|&l2| l2 != l && self.costs.step[l2][0] != 0) {
                    let n = l2 * per + k;
                    if seen[n >> 6] & (1 << (n & 63)) == 0 && !off(n) {
                        seen[n >> 6] |= 1 << (n & 63);
                        todo.push(n);
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::Ask;
    use crate::route::{Cfg, Router};
    use crate::scene::Scene;

    /// The ask for a track from pad `from` to pad `to` (the search's targets are marked: `to`'s nodes).
    fn ask(r: &mut Router, from: usize, to: usize) -> Ask {
        let mut sources = Vec::new();
        r.pad_sources(from, &mut sources);
        for a in r.acc[to].clone() {
            for l in r.pad_layers(to) {
                r.search.mark_target(&r.f, l, a.y as usize, a.x as usize);
            }
        }
        Ask { net: r.sc.pads[from].net, sources, tbox: (0, 0, 0, 0), tlayers: 1, plane_goal: None, own_vias: vec![], own_pads: vec![], via_mm: 1.0, rip_mm: 1.0, hard: 100, cap: 100, shut: false, what: String::new() }
    }

    #[test]
    fn the_board_says_whether_it_leaves_a_pad_a_way() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/walled.json")).unwrap();
        let mut r = Router::new(Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap(), Cfg::default(), Vec::new()).unwrap();
        r.find_via_sites();
        let pad = |r: &Router, name: &str| r.sc.pads.iter().position(|p| p.ref_ == name).unwrap();
        let (a1, a2, b1, b2) = (pad(&r, "A1"), pad(&r, "A2"), pad(&r, "B1"), pad(&r, "B2"));
        // B2 stands in a ring of keep-out: no way out of it, and none into it
        for (from, to, walled) in [(b2, b1, true), (b1, b2, true), (a2, a1, false), (a1, a2, false)] {
            let q = ask(&mut r, from, to);
            assert_eq!(r.walled_in(&q), walled, "{} to {}", r.sc.pads[from].ref_, r.sc.pads[to].ref_);
            // a search stopped at its limit (100 nodes: both are) has not seen that; the board is asked
            assert!(r.solve(&q).is_none());
            assert_eq!(r.no_way, walled);
            r.search.clear_targets();
        }
    }
}
