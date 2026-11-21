# T2 Rules

**Wave 1.** Mission: one answer to "what must hold between these two things here", used by the field, the
exact index and the checker alike.

Owns: `src/rules/`. Needs: scene 2. Gives: the resolver to every track; the narrowing ladder to T6, T12.
Reference: KRT's CLAUDE.md section on net classes, `docs/design-rules-proposal.md`, `fab_tiers.py`,
`docs/configuration.md` (Geometry, Fab Tier). House rulings: `~/tools/pcb/docs/routing.md` (sizes come from
the circuit file's class lines; JLC floors; the house via).

### T2.1 The resolver
Build: `clearance(a, b, layer)`: the larger of the two classes; a layer rule replaces it; a pad override
raises it; net-tie pads exempt where they touch. Same for width and via by class and layer.
Done when: equal to KRT's `check_drc` pair clearance on the T1.4 samples.

### T2.2 The field by class and layer
Build: thresholds per class and layer in the field, so a wide-clearance class and a tight inner layer both
hold without a second field.
Done when: a fixture with a 0.5 mm class beside a 0.127 mm class, and one with a tighter inner layer, pass
KiCad DRC; bench times within 5%.

### T2.3 Holes (parallel)
Build: hole to hole for vias and pad holes, same net and different; copper to hole; slots; unplated holes.
Done when: must-pass and must-fail fixtures agree with KiCad DRC.

### T2.4 The edge (parallel)
Build: edge clearance from the board; cut-outs; castellated pads on the edge, and a track landing on one
pulled back to the pad's inner reach.
Done when: fixtures agree with KiCad DRC; `watchy` and a castellated module route.

### T2.5 Sizes by class (parallel)
Build: track width and via size per class; a width table per layer for T7; the minimums.
Done when: a three-class fixture routes each net at its class size.

### T2.6 Fab floors and the ladder
Build: floors from a file (`fab-overrides.txt` as the house writes it) and named tiers; the policies KRT
calls off, board and fab; every narrowing recorded with net, place, asked and given; `--strict` exits 3.
Done when: a fixture that cannot route at its class sizes routes narrowed under `board` and reports each
narrowing; fails cleanly under `off`.

### T2.7 Keep-outs (parallel)
Build: rule areas by what they forbid (tracks, vias, pours) per layer; keep-outs drawn on a user layer;
no foreign via under an area-array part.
Done when: fixtures agree with KiCad DRC.

### T2.8 Vias and pads
Build: no via in a surface pad unless the class or a flag allows it; the same-net gap between a via and its
pad; a via under solder marked so the writer can declare it filled and capped.
Done when: fixtures pass; the count of vias in pads is in the report.

### T2.9 Rule fixtures
Build: one tiny board per rule with a geometry that must pass and one that must fail, judged by KiCad.
Done when: every rule above has its pair and both verdicts match.
