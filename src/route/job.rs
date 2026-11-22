//! A job: what the negotiation routes, and the copper it laid.

use super::Router;
use crate::geom::hypot;

#[derive(Clone)]
pub struct Track {
    pub layer: u8,
    pub width: f64,
    pub pts: Vec<(f64, f64)>,
    /// per vertex, once the corners are rounded: the arc (start, a point on it, end) that stands for it
    pub arcs: Vec<Option<((f64, f64), (f64, f64), (f64, f64))>>,
}

/// A piece of a track as it is drawn.
pub enum Piece {
    Line((f64, f64), (f64, f64)),
    Arc((f64, f64), (f64, f64), (f64, f64)),
}

impl Track {
    /// The track as lines and arcs, in order.
    pub fn pieces(&self) -> Vec<Piece> {
        let mut out = Vec::with_capacity(self.pts.len());
        let mut cur = self.pts[0];
        for i in 1..self.pts.len() {
            match self.arcs.get(i).copied().flatten() {
                Some((s, m, e)) => {
                    if hypot(s.0 - cur.0, s.1 - cur.1) > 1e-6 {
                        out.push(Piece::Line(cur, s));
                    }
                    out.push(Piece::Arc(s, m, e));
                    cur = e;
                }
                None => {
                    if hypot(self.pts[i].0 - cur.0, self.pts[i].1 - cur.1) > 1e-6 {
                        out.push(Piece::Line(cur, self.pts[i]));
                    }
                    cur = self.pts[i];
                }
            }
        }
        out
    }

    pub fn length(&self) -> f64 {
        self.pieces()
            .iter()
            .map(|p| match p {
                Piece::Line(a, b) => hypot(a.0 - b.0, a.1 - b.1),
                Piece::Arc(s, m, e) => crate::geom::arc_points(*s, *m, *e, 0.0005).windows(2).map(|w| hypot(w[0].0 - w[1].0, w[0].1 - w[1].1)).sum(),
            })
            .sum()
    }
}

/// A kind of job: what the negotiation can route (docs/hooks.md, "Job kind"). A pass makes jobs of its
/// kind (`Router::add_job`); the negotiation calls `route` for each, again whenever it lifted the job.
#[derive(Clone, Copy)]
pub struct JobKind {
    pub name: &'static str,
    /// the pass on whose clock its routing runs
    pub pass: &'static str,
    /// Route job j from nothing: lay its copper (`Router::commit`), say what stayed open (`Job::open`),
    /// set `Job::done`.
    pub route: fn(&mut Router, usize),
}

pub struct Job {
    pub kind: JobKind,
    pub net: u16,
    /// The one pad the job is about, when it is about one (a drop: the pad it takes into the plane).
    pub pad: Option<usize>,
    pub objs: Vec<u32>,
    pub tracks: Vec<Track>,
    /// vias this job laid, as nodes (x, y)
    pub vias: Vec<(u16, u16)>,
    /// those of its vias that are the small one
    pub small: Vec<(u16, u16)>,
    /// how many nodes its copper runs through
    pub nodes: usize,
    /// the nodes (field indices) where its copper was laid through another net's
    pub clash_cells: Vec<usize>,
    pub done: bool,
    /// pads this job could not join (indices into the scene's pads)
    pub open: Vec<usize>,
    /// times it was routed again with a pad still open
    pub tries: u32,
    /// pads of it the board itself leaves no way to (pads, edges, keep-outs, copper that stays), whatever
    /// the other nets do: seen the last time it was routed free to go through other nets' copper
    /// (`Router::solve`). They are open for good, and not tried again where that cannot change.
    pub walled: Vec<usize>,
    /// and how many of its loose pieces of old copper (`loose`) are walled in the same way
    pub walled_loose: usize,
    /// what stands in its way where it is open for want of room: the nets its nearest way runs through,
    /// and where (`Router::blame`)
    pub blockers: Vec<(u16, (f64, f64))>,
    /// its copper is locked (`Router::lock_job`): the job is never lifted, so never routed again
    pub locked: bool,
    /// old copper of the net that no pad stands on and that could not be joined to the rest: where
    pub loose: Vec<(f64, f64)>,
    /// a net with a pour: per pad, the group the pour had it in when the job was routed (`fill`)
    pub pour_pads: Vec<u32>,
    /// and the fill its copper leans on: (the node, the group the fill there was in)
    pub pour_ends: Vec<(u32, u32)>,
    /// times it was routed again in the rounds because its pour's fill had been cut
    pub refills: u32,
    /// the nodes of its paths whose copper is drawn off the node (`ends`): the node, where the copper is
    pub moved: Vec<((u8, u16, u16), (f64, f64))>,
    /// the paths its copper was drawn from, in the order they were laid: the path, the pads its ends
    /// stand on (`Router::commit`)
    pub paths: Vec<(Vec<(u8, u16, u16)>, Option<usize>, Option<usize>)>,
    /// some of it was drawn with a concession to what stood beside it: a narrowed stretch, an end laid
    /// as found (`redraw`)
    pub rough: bool,
    /// how many of its track ends at pads are on their true line, and how many are laid as found
    pub ends: (u32, u32),
    /// the net's half of its pair's run side by side, where it is in one (`duo`): the job's first track
    pub trunk: Option<super::duo::Trunk>,
}

impl Job {
    pub fn new(kind: JobKind, net: u16, pad: Option<usize>) -> Job {
        Job { kind, net, pad, objs: vec![], tracks: vec![], vias: vec![], small: vec![], nodes: 0, clash_cells: vec![], done: false, open: vec![], tries: 0, walled: vec![], walled_loose: 0, blockers: vec![], locked: false, loose: vec![], pour_pads: vec![], pour_ends: vec![], refills: 0, moved: vec![], paths: vec![], rough: false, ends: (0, 0), trunk: None }
    }
}

impl Router {
    /// A job for the negotiation to route, after those there are. Returns its number.
    pub fn add_job(&mut self, job: Job) -> usize {
        let j = self.jobs.len();
        if let Some(p) = job.pad {
            self.drop_of.insert(p, j);
        }
        self.jobs.push(job);
        self.deps.push(Vec::new());
        j
    }
}
