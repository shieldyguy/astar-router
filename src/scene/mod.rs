//! The scene: the board as the router is given it. Two formats are read: router-bench's 1.1 (its
//! CONTRACT.md) and astar's own scene 2 (docs/scene.md), which carries what 1.1 has no place for.
//! What scene 2 adds is carried here and not used until its track uses it (`Scene::carried_unused`).

mod check;
mod read;
mod read2;

use crate::geom::Shape;

#[derive(Clone)]
pub struct Rules {
    pub track: f64,
    pub clearance: f64,
    pub via: f64,
    pub drill: f64,
    pub edge: f64,
    pub hole_clearance: f64,
    /// (scene 2) hole edge to hole edge
    pub hole_to_hole: Option<f64>,
    /// (scene 2) the fewest spokes a thermal relief may be left with (KiCad's default: 2)
    pub min_spokes: Option<f64>,
    /// (scene 2) what the fab can make: nothing is drawn or narrowed below these
    pub floors: Floors,
    /// (scene 2) rules that hold on one layer only; they replace the class's there
    pub layer_rules: Vec<LayerRule>,
}

#[derive(Clone, Default)]
pub struct Floors {
    pub track: Option<f64>,
    pub clearance: Option<f64>,
    pub via: Option<f64>,
    pub drill: Option<f64>,
    pub annular: Option<f64>,
    pub hole_to_hole: Option<f64>,
    pub edge: Option<f64>,
}

#[derive(Clone)]
pub struct LayerRule {
    pub layer: usize,
    pub track: Option<f64>,
    pub clearance: Option<f64>,
}

/// (scene 2) A net class. classes[0] is the Default class.
#[derive(Clone)]
pub struct NetClass {
    pub name: String,
    pub track: Option<f64>,
    pub clearance: Option<f64>,
    pub via: Option<f64>,
    pub drill: Option<f64>,
    pub pair_width: Option<f64>,
    pub pair_gap: Option<f64>,
    pub impedance: Option<Impedance>,
    pub layer_rules: Vec<LayerRule>,
}

/// (scene 2) The clearance between a net of class `a` and a net of class `b`.
#[derive(Clone)]
pub struct ClassPair {
    pub a: usize,
    pub b: usize,
    pub clearance: f64,
}

#[derive(Clone)]
pub struct Impedance {
    pub ohms: f64,
    pub differential: bool,
    /// percent
    pub tolerance: Option<f64>,
    pub coplanar_gap: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StackKind {
    Copper,
    Core,
    Prepreg,
    Mask,
}

/// (scene 2) One layer of the stackup, top to bottom, copper and what lies between.
#[derive(Clone)]
pub struct StackLayer {
    pub name: String,
    pub kind: StackKind,
    pub thickness: f64,
    pub epsilon_r: Option<f64>,
    pub loss_tangent: Option<f64>,
    pub material: Option<String>,
}

#[derive(Clone)]
pub struct Pad {
    pub ref_: String,
    pub num: String,
    pub net: u16,
    pub x: f64,
    pub y: f64,
    /// the centre of the pad's footprint
    pub fx: f64,
    pub fy: f64,
    pub tht: bool,
    pub back: bool,
    pub shape: Shape,
    /// The pad's own clearance when it has one, else 0.
    pub clearance: f64,
    /// (scene 2) the pad's drill
    pub drill: Option<f64>,
    /// (scene 2) how a pour of its net joins the pad, when the pad says so itself, and its own relief gap
    pub pour: Option<ZonePads>,
    pub thermal_gap: Option<f64>,
    /// (scene 2) the angle its relief's spokes stand at in the pad's own frame, degrees
    pub spoke_angle: Option<f64>,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Class {
    Signal,
    Rail,
    Ground,
}

#[derive(Clone)]
pub struct Net {
    pub name: String,
    /// what the net is for: signal, rail or ground (1.1's `class`, scene 2's `role`)
    pub class: Class,
    /// Track width in mm (the Default class's unless the net has its own).
    pub width: f64,
    /// The net's own clearance when it has one, else 0.
    pub clearance: f64,
    pub pads: Vec<usize>,
    /// The copper layer this net owns a plane on.
    pub plane: Option<usize>,
    /// (scene 2) its net class, an index into `Scene::classes`
    pub netclass: usize,
    /// (scene 2) its own via size and drill
    pub via: Option<f64>,
    pub drill: Option<f64>,
    pub impedance: Option<Impedance>,
    /// (scene 2) false: the net is not to be routed
    pub in_scope: bool,
    /// the layers its tracks are kept to, as a mask over `Scene::layers`; 0: any layer. A pad of the net
    /// on another layer is left by a short stub to a via (`Cfg::stub_mm`).
    pub keep: u32,
}

impl Net {
    /// Whether the router is to join this net: it has pads to join, and it is asked for (`in_scope`).
    /// A net not asked for keeps its pads, its copper and its way out of its pins; it gets no copper.
    pub fn wanted(&self) -> bool {
        self.pads.len() >= 2 && self.in_scope
    }

    /// A net as scene 1.1 knows one.
    pub fn plain(name: String, class: Class, width: f64, clearance: f64) -> Net {
        Net { name, class, width, clearance, pads: vec![], plane: None, netclass: 0, via: None, drill: None, impedance: None, in_scope: true, keep: 0 }
    }
}

/// What a keep-out forbids. Scene 1.1's forbid tracks, vias and zones.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Forbid {
    pub tracks: bool,
    pub vias: bool,
    pub zones: bool,
    pub pads: bool,
    pub footprints: bool,
}

pub const FORBID_COPPER: Forbid = Forbid { tracks: true, vias: true, zones: true, pads: false, footprints: false };

#[derive(Clone)]
pub struct Keepout {
    pub layers: Vec<usize>,
    pub poly: Vec<(f64, f64)>,
    pub forbid: Forbid,
    pub name: String,
    /// (scene 2) the one net the area does not keep out (0: it keeps out every net)
    pub net: u16,
}

/// A piece of a path: a line (x0, y0, x1, y1) or an arc (start, a point on it, end).
#[derive(Clone, Copy, Debug)]
pub enum PathPiece {
    Line([f64; 4]),
    Arc([f64; 6]),
}

/// (scene 2) A track already on the board.
#[derive(Clone)]
pub struct OldTrack {
    pub net: u16,
    pub layer: usize,
    pub width: f64,
    pub path: Vec<PathPiece>,
    pub locked: bool,
}

/// (scene 2) A via already on the board. `span`: its first and last copper layer when it is not through.
#[derive(Clone)]
pub struct OldVia {
    pub net: u16,
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub drill: f64,
    pub span: Option<(usize, usize)>,
    pub locked: bool,
}

#[derive(Clone)]
pub struct Island {
    pub outline: Vec<(f64, f64)>,
    pub holes: Vec<Vec<(f64, f64)>>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ZonePads {
    Solid,
    Thermal,
    ThermalTht,
    /// the zone joins no pad by itself
    None,
}

/// (scene 2) A zone already on the board, on one layer, with its fill as KiCad last made it.
#[derive(Clone)]
pub struct Zone {
    pub net: u16,
    pub layer: usize,
    pub outline: Vec<(f64, f64)>,
    pub holes: Vec<Vec<(f64, f64)>>,
    pub fill: Vec<Island>,
    pub priority: i32,
    pub clearance: Option<f64>,
    pub min_width: Option<f64>,
    pub pads: ZonePads,
    /// the gap a thermal relief leaves round a pad, and the width of its spokes
    pub thermal_gap: Option<f64>,
    pub spoke_width: Option<f64>,
    pub locked: bool,
}

#[derive(Clone, Default)]
pub struct Copper {
    pub tracks: Vec<OldTrack>,
    pub vias: Vec<OldVia>,
    pub zones: Vec<Zone>,
}

/// (scene 2) A part: its courtyard, its side, whether it may be touched.
#[derive(Clone)]
pub struct Part {
    pub ref_: String,
    pub courtyard: Vec<(f64, f64)>,
    pub back: bool,
    pub fixed: bool,
    pub in_scope: bool,
}

/// (scene 2) A differential pair.
#[derive(Clone)]
pub struct Pair {
    pub name: String,
    pub p: u16,
    pub n: u16,
    pub width: Option<f64>,
    pub gap: Option<f64>,
    pub impedance: Option<Impedance>,
    pub max_skew: Option<f64>,
    pub max_uncoupled: Option<f64>,
}

/// (scene 2) Nets to be made the same length (or the same delay) within `tolerance`.
#[derive(Clone)]
pub struct MatchGroup {
    pub name: String,
    pub nets: Vec<u16>,
    pub tolerance: f64,
    /// the length to reach, mm; None: the longest net's
    pub target: Option<f64>,
    pub by_time: bool,
}

/// (scene 2) Parts that belong together (a chip and its capacitors).
#[derive(Clone)]
pub struct Group {
    pub name: String,
    pub parts: Vec<usize>,
}

#[derive(Clone)]
pub struct Scene {
    /// the format it was read from: 1 or 2
    pub version: u32,
    pub layers: Vec<String>,
    pub rules: Rules,
    /// nets[0] is "no net"; a pad's `net` indexes this.
    pub nets: Vec<Net>,
    pub pads: Vec<Pad>,
    pub holes: Vec<(f64, f64, f64)>,
    /// Edge.Cuts as straight segments (arcs flattened).
    pub outline: Vec<(f64, f64, f64, f64)>,
    pub keepouts: Vec<Keepout>,
    /// (1.1) how many pieces of copper the scene's `copper` lists
    pub existing_copper: usize,
    /// (scene 2) from here on
    pub stackup: Vec<StackLayer>,
    pub classes: Vec<NetClass>,
    pub class_pairs: Vec<ClassPair>,
    pub copper: Copper,
    pub parts: Vec<Part>,
    pub pairs: Vec<Pair>,
    pub matches: Vec<MatchGroup>,
    pub groups: Vec<Group>,
}

impl Scene {
    /// What the scene carries that the router does not use yet, one line a thing. A track that starts
    /// using one takes its line out.
    pub fn carried_unused(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut say = |n: usize, what: &str| {
            if n > 0 {
                out.push(format!("{} {}", n, what));
            }
        };
        if self.version < 2 {
            say(self.existing_copper, "pieces of copper already on the board");
            return out;
        }
        // (a class's track width and clearance are its nets'; its via is not used: every net gets the board's)
        say(self.classes.iter().filter(|c| c.via.is_some() || c.drill.is_some()).count(), "net classes with a via size of their own");
        say(self.rules.layer_rules.iter().chain(self.classes.iter().flat_map(|c| c.layer_rules.iter())).filter(|r| r.track.is_some()).count(), "track widths for one layer");
        let f = &self.rules.floors;
        // (the floors of track, via, drill and annular ring are used: a neck, the small via)
        say([f.clearance, f.hole_to_hole, f.edge].iter().filter(|v| v.is_some()).count(), "fab floors (of clearance, hole to hole and edge)");
        say(self.stackup.len(), "stackup layers (thickness, dielectric)");
        say(self.nets.iter().filter(|n| n.via.is_some() || n.drill.is_some()).count(), "nets with a via size of their own");
        say(self.nets.iter().filter(|n| n.impedance.is_some()).count() + self.classes.iter().filter(|c| c.impedance.is_some()).count() + self.pairs.iter().filter(|p| p.impedance.is_some()).count(), "impedance targets");
        say(self.pairs.len(), "differential pairs");
        say(self.matches.len(), "match groups");
        say(self.groups.len(), "groups of parts");
        say(self.parts.iter().filter(|p| !p.in_scope).count(), "parts out of scope");
        out
    }
}
