//! The plan written: router-bench's plan JSON (its CONTRACT.md) and the run's own lines.

use crate::json::{num, quote};
use crate::route::{Job, Piece, Router};
use std::fmt::Write as _;

/// The plan as JSON. `read_s`, `total_s`: the seconds the caller spent reading the scene and in all.
pub fn plan_json(r: &Router, note: &str, read_s: f64, total_s: f64) -> String {
    let mut o = String::new();
    o.push_str("{\"version\": 1,\n \"nets\": [");
    let mut first = true;
    for (ni, n) in r.sc.nets.iter().enumerate() {
        let jobs: Vec<&Job> = r.jobs.iter().filter(|j| j.net as usize == ni && (!j.tracks.is_empty() || !j.vias.is_empty())).collect();
        if jobs.is_empty() {
            continue;
        }
        if !first {
            o.push(',');
        }
        first = false;
        let _ = write!(o, "\n  {{\"name\": {}, \"tracks\": [", quote(&n.name));
        let mut ft = true;
        for j in &jobs {
            for t in &j.tracks {
                if !ft {
                    o.push(',');
                }
                ft = false;
                let _ = write!(o, "\n    {{\"layer\": {}, \"width\": {}, \"path\": [", quote(&r.sc.layers[t.layer as usize]), num(t.width));
                for (k, p) in t.pieces().iter().enumerate() {
                    if k > 0 {
                        o.push_str(", ");
                    }
                    let _ = match p {
                        Piece::Line(a, b) => write!(o, "{{\"line\": [{}, {}, {}, {}]}}", num(a.0), num(a.1), num(b.0), num(b.1)),
                        Piece::Arc(s, m, e) => write!(o, "{{\"arc\": [{}, {}, {}, {}, {}, {}]}}", num(s.0), num(s.1), num(m.0), num(m.1), num(e.0), num(e.1)),
                    };
                }
                o.push_str("]}");
            }
        }
        o.push_str("],\n   \"vias\": [");
        let mut fv = true;
        for j in &jobs {
            for &(x, y) in &j.vias {
                if !fv {
                    o.push_str(", ");
                }
                fv = false;
                // every via says its size: the board's via, or the smaller one where that was laid. (A
                // class's own via size is not used yet; a plan that left the size out would be laid at the
                // board's and checked at the class's.)
                let (size, drill) = r.via_small.filter(|_| j.small.contains(&(x, y))).unwrap_or((r.sc.rules.via, r.sc.rules.drill));
                let _ = write!(o, "{{\"at\": [{}, {}], \"size\": {}, \"drill\": {}}}", num(r.f.wx(x as usize)), num(r.f.wy(y as usize)), num(size), num(drill));
            }
        }
        o.push_str("]}");
    }
    o.push_str("],\n \"planes\": [");
    for (k, &(n, l)) in r.poured.iter().enumerate() {
        if k > 0 {
            o.push_str(", ");
        }
        let _ = write!(o, "{{\"net\": {}, \"layer\": {}}}", quote(&r.sc.nets[n as usize].name), quote(&r.sc.layers[l]));
    }
    // a net joined off this board: its pads' vias reach a pour that is its host's (no copper is laid for it)
    for (k, &n) in r.hosted.iter().enumerate() {
        if k > 0 || !r.poured.is_empty() {
            o.push_str(", ");
        }
        let l = r.sc.nets[n as usize].plane.unwrap_or(0);
        let _ = write!(o, "{{\"net\": {}, \"layer\": {}, \"host\": true}}", quote(&r.sc.nets[n as usize].name), quote(&r.sc.layers[l]));
    }
    o.push_str("],\n \"left_out\": [");
    for (k, &(n, got, all)) in r.open_nets().iter().enumerate() {
        if k > 0 {
            o.push(',');
        }
        let _ = write!(o, "\n  {{\"net\": {}, \"reason\": {}}}", quote(&r.sc.nets[n as usize].name), quote(&format!("{} of {} pads joined", got, all)));
    }
    // the nets that were not asked for: the check does not ask whether they are whole
    let left: Vec<String> = r.sc.nets.iter().skip(1).filter(|n| n.pads.len() >= 2 && !n.in_scope).map(|n| quote(&n.name)).collect();
    let _ = write!(o, "],\n \"not_asked\": [{}", left.join(", "));
    let (mm, vias, pops, searches) = r.totals();
    let passes: Vec<String> = r.clock.lines().iter().map(|(name, s)| format!("{}: {:.4}", quote(name), s)).collect();
    let notes: Vec<String> = r
        .notes
        .iter()
        .map(|n| {
            let at = n.at.map_or(String::new(), |(x, y)| format!(", \"at\": [{}, {}]", num(x), num(y)));
            let net = if n.net == 0 { String::new() } else { format!(", \"net\": {}", quote(&r.sc.nets[n.net as usize].name)) };
            format!("\n  {{\"kind\": {}{}{}, \"detail\": {}}}", quote(n.kind), net, at, quote(&n.detail))
        })
        .collect();
    let skipped: Vec<String> = r.cfg.skip.iter().map(|s| quote(s)).collect();
    let _ = write!(
        o,
        "],\n \"moves\": [],\n \"notes\": [{}],\n \"timing_s\": {{\"read\": {:.4}, \"build\": {:.4}, \"route\": {:.4}, \"round\": {:.4}, \"total\": {:.4},\n   \"passes\": {{{}}}}},\n \"summary\": {{\"router\": \"astar\", \"note\": {}, \"pitch\": {}, \"grid\": [{}, {}], \"track_mm\": {:.1}, \"vias\": {}, \"fences\": {}, \"corners\": {}, \"rounded\": {}, \"routings\": {}, \"rounds\": {}, \"rerouted\": {}, \"searches\": {}, \"expanded\": {}, \"skipped\": [{}]}}\n}}\n",
        notes.join(","),
        read_s,
        r.stats.build_s,
        r.stats.route_s,
        r.clock.secs("arcs"),
        total_s,
        passes.join(", "),
        quote(note),
        num(r.f.pitch),
        r.f.w,
        r.f.h,
        mm,
        vias,
        r.stats.fences,
        r.stats.corners,
        r.stats.rounded,
        r.stats.routings,
        r.stats.rounds,
        r.stats.rips,
        searches,
        pops,
        skipped.join(", ")
    );
    o
}

/// The run in one line, and with `verbose` what lies behind it.
pub fn lines(r: &Router, note: &str, read_s: f64, total_s: f64, verbose: bool) -> Vec<String> {
    let open = r.open_nets();
    let (mm, vias, pops, searches) = r.totals();
    let nets = r.sc.nets.iter().skip(1).filter(|n| n.wanted()).count();
    let left = r.sc.nets.iter().skip(1).filter(|n| n.pads.len() >= 2 && !n.in_scope).count();
    let mut out = vec![format!(
        "astar: {} of {} nets joined{}, {:.1} mm, {} vias; {:.3} s (read {:.3}, build {:.3}, route {:.3}{}); {} searches, {} expanded, {} rounds, {} routed again{}",
        nets - open.len(),
        nets,
        if left > 0 { format!(" ({} not asked for)", left) } else { String::new() },
        mm,
        vias,
        total_s,
        read_s,
        r.stats.build_s,
        r.stats.route_s,
        // the time that went on boards that were not kept (the other way of a pour, a run with the style
        // rules on): said, so that the parts add up to the whole
        {
            let rest = total_s - read_s - r.stats.build_s.max(0.0) - r.stats.route_s;
            if rest > 0.2 && rest > total_s * 0.1 {
                format!(", {:.3} on boards not kept", rest)
            } else {
                String::new()
            }
        },
        searches,
        pops,
        r.stats.rounds,
        r.stats.rips,
        if note.is_empty() { String::new() } else { format!("; {}", note) }
    )];
    if !r.cfg.skip.is_empty() {
        out.push(format!("astar: passes left out: {}", r.cfg.skip.join(", ")));
    }
    for n in r.notes.iter().filter(|n| n.kind == "unused") {
        out.push(format!("astar: note: {}", n.detail));
    }
    let narrowed = r.notes.iter().filter(|n| n.kind == "narrowed").count();
    if narrowed > 0 {
        out.push(format!("astar: note: {} nets are drawn narrower than they ask where their width did not fit (the plan's notes say where; --strict exits 3 for it)", narrowed));
    }
    // the nets left open, and why (`close_report`): the first dozen, the plan's notes have them all
    let opens: Vec<&crate::report::Note> = r.notes.iter().filter(|n| n.kind == "open").collect();
    for n in opens.iter().take(12) {
        out.push(format!("astar: open: {}{}: {}", r.sc.nets[n.net as usize].name, if r.is_ground(n.net) { " (ground)" } else { "" }, n.detail));
    }
    if opens.len() > 12 {
        out.push(format!("astar: open: and {} more nets (the plan's notes say which, and why)", opens.len() - 12));
    }
    if verbose {
        out.push(format!("  passes, s: {}", r.clock.lines().iter().map(|(name, s)| format!("{} {:.3}", name, s)).collect::<Vec<_>>().join(", ")));
        if !r.costs.terms.is_empty() {
            out.push(format!("  cost terms: {}", r.costs.terms.iter().map(|t| format!("{} ({} nodes priced, {} via sites reserved)", t.name, t.nodes, t.via_sites)).collect::<Vec<_>>().join(", ")));
        }
        out.push(format!("  corners: {} of {} rounded, {:.3} s", r.stats.rounded, r.stats.corners, r.clock.secs("arcs")));
        out.push(format!(
            "  route: search {:.3} s, laying copper {:.3} s, lifting {:.3} s, the rest {:.3} s",
            r.stats.search_s,
            r.stats.lay_s,
            r.stats.lift_s,
            r.stats.route_s - r.stats.search_s - r.stats.lay_s - r.stats.lift_s
        ));
        for n in r.notes.iter().filter(|n| n.kind == "narrowed") {
            out.push(format!("  {}: {}{}", n.kind, if n.net == 0 { String::new() } else { format!("{}: ", r.sc.nets[n.net as usize].name) }, n.detail));
        }
    }
    out
}
