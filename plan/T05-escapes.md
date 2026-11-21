# T5 Escapes

**Wave 2. Needs the egress plan (T4.1 to T4.6).** Mission: every package gets out: area arrays, fine-pitch
perimeter parts, exposed pads. An escape here is how an exit the egress plan assigned is drawn.

Owns: `src/passes/escape_*.rs`. Needs: T4, rules for vias in pads (T2.8). Gives: pair-aware escapes to T8.
Reference: KRT `py_router/bga_fanout/` and `qfn_fanout/` with their READMEs, `escape_fences.py`; the house
chain's steps 4 and 5. Boards: KRT `kicad_files/` glasgow_revC, orangecrab, ulx3s, kit-dev-coldfire,
rp2350, tigard, the `qfn_*` set; dock.
The spike T5.0 (wave 1, a note only): one quadrant of the ulx3s BGA escaped by hand-written rules, to
learn what T4 must hand over.

### T5.1 What kind of part
Build: area array, perimeter with or without a centre pad, pin rows, from the pad pattern and not the
name: pitch, rows, depth, rotation.
Done when: every part of the corpus over 16 pads is classed and a sample of 100 is checked by eye.

### T5.2 Perimeter parts, hardened
Build: today's fences on any rotation, either side, 0.35 to 0.65 mm pitch; inward exits to a via under the body.
Done when: KRT's `qfn_*` boards and dock land at every grid from 0.05 to 0.2 mm.

### T5.3 Area arrays: the outer rows
Build: balls that can leave on the surface between their neighbours do, as many per channel as the rules fit.
Done when: the hand count per channel is met on a 0.8 and a 1.0 mm fixture.

### T5.4 Area arrays: the inner balls
Build: a dog-bone via per ball; each ring of balls on its own layer so channels never collide; a via in the
ball's pad where no dog-bone fits and the rules allow it. Through vias block every layer and are counted so.
Done when: ulx3s and glasgow escape every signal ball; KiCad DRC clean under the parts.

### T5.5 Area arrays: power and ground balls
Build: plane balls dropped to their planes first, in the pattern that leaves the most channels for signals.
Done when: no plane ball takes a channel; the plane under the part is still one piece (T6.5 checks).

### T5.6 Exposed pads
Build: thermal vias in a centre pad by count and pitch, tied to the plane, clear of the paste where asked.
Done when: the house chain's step 4 cases pass `pcb drc`'s thermal line.

### T5.7 Parts on the far side
Build: decoupling caps under an array reached from the dog-bone vias without closing a channel. A cap that
sits on a needed via site is a placement finding; astar does not move it.
Done when: orangecrab's caps join; a blocked site is reported with the part's name.

### T5.8 Pairs leave together
Build: with T8: the two balls or pins of a pair leave on one layer in neighbouring channels.
Done when: `qfn_diffpair_escape`, `qfn_csi_underpad_diff` and lvds_converter fixtures escape coupled.

### T5.9 Swaps, off by default
Build: where the scene declares pins or targets swappable, pick the assignment with the fewest crossings and
report each swap. astar edits no schematic.
Done when: a fixture with a crossed bus routes straight after the swap and the report lists it.
