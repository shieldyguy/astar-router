# T4 Egress plan

**Wave 1. This track goes past KRT.** Mission: before any copper is laid, know which pads must leave which
parts, through which gaps, toward where and in what order; reserve those ways out; and say so when there
are not enough of them.

Why: routers lay copper in some order and find out late that an early track or via sealed a pad's only
way out. KRT does this part by part. astar today uses rules of thumb (fences, tolled lanes, mouths: see
`../README.md`). The board holds everything needed to do it properly: the pads, where each net goes, and
the width of every gap. Chris, 2026-11-16: "that's just information it has".

Owns: `src/passes/egress*.rs`, the egress block of the report. Needs: passes, cost terms, job order (T0).
Gives: exits to T5 (BGA, QFN), order and reservations to the route, deficits to T12, corridors to T10.
Works on the five bench scenes from day one; needs no board reader.
Prior art to read first: topo-router `NOTES.md` (escapes, fan lanes), the Field router's `egress` keys in
router-bench plans, astar's `fences`, `lane` and `mouth` code.

### T4.1 Demand
Build: for every pad that has to be joined: its kind (traveller, local hop to a near part, plane drop, pair
member), the bearing and distance of where it is going, the layers it could use. Grouped by part and side.
Done when: `astar egress SCENE` prints demand per part side for the bench five in under 0.2 s.

### T4.2 Gaps and their capacity
Build: per layer, the free space as regions joined by gaps. A gap is where the clearance field has a saddle
between two pieces of fixed copper; its capacity is how many tracks of a given width fit through it at the
rules. A region's via capacity is how many legal via sites it holds.
Done when: on hand-built fixtures (pad rows at 0.4, 0.5, 0.65, 0.8, 1.0 mm pitch; a BGA quadrant) every
capacity equals the hand count.

### T4.3 Exit candidates
Build: for each pad, its real ways out, found by short searches in the exact field: straight out, the two
45s, inward to a via, a via beside the pad. Each candidate knows the gaps and via sites it uses and its cost.
Done when: every pad of the bench five has its candidates listed; a pad with none is named.

### T4.4 Assignment
Build: one exit per pad such that no gap or via site is over capacity, exits of neighbours do not cross on a
layer, and bearing mismatch and length are least. Min-cost flow per part; parts that share gaps settled by
price iteration between them. The same input gives the same answer.
Done when: on the capacity fixtures the assignment is feasible whenever one exists (checked by brute force
on the small ones); every pin of dock's U1 gets an exit and no two cross.

### T4.5 Deficits
Build: where demand exceeds capacity, say it: part, side, layer, need, have. Offer remedies in order: another
layer, a via inside or under the part, a neck to the floor where the rules allow, a part to move and which way.
`astar egress` exits 1 on a deficit, before a second of routing is spent.
Done when: a fixture with one track too many reports exactly that, with the part, the side and the count.

### T4.6 Reservations
Build: assigned exits held in the field as a cost term. Another net pays to use one, by how scarce that
pad's other exits are: free when it has many, forbidden when it is the last. No foreign via in a reserved
mouth. A reservation ends when its owner is routed, and is re-planned if the owner went another way.
Done when: the fixture where a neighbour's via seals a pin routes clean with reservations and fails without.

### T4.7 Order
Build: the route's order from slack: the pad whose tightest gap has the least room goes first; plane drops
in tight pockets before drops in open ones; nets that only pass a part go after the nets that leave it.
Done when: measured on the corpus (T4.11): fewer re-routes, no board worse.

### T4.8 Corridors
Build: past the part, a coarse route for each traveller on the region graph, aware of how full each gap is.
It becomes a cost term, so nets that go the same way stay together and none cuts across another part's exits.
Done when: tb150 and ub400 show fewer crossings of reserved exits and no more track than before.

### T4.9 Check and learn
Build: every assigned exit proved by a real search in the exact field. A failure lowers that gap's capacity
and the assignment is done again, three rounds at most.
Done when: after the loop no assigned exit fails its search on the corpus.

### T4.10 The picture and the report
Build: per part, a drawing of its exits, reservations and deficits; in the report, per part side: demand,
capacity, assigned, short by; per net: its exit.
Done when: Chris can see at a glance on dock why U1's north side is tight.

### T4.11 Proof it helps
Build: the corpus routed with the plan and without: negotiation rounds, re-routes, open nets, vias, seconds.
Done when: rounds and re-routes fall, no board lands fewer nets, and dock lands 28 of 28 at 0.05, 0.1 and
0.2 mm grid (today 27 of 28 at 0.05 and 0.2).
