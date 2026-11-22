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

/// Does a net's name say power? (`+5V`, `3V3`, `VBUS`, `VCC_IO`, `/power/VIN`)
pub fn says_power(name: &str) -> bool {
    let last = name.rsplit('/').next().unwrap_or(name).to_ascii_uppercase();
    let b = last.as_bytes();
    if b.is_empty() {
        return false;
    }
    if b[0] == b'+' || (b[0] == b'-' && b.len() > 1 && b[1].is_ascii_digit()) {
        return true;
    }
    if ["VCC", "VDD", "VIN", "VBUS", "VBAT", "VSYS", "VOUT", "VPP", "VDC", "VAA", "VEE", "VMOT", "AVDD", "DVDD", "AVCC", "PWR"].iter().any(|p| last.starts_with(p)) {
        return true;
    }
    // a voltage in the name: 3V3, 5V, 12V_SW
    (1..b.len()).any(|i| b[i] == b'V' && b[i - 1].is_ascii_digit() && (i + 1 == b.len() || b[i + 1].is_ascii_digit() || b[i + 1] == b'_'))
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
        // the pairs: laid side by side, or routed as two nets and why
        let said: Vec<(u16, String)> = self.duos.iter().filter(|d| !d.said.is_empty()).map(|d| (d.p, d.said.clone())).chain(self.duo_notes.iter().cloned()).collect();
        for (net, text) in said {
            self.note("pair", net, None, text);
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
        // nets kept to layers: how much runs where
        let mut kept: Vec<u16> = self.keeps.keys().copied().collect();
        kept.sort();
        for n in kept {
            let nl = self.sc.layers.len();
            let mut mm = vec![0.0f64; nl];
            for t in self.jobs.iter().filter(|j| j.net == n).flat_map(|j| j.tracks.iter()) {
                mm[t.layer as usize] += t.length();
            }
            let keep = self.sc.nets[n as usize].keep;
            let (on, other): (f64, Vec<String>) = ((0..nl).filter(|l| keep & (1 << l) != 0).map(|l| mm[l]).sum(), (0..nl).filter(|&l| keep & (1 << l) == 0 && mm[l] > 0.0).map(|l| format!("{:.1} mm on {}", mm[l], self.sc.layers[l])).collect());
            let stubs = if other.is_empty() { String::new() } else { format!("; {} from its pads there to their vias", other.join(", ")) };
            self.note("route on", n, None, format!("kept to {}: {:.1} mm there{}", self.kept_to(n), on, stubs));
        }
        // nets that were not asked for
        let left: Vec<&str> = self.sc.nets.iter().skip(1).filter(|n| n.pads.len() >= 2 && !n.in_scope).map(|n| n.name.as_str()).collect();
        if !left.is_empty() {
            let names = if left.len() <= 8 { left.join(", ") } else { format!("{}, and {} more", left[..8].join(", "), left.len() - 8) };
            self.note("not asked", 0, None, format!("{} nets were not asked for and get no copper ({}): their pads, their copper and the way out of their pins are kept clear of; whether they are joined is not looked at", left.len(), names));
        }
        // pads that lie on each other are joined as they are
        for &(p, other, at) in &self.old.shorts.clone() {
            let (pad, name) = (&self.sc.pads[p], self.sc.nets[other as usize].name.clone());
            let text = format!("the board came with a short: {}.{} lies on copper of {}. astar moves nothing the board came with; KiCad will call it a short", pad.ref_, pad.num, name);
            self.note("short", pad.net, Some(at), text);
        }
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
            self.note("ground", n as u16, None, format!("{} pads joined by {:.0} mm of track: the ground has no pour and no plane, and is wired as a net (--drops {} is for a block whose pour is its host's)", self.sc.nets[n].pads.len(), mm, name));
        }
        // rails at a signal's width: a net whose name says power, routed as track, no wider than the
        // board's plain track. The width is the board's to say (a net class); what astar can do is say so
        let thin: Vec<String> = (1..self.sc.nets.len())
            .filter(|&n| {
                let net = &self.sc.nets[n];
                let wired_ground = net.class == crate::scene::Class::Ground && net.pads.len() >= 3;
                net.wanted() && net.plane.is_none() && !self.pours.contains_key(&(n as u16)) && (says_power(&net.name) || wired_ground) && net.width <= self.sc.rules.track + 1e-9 && self.jobs.iter().any(|j| j.net == n as u16 && !j.tracks.is_empty())
            })
            .map(|n| self.sc.nets[n].name.clone())
            .collect();
        if !thin.is_empty() {
            let more = if thin.len() > 8 { format!(" and {} more", thin.len() - 8) } else { String::new() };
            self.note("thin", 0, None, format!("{} {} power by name and no wider than a signal, {} mm: {}{}. A net class with a track width makes a rail wide", thin.len(), if thin.len() == 1 { "net is" } else { "nets are" }, self.sc.rules.track, thin.iter().take(8).cloned().collect::<Vec<_>>().join(", "), more));
        }
        // the copper the router drew for a net: the ground's where the board drew none, and what the run named
        for &(n, l) in &self.poured.clone() {
            let net = &self.sc.nets[n as usize];
            let what = if net.plane == Some(l) { format!("{} is its plane: the layer is closed to tracks, and every pad of the net goes down to it by a via", self.sc.layers[l]) } else { format!("a pour of it is drawn over the board on {}: the layer stays open to tracks, and the net leans on the fill", self.sc.layers[l]) };
            if self.cfg.planes.iter().any(|(name, _)| *name == net.name) {
                self.note("planned", n, None, format!("{}, as --plane says", what));
            } else if net.class == crate::scene::Class::Ground {
                self.note("planned", n, None, format!("the board draws no copper for its ground. {} (--ground tracks wires it as a net; --plane gives it other layers)", what));
            }
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
