//! What the router may be told: the grid, the prices, the limits.

#[derive(Clone)]
pub struct Cfg {
    /// grid pitch, mm
    pub pitch: f64,
    /// the heuristic's weight: 1 is exact A*, more is faster and a little less tidy
    pub weight: f64,
    /// the weight once the search may cross other nets' copper
    pub weight_soft: f64,
    /// what a via costs, in mm of track
    pub via_mm: f64,
    /// what the via of a plane drop costs, in mm of track (a second pad shares a via nearer than this)
    pub plane_via_mm: f64,
    /// what a 45 degree bend costs, in mm of track
    pub turn_mm: f64,
    /// how much dearer a track is on an inner layer
    pub inner_mul: f64,
    /// what starting a narrowed stretch of a wide net costs, in mm of track, and how much dearer each
    /// step of it is: a net is drawn narrower than it asks only where the way round is dearer than this
    pub neck_mm: f64,
    pub neck_mul: u32,
    /// what leaving a pad by its side costs, in mm of track (`passes::exits`)
    pub exit_mm: f64,
    /// what a step across another net's pour costs, in steps on top of its own (`passes::cross`); 0: nothing
    pub cross_mul: f64,
    /// nets that are joined off this board: each of their pads gets a via and nothing else (`--drops`)
    pub drops: Vec<String>,
    /// what crossing another net's copper (which is then lifted and re-routed) costs, in mm of track
    pub rip_mm: f64,
    /// the same for a plane drop looking for its via
    pub rip_drop_mm: f64,
    /// how far a fence runs out of a narrow pad past its end, mm
    pub fence_mm: f64,
    /// how far the fan lane in front of a fence reaches, mm, and what another net pays per mm inside it
    pub lane_mm: f64,
    pub lane_toll: f64,
    /// what a via costs more, in mm of track, where its own lane's mouth overlaps a neighbour's
    pub mouth_mm: f64,
    /// a pad at least this big both ways may take a via of its own net when it cannot get out otherwise, mm
    pub vip_mm: f64,
    /// pour the rail with the most pads over a free inner layer when it has at least this many (0: never)
    pub pour_pads: usize,
    /// how many rounds of routing clashing nets again, and how much dearer a clash gets each round
    pub rounds: u32,
    pub pressure: f64,
    /// stop starting new work after this many seconds (0: never)
    pub budget_s: f64,
    /// the radius of the arcs the corners are turned into at the end, mm (0: corners stay sharp)
    pub radius_mm: f64,
    /// route the board with every board pass's change and without, and keep the better (false: only with)
    pub two_ways: bool,
    /// passes left out, by name (`passes::PASSES`)
    pub skip: Vec<String>,
    /// the largest grid the router will build: nodes times layers (12 bytes each, and as much again for the search)
    pub max_cells: usize,
    pub verbose: bool,
}

impl Default for Cfg {
    fn default() -> Cfg {
        Cfg {
            pitch: 0.1,
            weight: 2.0,
            weight_soft: 4.0,
            via_mm: 5.0,
            plane_via_mm: 1.5,
            turn_mm: 0.2,
            inner_mul: 2.0,
            neck_mm: 4.0,
            neck_mul: 4,
            exit_mm: 2.0,
            cross_mul: 2.0,
            drops: Vec::new(),
            rip_mm: 8.0,
            rip_drop_mm: 3.0,
            fence_mm: 0.35,
            lane_mm: 1.2,
            lane_toll: 15.0,
            mouth_mm: 6.0,
            vip_mm: 1.5,
            pour_pads: 5,
            rounds: 40,
            pressure: 1.4,
            budget_s: 0.0,
            radius_mm: 1.0,
            two_ways: true,
            skip: Vec::new(),
            max_cells: 120_000_000,
            verbose: false,
        }
    }
}

impl Cfg {
    /// Are these settings the router can run with? The first thing wrong is the error.
    pub fn check(&self) -> Result<(), String> {
        if !(0.01..=1.0).contains(&self.pitch) {
            return Err(format!("the pitch is {} mm: it must be 0.01 to 1", self.pitch));
        }
        // a price is at most ten metres of track: past that the search's sums no longer hold
        for (name, v) in [
            ("--weight", self.weight),
            ("--weight-soft", self.weight_soft),
            ("--via", self.via_mm),
            ("--plane-via", self.plane_via_mm),
            ("--turn", self.turn_mm),
            ("the neck price", self.neck_mm),
            ("the exit price", self.exit_mm),
            ("--cross", self.cross_mul),
            ("--inner", self.inner_mul),
            ("--rip", self.rip_mm),
            ("--rip-drop", self.rip_drop_mm),
            ("--fence", self.fence_mm),
            ("--lane", self.lane_mm),
            ("--toll", self.lane_toll),
            ("the mouth price", self.mouth_mm),
            ("the via-in-pad size", self.vip_mm),
            ("--pressure", self.pressure),
            ("--radius", self.radius_mm),
            ("--seconds", self.budget_s),
        ] {
            let top = match name {
                "--weight" | "--weight-soft" | "--inner" | "--pressure" | "--radius" | "--lane" | "--fence" => 100.0,
                "--seconds" => 1e9,
                _ => 10_000.0,
            };
            if !(0.0..=top).contains(&v) {
                return Err(format!("{} is {}: it must be 0 to {}", name, v, top));
            }
        }
        if self.inner_mul <= 0.0 || self.pressure <= 0.0 {
            return Err("--inner and --pressure must be more than 0".into());
        }
        if let Some(s) = self.skip.iter().find(|s| crate::passes::index(s).is_none()) {
            return Err(format!("no pass is called {}: the passes are {}", s, crate::passes::PASSES.iter().map(|p| p.name).collect::<Vec<_>>().join(", ")));
        }
        Ok(())
    }
}
