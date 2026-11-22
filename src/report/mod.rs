//! What a run has to say for itself: its counts, its open nets, the plan.

pub mod plan;

use crate::route::Router;
use crate::scene::Class;
use std::collections::HashMap;

pub struct Stats {
    pub build_s: f64,
    pub route_s: f64,
    pub routings: u64,
    /// jobs routed again because their copper clashed with another net's
    pub rips: u64,
    pub rounds: u32,
    pub fences: usize,
    /// corners of the routed tracks, and how many became arcs
    pub corners: usize,
    pub rounded: usize,
    /// seconds inside the search, laying copper and lifting it
    pub search_s: f64,
    pub lay_s: f64,
    pub lift_s: f64,
}

/// A report entry (docs/hooks.md, "Report entry"): something the run has to say about a net or a place.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// what it is about: "open", "narrowed", "choice", "unused", or a track's own
    pub kind: &'static str,
    /// the net it is about; 0: none
    pub net: u16,
    /// where, mm
    pub at: Option<(f64, f64)>,
    pub detail: String,
}

impl Router {
    /// The report hook: say something about a net or a place. Notes go into the plan in the order
    /// they were made. A note must be the same for the same input: no seconds, nothing of the machine.
    pub fn note(&mut self, kind: &'static str, net: u16, at: Option<(f64, f64)>, detail: String) {
        self.notes.push(Note { kind, net, at, detail });
    }

    /// What the end of a run says by itself: the nets left open and the widths narrowed.
    pub(crate) fn close_report(&mut self) {
        // vias laid at the board's least size where the class's own did not fit
        let (small, all): (usize, usize) = (self.jobs.iter().map(|j| j.small.len()).sum(), self.jobs.iter().map(|j| j.vias.len()).sum());
        if let (true, Some((size, drill))) = (small > 0, self.via_small) {
            self.note("small via", 0, None, format!("{} of {} vias are {} mm on a {} mm hole, the least the board allows, where the {} mm via did not fit", small, all, crate::json::num(size), crate::json::num(drill), crate::json::num(self.sc.rules.via)));
        }
        // copper the board came with, and what it joins
        if self.old.pieces != (0, 0) {
            let (pads, nets) = (self.old.isles.iter().filter(|i| i.copper).map(|i| i.pads.len()).sum::<usize>(), {
                let mut n: Vec<u16> = self.old.isles.iter().filter(|i| i.copper && !i.pads.is_empty()).map(|i| i.net).collect();
                n.sort();
                n.dedup();
                n.len()
            });
            self.note("kept", 0, None, format!("the board came with {} tracks and {} vias: they stay where they are, {} pads of {} nets stand on them, and new tracks start and end on them", self.old.pieces.0, self.old.pieces.1, pads, nets));
        }
        // nets that were not asked for
        let left: Vec<&str> = self.sc.nets.iter().skip(1).filter(|n| n.pads.len() >= 2 && !n.in_scope).map(|n| n.name.as_str()).collect();
        if !left.is_empty() {
            let names = if left.len() <= 8 { left.join(", ") } else { format!("{}, and {} more", left[..8].join(", "), left.len() - 8) };
            self.note("not asked", 0, None, format!("{} nets were not asked for and get no copper ({}): their pads, their copper and the way out of their pins are kept clear of; whether they are joined is not looked at", left.len(), names));
        }
        // pads that lie on each other are joined as they are
        if self.old.pad_isles > 0 {
            let pads: usize = self.old.isles.iter().filter(|i| !i.copper).map(|i| i.pads.len()).sum();
            self.note("touching", 0, None, format!("{} pads lie on another pad of their net in {} places: they are joined as they are, and get no track between them", pads, self.old.pad_isles));
        }
        // old copper that no pad stands on and nothing joins to its net: KiCad counts it as not joined
        let mut loose: Vec<(u16, (f64, f64))> = self.jobs.iter().flat_map(|j| j.loose.iter().map(move |&at| (j.net, at))).collect();
        for i in self.old.isles.iter().filter(|i| i.pads.is_empty()) {
            // (of a net with a plane: joined when it has a via through the plane's layer)
            if let (Some(pl), Some(&(_, y, x))) = (self.sc.nets[i.net as usize].plane, i.nodes.first()) {
                if i.layers & (1 << pl) == 0 {
                    loose.push((i.net, (self.f.wx(x as usize), self.f.wy(y as usize))));
                }
            }
        }
        for (net, at) in loose {
            self.note("loose", net, Some(at), "copper the board came with that no pad stands on, and that is not joined to the rest of its net".into());
        }
        // how ground is made: a net joined off this board, and a ground that is neither poured nor a plane
        for &n in &self.hosted.clone() {
            let (pads, vias) = (self.sc.nets[n as usize].pads.len(), self.jobs.iter().filter(|j| j.net == n).map(|j| j.vias.len()).sum::<usize>());
            self.note("drops", n, None, format!("joined off this board: its {} pads go down by {} vias, and nothing joins the vias here; the pour they reach is the host's", pads, vias));
        }
        for n in 1..self.sc.nets.len() {
            let net = &self.sc.nets[n];
            if net.class != crate::scene::Class::Ground || net.pads.len() < 3 || !net.in_scope || net.plane.is_some() || self.pours.contains_key(&(n as u16)) {
                continue;
            }
            let mm: f64 = self.jobs.iter().filter(|j| j.net == n as u16).flat_map(|j| j.tracks.iter()).map(|t| t.pts.windows(2).map(|w| crate::geom::hypot(w[1].0 - w[0].0, w[1].1 - w[0].1)).sum::<f64>()).sum();
            let name = net.name.clone();
            self.note("ground", n as u16, None, format!("{} pads joined by {:.0} mm of track: the board has no pour and no plane for its ground (a pour drawn on the board is used; --drops {} is for a block whose pour is its host's)", self.sc.nets[n].pads.len(), mm, name));
        }
        // track ends that are not on the line of the pad they end at
        let (on, off): (u32, u32) = self.jobs.iter().fold((0, 0), |s, j| (s.0 + j.ends.0, s.1 + j.ends.1));
        if off > 0 {
            self.note("ends", 0, None, format!("{} of {} track ends at pads are laid as the grid found them, a step beside the pad's own line, where that line was not clear", off, on + off));
        }
        for (n, got, all) in self.open_nets() {
            let pad = self.jobs.iter().filter(|j| j.net == n).find_map(|j| j.open.first().copied().or(if j.done { None } else { j.pad }));
            let at = pad.map(|p| (self.sc.pads[p].x, self.sc.pads[p].y));
            // why: a pad the board itself shuts in is a matter for the placement or the rules, one left
            // out among the other nets a matter of room
            let name = |p: &usize| format!("{}.{}", self.sc.pads[*p].ref_, self.sc.pads[*p].num);
            let of_net = || self.jobs.iter().filter(move |j| j.net == n);
            // what shuts the first walled pad in, and what stands in the way of the others, by name
            let wall: Vec<String> = of_net().flat_map(|j| j.open.iter().filter(move |p| j.walled.contains(p))).next().map_or(Vec::new(), |&p| self.wall_of(p));
            let mut through: Vec<(u16, (f64, f64))> = Vec::new();
            for j in of_net() {
                for &b in &j.blockers {
                    if !through.iter().any(|t| t.0 == b.0) {
                        through.push(b);
                    }
                }
            }
            let mut walled: Vec<String> = of_net().flat_map(|j| j.open.iter().filter(move |p| j.walled.contains(p))).map(name).collect();
            let mut crowded: Vec<String> = of_net().flat_map(|j| j.open.iter().filter(move |p| !j.walled.contains(p))).map(name).collect();
            // (a part's pads of one number, a mounting hole's ring of them, are named once)
            walled.dedup();
            crowded.dedup();
            let mut why = String::new();
            if !walled.is_empty() {
                // (named where the flood round the pad says what stopped it)
                let by = if wall.is_empty() {
                    "what stays round it (pads, an edge, a keep-out, copper the board came with) shuts it in".to_string()
                } else {
                    let (last, first) = wall.split_last().unwrap();
                    format!("{} is shut in by {}", walled[0], if first.is_empty() { last.clone() } else { format!("{} and {}", first.join(", "), last) })
                };
                why += &format!("; the board itself leaves no way to {}: {}, whatever the other nets do", walled.join(", "), by);
            }
            if !crowded.is_empty() {
                why += &format!("; no way was found to {} among the other nets", crowded.join(", "));
                if !through.is_empty() {
                    let named: Vec<String> = through.iter().take(4).map(|&(b, at)| format!("{} at ({:.2}, {:.2})", self.sc.nets[b as usize].name, at.0, at.1)).collect();
                    why += &format!(": the nearest way runs through {}{}", named.join(", "), if through.len() > 4 { format!(" and {} more", through.len() - 4) } else { String::new() });
                }
            }
            self.note("open", n, at, format!("{} of {} pads joined{}", got, all, why));
        }
        // a wide net drawn narrower than it asks, where its full width did not fit
        for ni in 1..self.sc.nets.len() {
            let want = self.sc.nets[ni].width;
            let (mut k, mut mm, mut at, mut least) = (0, 0.0, None, want);
            for j in self.jobs.iter().filter(|j| j.net as usize == ni) {
                for t in j.tracks.iter().filter(|t| t.width < want - 1e-9) {
                    k += 1;
                    mm += t.length();
                    at = at.or(t.pts.first().copied());
                    least = least.min(t.width);
                }
            }
            if k > 0 {
                self.note("narrowed", ni as u16, at, format!("{} stretches, {:.1} mm, down to {} mm where the net asks {} mm", k, mm, crate::json::num(least), crate::json::num(want)));
            }
        }
    }

    /// Nets with a pad left unjoined: (net, pads joined, pads in all). A pad no job is about (its pass
    /// was left out) is not joined.
    pub fn open_nets(&self) -> Vec<(u16, usize, usize)> {
        let mut m: HashMap<u16, usize> = HashMap::new();
        let mut has_job = vec![false; self.sc.nets.len()];
        for j in &self.jobs {
            has_job[j.net as usize] = true;
            let missing = if j.done { j.open.len() + j.loose.len() } else if j.pad.is_some() { 1 } else { self.sc.nets[j.net as usize].pads.len() - 1 };
            if missing > 0 {
                *m.entry(j.net).or_insert(0) += missing;
            }
        }
        for (ni, n) in self.sc.nets.iter().enumerate().skip(1) {
            if !n.wanted() {
                continue;
            }
            let missing = if n.plane.is_some() { n.pads.iter().filter(|&&p| !self.pad_in_plane(p) && !self.old_reaches_plane(p) && !self.drop_of.contains_key(&p)).count() } else if has_job[ni] { 0 } else { n.pads.len() - 1 };
            if missing > 0 {
                *m.entry(ni as u16).or_insert(0) += missing;
            }
        }
        let mut v: Vec<(u16, usize, usize)> = m.into_iter().map(|(n, k)| (n, self.sc.nets[n as usize].pads.len() - k.min(self.sc.nets[n as usize].pads.len()), self.sc.nets[n as usize].pads.len())).collect();
        v.sort();
        v
    }

    pub fn totals(&self) -> (f64, usize, u64, u64) {
        let mut mm = 0.0;
        let mut vias = 0;
        for j in &self.jobs {
            vias += j.vias.len();
            for t in &j.tracks {
                mm += t.length();
            }
        }
        (mm, vias, self.search.pops, self.search.searches)
    }

    pub fn is_ground(&self, net: u16) -> bool {
        self.sc.nets[net as usize].class == Class::Ground
    }
}
