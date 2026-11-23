//! What a step costs, and the cost hook (docs/hooks.md, "Cost term"): named layers of per-node prices
//! that the search adds to a step. A term prices nodes for every net but the node's owner, and may
//! reserve via sites for nets. The prices of all terms are summed as they are written, into one byte
//! of the field's cell (a price class), so the search's inner loop reads no more than it did.

use crate::field::Field;
use std::collections::HashMap;

/// What a step costs. Units: 1000 per mm of track on a layer of cost 1.
pub struct Costs {
    /// per layer: [straight step, diagonal step]; 0 = no tracks on this layer
    pub step: Vec<[u32; 2]>,
    /// by bend: 0, 45, 90, 135 degrees
    pub turn: [u32; 4],
    /// narrow stretches of a wide net cost this many times a step, and this to begin
    pub neck_mul: u32,
    pub neck_in: u32,
    /// the nodes in the copper of a pad that has a front and a back (field index -> the headings, as
    /// bits, in which a track of the pad's net may cross the pad's edge there), and what crossing it
    /// any other way costs (`passes::exits`)
    pub exits: HashMap<u32, u8>,
    pub exit_price: u32,
    /// one straight step on the cheapest layer (the heuristic's unit)
    pub h_orth: u64,
    pub h_diag: u64,
    /// the cost terms, by `TermId`
    pub terms: Vec<Term>,
    /// the price classes: a cell's `toll` is an index here (0: nothing to pay)
    classes: Vec<Price>,
    /// cells that owe to more than one owner (their `toll` is SEVERAL): field index -> what each is owed
    several: HashMap<u32, Vec<(u16, Price)>>,
    /// per node (y * w + x): the net a via site is reserved for; 0: free; SEVERAL_NETS: see `via_several`
    pub via_net: Vec<u16>,
    /// via sites reserved for more than one net: node -> (net, the term that reserved it)
    via_several: HashMap<u32, Vec<(u16, TermId)>>,
    via_term: Vec<u8>,
}

/// A cost term, as its owner named it.
pub struct Term {
    pub name: &'static str,
    /// what a via costs a net at a site reserved for it and for another net too
    pub via_shared: u32,
    /// how many nodes it priced, how many via sites it reserved
    pub nodes: usize,
    pub via_sites: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TermId(pub u8);

/// A price for a step onto a node: `flat`, plus `per_width` once for every default track width the
/// paying net is wide (`Query::toll_mul`: a wide track shuts more of a lane).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Price {
    pub flat: u32,
    pub per_width: u32,
}

impl Price {
    pub fn flat(v: u32) -> Price {
        Price { flat: v, per_width: 0 }
    }
    pub fn per_width(v: u32) -> Price {
        Price { flat: 0, per_width: v }
    }
    fn plus(self, o: Price) -> Price {
        Price { flat: self.flat.saturating_add(o.flat), per_width: self.per_width.saturating_add(o.per_width) }
    }
    #[inline]
    pub fn at(self, mul: u32) -> u32 {
        self.flat.saturating_add(self.per_width.saturating_mul(mul))
    }
}

/// `Cell::toll` of a cell that owes to more than one owner.
pub const SEVERAL: u8 = 255;
/// `via_net` of a site reserved for more than one net.
pub const SEVERAL_NETS: u16 = 0xffff;

impl Costs {
    pub fn new(step: Vec<[u32; 2]>, turn: [u32; 4], neck_mul: u32, h_orth: u64, h_diag: u64, nodes: usize) -> Costs {
        Costs { step, turn, neck_mul, neck_in: 0, exits: HashMap::new(), exit_price: 0, h_orth, h_diag, terms: Vec::new(), classes: vec![Price::default()], several: HashMap::new(), via_net: vec![0; nodes], via_several: HashMap::new(), via_term: vec![0; nodes] }
    }

    /// A cost term by name: made the first time it is asked for.
    pub fn term(&mut self, name: &'static str) -> TermId {
        if let Some(i) = self.terms.iter().position(|t| t.name == name) {
            return TermId(i as u8);
        }
        assert!(self.terms.len() < 255, "more than 255 cost terms");
        self.terms.push(Term { name, via_shared: 0, nodes: 0, via_sites: 0 });
        TermId(self.terms.len() as u8 - 1)
    }

    /// Every net but `owner` (0: every net) pays `price` more for a step onto the cell at field index
    /// `ci`. Prices add up: a second charge on a cell is paid on top of the first.
    pub fn charge(&mut self, f: &mut Field, term: TermId, ci: usize, owner: u16, price: Price) {
        self.terms[term.0 as usize].nodes += 1;
        let cell = &mut f.cells[ci];
        if cell.toll == SEVERAL {
            let list = self.several.entry(ci as u32).or_default();
            match list.iter_mut().find(|e| e.0 == owner) {
                Some(e) => e.1 = e.1.plus(price),
                None => list.push((owner, price)),
            }
            return;
        }
        let (old_owner, old) = (cell.toll_net, self.classes[cell.toll as usize]);
        if cell.toll == 0 || old_owner == owner {
            let sum = old.plus(price);
            let class = match self.classes.iter().position(|&c| c == sum) {
                Some(c) => c,
                None if self.classes.len() < SEVERAL as usize => {
                    self.classes.push(sum);
                    self.classes.len() - 1
                }
                None => SEVERAL as usize,
            };
            cell.toll_net = owner;
            cell.toll = class as u8;
            if class == SEVERAL as usize {
                self.several.insert(ci as u32, vec![(owner, sum)]);
            }
        } else {
            cell.toll = SEVERAL;
            self.several.insert(ci as u32, vec![(old_owner, old), (owner, price)]);
        }
    }

    /// The prices of the classes for a net this many default tracks wide: what the search adds for a
    /// step onto a cell of class c that the net does not own.
    #[inline]
    pub fn price_table(&self, mul: u32) -> [u32; 256] {
        let mut tab = [0u32; 256];
        for (c, p) in self.classes.iter().enumerate() {
            tab[c] = p.at(mul);
        }
        tab
    }

    /// What a net pays for a step onto a cell that owes to several owners.
    #[cold]
    pub fn several_price(&self, ci: usize, net: u16, mul: u32) -> u32 {
        self.several.get(&(ci as u32)).map_or(0, |l| l.iter().filter(|e| e.0 != net).fold(0u32, |s, e| s.saturating_add(e.1.at(mul))))
    }

    /// What a net pays for a step onto cell `ci`, by the book (the search reads the same more quickly).
    pub fn step_price(&self, f: &Field, ci: usize, net: u16, mul: u32) -> u32 {
        let cell = &f.cells[ci];
        if cell.toll == SEVERAL {
            self.several_price(ci, net, mul)
        } else if cell.toll != 0 && cell.toll_net != net {
            self.classes[cell.toll as usize].at(mul)
        } else {
            0
        }
    }

    /// Reserve the via site at node `vi` (y * w + x) for `owner`: no other net may put a via there. A
    /// site reserved for several nets is open to each of them at the term's `via_shared` price.
    pub fn reserve_via(&mut self, term: TermId, vi: usize, owner: u16) {
        self.terms[term.0 as usize].via_sites += 1;
        let old = self.via_net[vi];
        if old == 0 || (old == owner && self.via_term[vi] == term.0) {
            self.via_net[vi] = owner;
            self.via_term[vi] = term.0;
        } else {
            let e = self.via_several.entry(vi as u32).or_default();
            if old != SEVERAL_NETS && !e.contains(&(old, TermId(self.via_term[vi]))) {
                e.push((old, TermId(self.via_term[vi])));
            }
            if !e.contains(&(owner, term)) {
                e.push((owner, term));
            }
            self.via_net[vi] = SEVERAL_NETS;
        }
    }

    /// What a via at node `vi`, reserved (`via_net[vi]` is neither 0 nor `net`), costs `net` more:
    /// None when the site is not this net's to use.
    #[cold]
    pub fn via_price(&self, vi: usize, net: u16) -> Option<u32> {
        if self.via_net[vi] != SEVERAL_NETS {
            return None;
        }
        let list = self.via_several.get(&(vi as u32))?;
        if !list.iter().any(|e| e.0 == net) {
            return None;
        }
        // once for every term under which another net shares the site
        let mut price = 0u32;
        for (k, e) in list.iter().enumerate() {
            if e.0 != net && !list[..k].iter().any(|d| d.0 != net && d.1 == e.1) {
                price = price.saturating_add(self.terms[e.1 .0 as usize].via_shared);
            }
        }
        Some(price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::{Cfg, Router};
    use crate::scene::Scene;
    use crate::search::{Outcome, Query};

    /// The tiny board: one net, a pad at (2, 3) and one at (8, 4), two layers.
    fn tiny() -> Router {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/scenes/tiny11.json")).unwrap();
        let mut r = Router::new(Scene::from_json(&crate::json::parse(&text).unwrap()).unwrap(), Cfg::default(), Vec::new()).unwrap();
        r.find_via_sites();
        // no price for bends: what a way costs is then the sum of what its nodes cost, and the search
        // (at weight 1) finds the cheapest
        r.costs.turn = [0; 4];
        r
    }

    /// What the cheapest way from the first pad to the second costs net 1, three default tracks wide.
    fn cost(r: &mut Router) -> u32 {
        let mut sources = Vec::new();
        r.pad_sources(0, &mut sources);
        let goal = r.acc[1][0];
        r.search.mark_target(&r.f, 0, goal.y as usize, goal.x as usize);
        let tight = crate::route::exact::Tight { statics: &r.statics, clr: &r.clr_tab, more: &r.more_tab };
        let q = Query {
            net: 1,
            keep: None,
            t_min: r.t_min(1),
            t_low: r.t_low(1),
            half: r.w_min(1) / 2.0,
            tight: &tight,
            shun: &[],
            t_nom: r.t_nom(1),
            t_via: r.kinds[0].t,
            t_via_s: 0,
            small_cost: 0,
            t_via_fixed: 0,
            sources: &sources,
            tbox: (goal.x as i32, goal.y as i32, goal.x as i32, goal.y as i32),
            tlayers: 1,
            plane_goal: None,
            soft: false,
            then_soft: false,
            hard_pops: u64::MAX,
            rip_cell: 1000,
            flat: true,
            via_cost: 5000,
            share_cost: 0,
            toll_mul: 3,
            weight256: 256,
            weight256_soft: 256,
            own_vias: &[],
            own_pads: &[],
            via_gap2: r.kinds[0].gap2,
            via_steps: r.via_steps,
            max_pops: u64::MAX,
        };
        let out = r.search.run(&r.f, &r.costs, &r.wear, &r.kinds[0].ok, &[], &q);
        r.search.clear_targets();
        match out {
            Outcome::Found(_, c, _) => c,
            _ => panic!("the tiny board has a way"),
        }
    }

    /// Charge every node of the column half way between the pads, on both layers.
    fn wall(r: &mut Router, term: TermId, owner: u16, price: Price) -> usize {
        let (x, _) = r.f.node_near(5.0, 3.0);
        for l in 0..r.f.nl {
            for y in 0..r.f.h {
                let ci = r.f.idx(l, y, x);
                r.costs.charge(&mut r.f, term, ci, owner, price);
            }
        }
        r.f.idx(0, r.f.h / 2, x)
    }

    #[test]
    fn the_search_adds_the_terms_prices_and_spares_the_owner() {
        let mut r = tiny();
        let free = cost(&mut r);
        let a = r.costs.term("a");
        assert_eq!(r.costs.term("a"), a);
        // every net pays a wall it must cross once
        let ci = wall(&mut r, a, 0, Price::flat(700));
        assert_eq!(cost(&mut r), free + 700);
        // a second term on the same nodes is paid on top; by the width: three times for this net
        let b = r.costs.term("b");
        wall(&mut r, b, 0, Price::per_width(100));
        assert_eq!(cost(&mut r), free + 700 + 300);
        // a charge owned by net 1 is not net 1's to pay, and every other net's
        let c = r.costs.term("c");
        wall(&mut r, c, 1, Price::flat(50));
        assert_eq!(r.f.cells[ci].toll, SEVERAL);
        assert_eq!(cost(&mut r), free + 700 + 300);
        assert_eq!(r.costs.step_price(&r.f, ci, 1, 3), 1000);
        assert_eq!(r.costs.step_price(&r.f, ci, 2, 3), 1050);
        assert_eq!(r.costs.terms[a.0 as usize].nodes, r.f.nl * r.f.h);
    }

    #[test]
    fn a_wall_owned_by_the_net_is_free_for_it() {
        let mut r = tiny();
        let free = cost(&mut r);
        let a = r.costs.term("a");
        let ci = wall(&mut r, a, 1, Price::flat(700));
        assert_eq!(cost(&mut r), free);
        assert_eq!((r.costs.step_price(&r.f, ci, 1, 1), r.costs.step_price(&r.f, ci, 2, 1)), (0, 700));
    }

    #[test]
    fn a_reserved_via_site_is_closed_to_others_and_shared_at_a_price() {
        let mut r = tiny();
        let t = r.costs.term("mouth");
        r.costs.terms[t.0 as usize].via_shared = 6000;
        let vi = 77;
        r.costs.reserve_via(t, vi, 5);
        assert_eq!(r.costs.via_net[vi], 5);
        assert_eq!(r.costs.via_price(vi, 1), None);
        r.costs.reserve_via(t, vi, 5);
        assert_eq!(r.costs.via_net[vi], 5);
        r.costs.reserve_via(t, vi, 6);
        r.costs.reserve_via(t, vi, 9);
        assert_eq!(r.costs.via_net[vi], SEVERAL_NETS);
        assert_eq!((r.costs.via_price(vi, 5), r.costs.via_price(vi, 6), r.costs.via_price(vi, 7)), (Some(6000), Some(6000), None));
    }
}
