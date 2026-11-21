//! What the scene's rules ask between two things: the check's own reading of docs/scene.md and of
//! router-bench's CONTRACT.md, sharing nothing with the router's.

use crate::scene::Scene;

pub struct Rules<'a> {
    sc: &'a Scene,
}

impl<'a> Rules<'a> {
    pub fn new(sc: &'a Scene) -> Rules<'a> {
        Rules { sc }
    }

    /// Has the net a clearance of its own (scene 1.1's and 2's `clearance` on a net).
    fn has_own(&self, net: u16) -> bool {
        net != 0 && self.sc.nets[net as usize].clearance > 0.0
    }

    /// The clearance one net asks for on one layer: its own, else its class's rule for that layer, else
    /// its class's, else the Default's.
    fn of_net(&self, net: u16, layer: usize) -> f64 {
        let n = &self.sc.nets[net as usize];
        if self.has_own(net) {
            return n.clearance;
        }
        if let Some(c) = self.sc.classes.get(n.netclass) {
            if let Some(v) = c.layer_rules.iter().find(|r| r.layer == layer).and_then(|r| r.clearance) {
                return v;
            }
            if let Some(v) = c.clearance {
                return v;
            }
        }
        self.sc.rules.clearance
    }

    /// Between two nets on one layer: a rule for that layer replaces everything; else the larger of the
    /// two nets' own, unless `class_pairs` names their two classes and neither net has a clearance of
    /// its own.
    fn on_layer(&self, a: u16, b: u16, layer: usize) -> f64 {
        let sc = self.sc;
        if let Some(v) = sc.rules.layer_rules.iter().find(|r| r.layer == layer).and_then(|r| r.clearance) {
            return v;
        }
        if !self.has_own(a) && !self.has_own(b) {
            let (ca, cb) = (sc.nets[a as usize].netclass, sc.nets[b as usize].netclass);
            if let Some(p) = sc.class_pairs.iter().find(|p| (p.a == ca && p.b == cb) || (p.a == cb && p.b == ca)) {
                return p.clearance;
            }
        }
        self.of_net(a, layer).max(self.of_net(b, layer))
    }

    /// Between copper of two nets that share the layers of `lmask`: the most any of those layers asks.
    pub fn between(&self, a: u16, b: u16, lmask: u32) -> f64 {
        let mut most = 0.0f64;
        for l in 0..self.sc.layers.len() {
            if lmask & (1 << l) != 0 {
                most = most.max(self.on_layer(a, b, l));
            }
        }
        most
    }

    /// A via's size and drill when the plan does not say: the net's own, else its class's, else the rules'.
    pub fn via_of(&self, net: u16) -> (f64, f64) {
        let n = &self.sc.nets[net as usize];
        let c = self.sc.classes.get(n.netclass);
        (n.via.or(c.and_then(|c| c.via)).unwrap_or(self.sc.rules.via), n.drill.or(c.and_then(|c| c.drill)).unwrap_or(self.sc.rules.drill))
    }

    /// The farthest any rule reaches: no two things farther apart than this can break one.
    pub fn reach(&self) -> f64 {
        let sc = self.sc;
        let r = &sc.rules;
        let mut most = r.clearance.max(r.edge).max(r.hole_clearance).max(r.hole_to_hole.unwrap_or(0.0));
        for v in r.layer_rules.iter().filter_map(|x| x.clearance) {
            most = most.max(v);
        }
        for c in &sc.classes {
            most = most.max(c.clearance.unwrap_or(0.0));
            for v in c.layer_rules.iter().filter_map(|x| x.clearance) {
                most = most.max(v);
            }
        }
        for p in &sc.class_pairs {
            most = most.max(p.clearance);
        }
        for n in &sc.nets {
            most = most.max(n.clearance);
        }
        for p in &sc.pads {
            most = most.max(p.clearance);
        }
        most
    }
}
