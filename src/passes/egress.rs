//! The egress plan: before any copper is laid, which pads must leave which parts, through which gaps,
//! toward where; a way out for each; and where there are not enough of them (plan/T04-egress.md).
//!
//! ```text
//! astar egress SCENE.json [--pitch MM] [--skip PASS] [--part REF] [-v]
//! ```
//!
//! prints the plan and exits 0, 1 when a pad has no way out (a deficit), 2 when the scene cannot be taken.

pub mod assign;
pub mod demand;
pub mod exits;
pub mod gaps;
pub mod geo;

use crate::passes::{Step, PASSES};
use crate::route::{Cfg, Router};
use crate::scene::Scene;
use crate::Error;
use demand::{Demand, Part, Want};
use geo::COMPASS;
use std::time::Instant;

/// The egress plan of a board.
pub struct Egress {
    pub parts: Vec<Part>,
    /// one entry a pad that has to be joined
    pub demands: Vec<Demand>,
    /// the gaps between fixed copper and the regions they close off, per layer
    pub gaps: gaps::Gaps,
    /// per demand: its ways out, the cheapest first
    pub cands: Vec<Vec<exits::Cand>>,
    /// nodes the search for ways out expanded
    pub pops: u64,
    /// what may not be chosen together
    pub model: assign::Model,
    /// per demand: the way out it was given (an index into its candidates), or none
    pub choice: Vec<Option<usize>>,
    /// groups of pads whose assignment was not searched to the end
    pub cut_short: usize,
    /// seconds: (what, how long)
    pub secs: Vec<(&'static str, f64)>,
}

/// More pads on a part's side want out than there are ways.
pub struct Deficit {
    pub part: usize,
    pub side: u8,
    /// the layer the pads are on (a through-hole pad's first)
    pub layer: usize,
    pub need: usize,
    pub have: usize,
    /// the demands left without a way
    pub short: Vec<usize>,
    /// what would help, the least drastic first
    pub remedies: Vec<String>,
}

/// The board as the route would be given it, up to the point where copper is laid: the board passes,
/// the field, the plan passes before this one, the via sites.
pub fn prepare(sc: Scene, cfg: &Cfg) -> Result<Router, Error> {
    crate::route::check(&sc, cfg).map_err(Error::BadInput)?;
    let on = |i: usize| !cfg.skip.iter().any(|s| s == PASSES[i].name);
    let mut changed = sc.clone();
    let mut poured = Vec::new();
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Board(f), true) = (p.step, on(i)) {
            poured.extend(f(&mut changed, cfg));
        }
    }
    let (sc, poured) = if poured.is_empty() || crate::route::check(&changed, cfg).is_err() { (sc, Vec::new()) } else { (changed, poured) };
    let mut r = Router::new(sc, cfg.clone(), poured).map_err(Error::BadInput)?;
    for (i, p) in PASSES.iter().enumerate() {
        if let (Step::Plan(f), true) = (p.step, on(i)) {
            if p.name != "egress" {
                f(&mut r);
            }
        }
    }
    r.find_via_sites();
    Ok(r)
}

/// The egress plan of a board that stands ready to be routed (`prepare`).
pub fn analyse(r: &mut Router) -> Egress {
    let mut secs = Vec::new();
    let t = Instant::now();
    let (parts, demands) = demand::demand(r);
    secs.push(("demand", t.elapsed().as_secs_f64()));
    let t = Instant::now();
    let gaps = gaps::find(r);
    secs.push(("gaps", t.elapsed().as_secs_f64()));
    let t = Instant::now();
    let mut ws = exits::Work::new(r);
    let anchors = exits::anchors(r);
    let cands: Vec<Vec<exits::Cand>> = demands.iter().map(|d| exits::candidates(r, &gaps, &anchors, d, &mut ws, false)).collect();
    secs.push(("exits", t.elapsed().as_secs_f64()));
    let pops = ws.pops;
    let t = Instant::now();
    let model = assign::model(r, &gaps, &demands, &cands);
    let (choice, cut_short) = assign::solve(&model, &cands);
    secs.push(("assign", t.elapsed().as_secs_f64()));
    Egress { parts, demands, gaps, cands, pops, model, choice, cut_short, secs }
}

impl Egress {
    /// Demand by part and side: (part, side, the demands there).
    pub fn sides(&self) -> Vec<(usize, u8, Vec<usize>)> {
        let mut out: Vec<(usize, u8, Vec<usize>)> = Vec::new();
        for (di, d) in self.demands.iter().enumerate() {
            match out.iter_mut().find(|s| s.0 == d.part && s.1 == d.side) {
                Some(s) => s.2.push(di),
                None => out.push((d.part, d.side, vec![di])),
            }
        }
        out.sort_by_key(|s| (s.0, s.1));
        out
    }

    /// The plan in words. `only`: one part's lines; `verbose`: a line a pad.
    pub fn lines(&self, r: &Router, only: Option<&str>, verbose: bool) -> Vec<String> {
        let mut out = Vec::new();
        let want = |di: &usize, w: Want| self.demands[*di].want == w;
        out.push(format!("demand: {} pads to join on {} parts ({} of the board's {} pads)", self.demands.len(), self.sides().iter().map(|s| s.0).collect::<std::collections::BTreeSet<_>>().len(), self.demands.len(), r.sc.pads.len()));
        out.push("  part       side pads  travel  hop drop pair  toward".to_string());
        for (part, side, ds) in self.sides() {
            let p = &self.parts[part];
            if only.map_or(false, |o| o != p.ref_) {
                continue;
            }
            let mut toward: Vec<(usize, &str)> = Vec::new();
            for &di in &ds {
                let t = self.demands[di].toward();
                match toward.iter_mut().find(|e| e.1 == t) {
                    Some(e) => e.0 += 1,
                    None => toward.push((1, t)),
                }
            }
            toward.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
            out.push(format!(
                "  {:<10} {:<4} {:>4}  {:>6} {:>4} {:>4} {:>4}  {}",
                p.ref_,
                COMPASS[side as usize],
                ds.len(),
                ds.iter().filter(|d| want(d, Want::Traveller)).count(),
                ds.iter().filter(|d| want(d, Want::Hop)).count(),
                ds.iter().filter(|d| want(d, Want::Drop)).count(),
                ds.iter().filter(|d| want(d, Want::Pair)).count(),
                toward.iter().filter(|e| e.1 != "-").map(|e| format!("{} {}", e.1, e.0)).collect::<Vec<_>>().join(", ")
            ));
            if verbose {
                for &di in &ds {
                    let d = &self.demands[di];
                    let pad = &r.sc.pads[d.pad];
                    out.push(format!("      {}.{:<5} {:<9} {:<24} to {:>2} {:>5.1} mm", pad.ref_, pad.num, d.want.name(), r.sc.nets[pad.net as usize].name, d.toward(), d.dist));
                }
            }
        }
        out.extend(self.gap_lines(r, only, verbose));
        out.extend(self.exit_lines(r, only, verbose));
        out.extend(self.plan_lines(r, only, verbose));
        out.push(format!("seconds: {}", self.secs.iter().map(|(n, s)| format!("{} {:.4}", n, s)).collect::<Vec<_>>().join(", ")));
        out
    }

    /// The gaps and the regions in words.
    fn gap_lines(&self, r: &Router, only: Option<&str>, verbose: bool) -> Vec<String> {
        let g = &self.gaps;
        let mut out = Vec::new();
        for &l in &r.routable {
            let on: Vec<&gaps::Gap> = g.gaps.iter().filter(|x| x.layer as usize == l).collect();
            let by = |n: u32, lanes: bool| on.iter().filter(|x| (if lanes { x.lanes } else { x.tracks }).min(3) == n).count();
            let pockets: Vec<&gaps::Region> = g.regions.iter().filter(|x| x.layer as usize == l && !x.open).collect();
            out.push(format!(
                "gaps on {}: {}; by the rules {} pass no track, {} one, {} two, {} more; on this grid {} none, {} one, {} two, {} more; {} pockets holding {} via sites",
                r.sc.layers[l],
                on.len(),
                by(0, false),
                by(1, false),
                by(2, false),
                by(3, false),
                by(0, true),
                by(1, true),
                by(2, true),
                by(3, true),
                pockets.len(),
                pockets.iter().map(|x| x.vias).sum::<u32>()
            ));
        }
        if verbose || only.is_some() {
            for x in &g.gaps {
                let (a, b) = (g.name(r, x.a), g.name(r, x.b));
                let of = |n: &str| only.map_or(true, |o| n.split('.').next() == Some(o));
                if !of(&a) && !of(&b) {
                    continue;
                }
                out.push(format!("      {:<10} | {:<10} {:<7} {:.3} mm: {} by the rules, {} on this grid", a, b, r.sc.layers[x.layer as usize], x.width, x.tracks, x.lanes));
            }
        }
        out
    }

    /// A pad by name.
    pub fn pad_name(&self, r: &Router, di: usize) -> String {
        let pad = &r.sc.pads[self.demands[di].pad];
        format!("{}.{}", pad.ref_, pad.num)
    }

    /// The ways out in words.
    fn exit_lines(&self, r: &Router, only: Option<&str>, verbose: bool) -> Vec<String> {
        let mut out = Vec::new();
        let total: usize = self.cands.iter().map(|c| c.len()).sum();
        let none: Vec<String> = (0..self.demands.len()).filter(|&di| self.cands[di].is_empty()).map(|di| self.pad_name(r, di)).collect();
        out.push(format!("ways out: {} for {} pads ({} nodes searched); {} pads have none{}", total, self.demands.len(), self.pops, none.len(), if none.is_empty() { String::new() } else { format!(": {}", none.join(", ")) }));
        if verbose || only.is_some() {
            for (di, d) in self.demands.iter().enumerate() {
                if only.map_or(false, |o| o != self.parts[d.part].ref_) {
                    continue;
                }
                let ways: Vec<String> = self.cands[di]
                    .iter()
                    .map(|c| format!("{} {:.1}{}", c.name(r, &self.gaps), c.len, if c.gaps.is_empty() || matches!(c.how, exits::How::Gap(_)) { String::new() } else { format!(" ({} gaps)", c.gaps.len()) }))
                    .collect();
                out.push(format!("      {:<10} {:<9} to {:<2}: {}", self.pad_name(r, di), d.want.name(), d.toward(), if ways.is_empty() { "NO WAY OUT".to_string() } else { ways.join(", ") }));
            }
        }
        out
    }

    /// The way out a pad was given.
    pub fn exit(&self, di: usize) -> Option<&exits::Cand> {
        self.choice[di].map(|c| &self.cands[di][c])
    }

    /// Where there are not enough ways out: per part, side and layer.
    pub fn deficits(&self, r: &Router) -> Vec<Deficit> {
        let mut out: Vec<Deficit> = Vec::new();
        for (part, side, ds) in self.sides() {
            let mut layers: Vec<u32> = ds.iter().map(|&di| self.demands[di].layers).collect();
            layers.sort_unstable();
            layers.dedup();
            for lm in layers {
                let here: Vec<usize> = ds.iter().copied().filter(|&di| self.demands[di].layers == lm).collect();
                let short: Vec<usize> = here.iter().copied().filter(|&di| self.choice[di].is_none()).collect();
                if short.is_empty() {
                    continue;
                }
                let layer = (0..r.f.nl).find(|l| lm & (1 << l) != 0).unwrap_or(0);
                let remedies = self.remedies(r, part, &short);
                out.push(Deficit { part, side, layer, need: here.len(), have: here.len() - short.len(), short, remedies });
            }
        }
        out
    }

    /// What would give the pads of a deficit their ways out, the least drastic first: another layer, a
    /// via under the part, a neck to the fab's floor, more room at the gap that is full.
    fn remedies(&self, r: &Router, part: usize, short: &[usize]) -> Vec<String> {
        let mut out = Vec::new();
        let (w, c) = (r.sc.rules.track, r.sc.rules.clearance);
        let f = &r.f;
        // another layer: a via beside the pad that another pad's way has taken
        let with_via: Vec<String> = short.iter().filter(|&&di| self.cands[di].iter().any(|k| k.via.is_some() && !k.dead)).map(|&di| self.pad_name(r, di)).collect();
        if !with_via.is_empty() {
            out.push(format!("another layer: {} can reach a via site, but each is taken by a neighbour's way out: free one", with_via.join(", ")));
        } else if r.routable.len() < 2 {
            out.push("another layer: there is none that carries tracks".to_string());
        }
        // a via inside or under the part
        let b = self.parts[part].bbox;
        let (x0, y0) = f.node_near(b.0, b.1);
        let (x1, y1) = f.node_near(b.2, b.3);
        let net = r.sc.pads[self.demands[short[0]].pad].net;
        let sites = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).filter(|&(x, y)| exits::via_site(r, net, x, y, false)).count();
        if sites > 0 && with_via.is_empty() {
            out.push(format!("a via under the part: {} nodes inside its outline can hold one, none within reach of these pads on their layer", sites));
        } else if sites == 0 {
            out.push("a via under the part: no node inside its outline can hold one".to_string());
        }
        // the gaps these pads' ways need and find full
        let mut full: Vec<u32> = Vec::new();
        for &di in short {
            for k in &self.cands[di] {
                for &g in &k.gaps {
                    if !full.contains(&g) {
                        full.push(g);
                    }
                }
            }
        }
        full.sort_by(|&a, &b| self.gaps.gaps[a as usize].usable.partial_cmp(&self.gaps.gaps[b as usize].usable).unwrap().then(a.cmp(&b)));
        if let Some(&g) = full.first() {
            let gp = &self.gaps.gaps[g as usize];
            let (a, bn) = (self.gaps.name(r, gp.a), self.gaps.name(r, gp.b));
            // a neck to the floor, where the fab allows a narrower track
            let fw = r.sc.rules.floors.track.unwrap_or(w);
            let fc = r.sc.rules.floors.clearance.unwrap_or(c);
            let at_floor = gaps::tracks_through(gp.usable, fw, fc);
            if at_floor > gp.tracks {
                out.push(format!("a neck: at the fab's floor ({} mm track, {} mm clearance) the gap {} | {} holds {} tracks where it holds {}", crate::json::num(fw), crate::json::num(fc), a, bn, at_floor, gp.tracks));
            } else {
                out.push(format!("a neck: none to be had: the rules give no narrower track{}", if r.sc.rules.floors.track.is_some() { " that would add a lane" } else { " (the scene names no fab floor)" }));
            }
            // more room: the part on the other side of the gap, away from this one
            let more = (gp.lanes as f64 + 1.0) * (w + c) + c - gp.usable;
            let own = |n: &str| n.split('.').next() == Some(self.parts[part].ref_.as_str());
            let (other, dir) = if own(&a) { (bn.clone(), (gp.pb.0 - gp.pa.0, gp.pb.1 - gp.pa.1)) } else { (a.clone(), (gp.pa.0 - gp.pb.0, gp.pa.1 - gp.pb.1)) };
            let heading = COMPASS[geo::compass(dir.0, dir.1) as usize];
            let what = match other.split('.').next() {
                Some(p) if other.contains('.') && !own(&other) => format!("move {} {} by {:.2} mm", p, heading, more.max(0.01)),
                _ => format!("the {} to the {} of {} is {:.2} mm too near", other, heading, if own(&a) { &a } else { &bn }, more.max(0.01)),
            };
            out.push(format!("more room: the gap {} | {} ({:.2} mm) holds {} and is full: {}", a, bn, gp.width, gp.lanes, what));
        }
        out
    }

    /// The assignment in words.
    fn plan_lines(&self, r: &Router, only: Option<&str>, verbose: bool) -> Vec<String> {
        let mut out = Vec::new();
        let given = self.choice.iter().filter(|c| c.is_some()).count();
        let by_via = (0..self.demands.len()).filter(|&di| self.exit(di).map_or(false, |c| c.via.is_some())).count();
        out.push(format!(
            "the plan: {} of {} pads have a way out: {} on their own layer, {} by a via; {} have none{}",
            given,
            self.demands.len(),
            given - by_via,
            by_via,
            self.demands.len() - given,
            if self.cut_short > 0 { format!(" ({} groups of pads not searched to the end)", self.cut_short) } else { String::new() }
        ));
        if verbose || only.is_some() {
            out.push("  part       side need  own layer  by via  short".to_string());
            for (part, side, ds) in self.sides() {
                if only.map_or(false, |o| o != self.parts[part].ref_) {
                    continue;
                }
                let via = ds.iter().filter(|&&di| self.exit(di).map_or(false, |c| c.via.is_some())).count();
                let none = ds.iter().filter(|&&di| self.choice[di].is_none()).count();
                out.push(format!("  {:<10} {:<4} {:>4}  {:>9}  {:>6}  {:>5}", self.parts[part].ref_, COMPASS[side as usize], ds.len(), ds.len() - via - none, via, none));
                for &di in &ds {
                    let d = &self.demands[di];
                    out.push(format!(
                        "      {:<10} {:<9} to {:<2}: {}",
                        self.pad_name(r, di),
                        d.want.name(),
                        d.toward(),
                        match self.exit(di) {
                            Some(c) => format!("{} {:.1} mm{}", c.name(r, &self.gaps), c.len, c.via.map_or(String::new(), |v| format!(" at ({:.2}, {:.2})", r.f.wx(v.0 as usize), r.f.wy(v.1 as usize)))),
                            None => "NO WAY OUT".to_string(),
                        }
                    ));
                }
            }
        }
        for d in self.deficits(r) {
            let names: Vec<String> = d.short.iter().map(|&di| self.pad_name(r, di)).collect();
            out.push(format!(
                "DEFICIT: {} side {} on {}: need {}, have {}, short by {}: {}",
                self.parts[d.part].ref_,
                COMPASS[d.side as usize],
                r.sc.layers[d.layer],
                d.need,
                d.have,
                d.short.len(),
                names.join(", ")
            ));
            for line in &d.remedies {
                out.push(format!("    {}", line));
            }
        }
        out
    }

    /// What `astar egress` exits with: 1 when a pad has no way out.
    pub fn exit_code(&self) -> i32 {
        if self.choice.iter().any(|c| c.is_none()) {
            crate::exit::OPEN
        } else {
            crate::exit::WHOLE
        }
    }
}

/// `astar egress`: the egress plan of a scene, printed. Returns the exit code.
pub fn command(args: &[String]) -> i32 {
    const USAGE: &str = "usage: astar egress SCENE.json [--pitch MM] [--skip PASS] [--part REF] [-v]\n         exit: 0 every pad has a way out, 1 a deficit, 2 the input could not be taken";
    let mut cfg = Cfg::default();
    let (mut file, mut only, mut verbose) = (None, None, false);
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| -> Option<&String> {
            *i += 1;
            args.get(*i)
        };
        match args[i].as_str() {
            "--pitch" => match next(&mut i).and_then(|s| s.parse::<f64>().ok()) {
                Some(v) => cfg.pitch = v,
                None => return bad(USAGE),
            },
            "--skip" => match next(&mut i) {
                Some(s) if PASSES.iter().any(|p| p.name == s) => cfg.skip.push(s.clone()),
                _ => return bad(USAGE),
            },
            "--part" => match next(&mut i) {
                Some(s) => only = Some(s.clone()),
                None => return bad(USAGE),
            },
            "-v" | "--verbose" => verbose = true,
            a if a.starts_with('-') => return bad(USAGE),
            a => file = Some(a.to_string()),
        }
        i += 1;
    }
    let Some(file) = file else { return bad(USAGE) };
    let t0 = Instant::now();
    let sc = match Scene::load(&file) {
        Ok(sc) => sc,
        Err(e) => return bad(&format!("astar: {}", e)),
    };
    let mut r = match prepare(sc, &cfg) {
        Ok(r) => r,
        Err(e) => return bad(&format!("astar: {}: {}", file, e)),
    };
    let built = t0.elapsed().as_secs_f64();
    let t1 = Instant::now();
    let eg = analyse(&mut r);
    let took = t1.elapsed().as_secs_f64();
    let mut out = vec![format!("egress: {}: grid {} by {} at {} mm, {} layers{}", file, r.f.w, r.f.h, crate::json::num(r.f.pitch), r.f.nl, r.poured.iter().map(|&(n, l)| format!("; {} poured on {}", r.sc.nets[n as usize].name, r.sc.layers[l])).collect::<String>())];
    out.extend(eg.lines(&r, only.as_deref(), verbose));
    out.push(format!("the plan took {:.3} s (the board read and its field built: {:.3} s)", took, built));
    // (a reader that stops reading early is not an error)
    let _ = std::io::Write::write_all(&mut std::io::stdout(), (out.join("\n") + "\n").as_bytes());
    eg.exit_code()
}

fn bad(what: &str) -> i32 {
    eprintln!("{}", what);
    crate::exit::BAD_INPUT
}
