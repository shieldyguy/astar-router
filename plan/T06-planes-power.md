# T6 Planes and power

**Wave 2. Needs rules (T2) and existing zones (T3.2).** Mission: every way KRT and the house chains get
power and ground onto a board, and a guarantee that planes stay whole.

Owns: `src/passes/planes_*.rs`, `drops.rs`, `pours.rs`. Gives: plane facts to T5, T8; rail report to T12.
Reference: KRT `docs/route-plane.md` (871 lines), `docs/power-nets.md`, `rail_pours.py`, `plane_drops.py`,
`repair_planes.py`, CLAUDE.md on pour-served pads; the house rulings in `~/tools/pcb/docs/routing.md`:
ground is never routed on four layers; a rail with a plane reaches it by a via at each pad, never by track;
two layers route ground first, on the bottom, pours last.

### T6.1 What a plane is
Build: which net owns copper on which layer, from zones on the board and pours astar plans; an estimate of
the fill in the field good enough to see islands; KiCad's refill as the oracle for the final answer.
Done when: the estimate agrees with KiCad's fill on which pads a plane reaches, on the corpus's plane boards.

### T6.2 Several rails on one layer
Build: each rail a region around its own pads on a free inner layer; a region serving one pad is dropped.
Par with krt-better's `rail_pours`, which pours twelve rails on tb150 where astar pours one.
Done when: tb150, ccb and g474 pour every rail krt-better does and track length falls to its level.

### T6.3 Outer pours and two-layer ground
Build: a ground pour on the outer layers after the route; on two layers the house way: a wide ground tree
on the bottom first, pours last.
Done when: the house two-layer fixtures pass step 15's grade.

### T6.4 Drops, generalised (parallel)
Build: today's drops for any plane net and any layer count; a drop refused for want of a via site is a
finding with the pad and the distance tried.
Done when: every surface pad of a plane net on the corpus has a via or a finding.

### T6.5 Planes stay whole
Build: a price on vias and tracks that would cut a plane into pieces or choke it under a dense part; an
audit after the route that every plane pad is reached by fill; a repair that moves a via or adds a strap.
Done when: no plane on the corpus is in more pieces after the route than before; KiCad agrees.

### T6.6 Spokes (parallel)
Build: the zone's connection style written per pad class; no pad left with too few spokes.
Done when: KiCad's thermal-relief check is 0 on the corpus.

### T6.7 Stitching (parallel)
Build: area stitching on a pitch, a fence along the edge, a ground via beside each signal via that changes
reference plane. All obey the rules; counts in the report.
Done when: DRC clean on the stitching and return-via fixtures; counts shown beside KRT's (`--stitch-vias`,
`--add-gnd-vias`) on the same boards.

### T6.8 Wide rails
Build: width from the class; a neck only at a pad or a fine-pitch part, no longer than a set length, with a
taper; every neck in the report in mm; a rail under its declared minimum flagged.
Done when: ub400's rails report each neck (KRT flags +24VIN at 0.2 mm there; astar says nothing today).

### T6.9 Repair
Build: after the final fill, each island of a zone and each plane pad KiCad lists as unconnected gets a
join or a tap.
Done when: KiCad's unconnected count for plane nets is 0 on the corpus.

### T6.10 The power report
Build: per rail: pads served by plane and by track, narrowest width, vias.
Done when: it is in the report and T14's chain reads its `planes:` facts from it.
