# T3 Existing copper and scope

**Wave 1.** Mission: astar works on a board that already has copper, and on part of a board.

Owns: `src/route/` after G0. Needs: copper kinds (T0.5), scene 2 copper (T0.2; real boards from T1.5).
Gives: scope and protection to T8, T9, T14. Starts on hand-written scenes; does not wait for T1.
Reference: KRT `docs/rip-up-reroute.md` (ripping pre-existing routes, the improvement gate), CLAUDE.md on
protected nets and locked copper, `docs/configuration.md` (Net Selection, Forcing a Re-Route);
`~/tools/pcb/docs/routing.md` ("A small change on a routed board").

### T3.1 Copper that stays
Build: existing tracks, arcs and vias stamped as fixed copper of their net. A net's tree starts from its
pads and the islands of copper it already has; the router joins islands.
Done when: a half-routed fixture is finished without moving a piece of what was there.

### T3.2 Zones as copper
Build: a filled zone of the net counts as joined where its fill reaches; without a fill, where a plane is
declared. Other nets keep the zone's clearance from nothing: they may cross a pour (it refills), never a plane.
Done when: fixtures for a pad joined through a pour and a track crossing a pour pass KiCad after refill.

### T3.3 Who may be ripped
Build: nothing existing is lifted by default. `--rip PATTERNS` allows it. Locked copper never. Protected
nets (routed pairs, matched groups) only when named exactly.
Done when: fixtures for each of the four cases; the allwinner DDR case from KRT's notes cannot recur.

### T3.4 Scope (parallel)
Build: `--nets` with patterns and `!`, `--component REF...`, `--group` (a KiCad group or a sheet). Everything
out of scope is fixed.
Done when: routing one part of tb150 leaves every other net's copper byte for byte.

### T3.5 Preview, undo, force (parallel)
Build: `--preview` writes no board and reports what would be added; `--undo` strips the scope's routed
copper; `--force NETS` replaces their routes.
Done when: preview then route equals route; route then undo equals the input.

### T3.6 The small change
Build: re-route one net, or the nets of a part that moved, on a finished board.
Done when: on the house fixture `pots.routed` one net is re-routed, all else untouched, under 0.5 s.

### T3.7 Dirty input
Build: stacked vias, zero-length tracks, overlapping same-net copper, tracks ending in air: taken as they
are, counted in the report, never a crash.
Done when: a fixture with each passes.

### T3.8 Custody
Build: every piece of copper in the output says where it came from (the input, a pass, a job). A check that
nothing from the input is missing unless a rule allowed it.
Done when: the check runs in the gate on every fixture.
