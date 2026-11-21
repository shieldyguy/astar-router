# T10 Looks

**Wave 2.** Mission: copper that looks like a person laid it out, measured as well as eyed. Chris finds
KRT's tracks messy and topo's chaotic; this is where astar earns its keep.

Owns: `src/finish/`. Needs: cost terms (T0.4); corridors from T4.8 help but are not required.
Reference: KRT `docs/bus-routing.md`, `pack_river.py`, `round_corners.py`, `loose_ends.py`,
`check_weird.py`; the house `pcb/docs/style.md` and the copper look rules; step 13 (`pcb cleanup`).
The any-angle look is parked on branch `claude/astar-free-look`; it is not in this track unless Chris asks.

### T10.1 A looks score
Build: per board: bends per net, detour (length over the shortest octilinear length), staircases, vias,
the share of track that runs parallel to a neighbour at the least pitch, knees at pads. On the scoreboard
beside KRT's and topo's numbers.
Done when: the score ranks ten picture pairs the way Chris does.

### T10.2 Pad exits (parallel)
Build: a track leaves on the pad's centre line and turns once; no knee where a fence meets the grid; no
acute angle at a pad.
Done when: knees on the bench five fall to 0; DRC unchanged.

### T10.3 Bundles
Build: nets that go the same way are found (same part rows at both ends, or the same corridor) and routed
in order, side by side at the least pitch: an attraction cost to the neighbour already laid, then a pass
that packs the bundle.
Done when: the parallel share rises on tb150 and ub400 and no net is lost.

### T10.4 Pull tight, again, at the end
Build: once every net is in, each path is pulled once more against its final neighbours; a pair of vias is
removed where a path on one layer exists.
Done when: track and vias fall on the corpus, nothing else changes.

### T10.5 Cleanup (parallel)
Build: tracks and vias that lead nowhere, loops of one net, stacked copper, zero-length pieces, staircases.
Done when: `check_weird.py` and `check_orphan_stubs.py` are clean on the corpus; the house grade's
`cleanup:` line reads none.

### T10.6 Arcs, second pass
Build: bundles turn on concentric arcs; pairs too; a radius per class.
Done when: bundle spacing through a bend equals spacing on the straight within 2 um.

### T10.7 Teardrops (parallel)
Build: the board's teardrop settings written for pads and vias, as KRT's `--add-teardrops` does.
Done when: KiCad shows them and DRC is unchanged.

### T10.8 Corridors by hand
Build: a path drawn on a user layer that named nets prefer, and a spacing along it.
Done when: KRT's guide-corridor fixture is followed.

### T10.9 Via looks
Build: vias of a fan in a regular row; no via nearer a pad than it needs when a tidier site is free.
Done when: the looks score counts it and it improves on dock and ub400.
