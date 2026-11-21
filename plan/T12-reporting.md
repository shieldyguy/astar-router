# T12 Reporting

**Wave 1.** Mission: when astar cannot do something it says what, where, why and what to change. The
house rule is that a failed net goes back to placement; the report has to make that step short.

Owns: `src/report/`. Needs: report entries (T0). Gives: findings to T14's chains.
Reference: krt-better's `route_board.py` report (open nets with blockers, narrowed, rails below floor,
seconds per step), KRT `py_tools/check_reachability.py`, `docs/rip-up-reroute.md` (diagnostics); the house
chain's `placement:` lines and stops.

### T12.1 The report
Build: one JSON beside the board: nets landed and open; per open net the pads joined, what stood in the
way (nets and fixed things on the last search's frontier) and a class of reason.
Done when: every open net on the corpus has a reason; the schema is in `docs/report.md`.

### T12.2 Can this pad be reached
Build: `astar explain BOARD --pad U4.26`: a flood from the pad at the least width: caged, a throat at the
floor, or open; with the width that would pass.
Done when: it agrees with KRT's `check_reachability.py` on its fixtures.

### T12.3 What to move
Build: for an open net, a refused drop or an egress deficit: the part to move and which way, as the
`placement:` lines the house chain expects.
Done when: on three boards made unroutable by one bad part, the line names that part.

### T12.4 What was narrowed (parallel)
Build: every width, clearance or via under what was asked, from T2.6 and T6.8, in one list.
Done when: the list is empty exactly when nothing was narrowed; `--strict` exits 3 on any entry.

### T12.5 Where the time went (parallel)
Build: seconds, searches and steps per pass.
Done when: the sum is within 2% of the wall time.

### T12.6 Exit codes and capabilities (parallel)
Build: fixed exit codes; `astar --capabilities` as JSON, so a door can check what this build does.
Done when: T14's door refuses a build that lacks a feature it needs, by name.

### T12.7 Pictures (parallel)
Build: a picture per layer of the result without KiCad; optionally a frame per negotiation round.
Done when: a 60 px/mm picture of ub400 is written in under a second.

### T12.8 The run, recorded
Build: each run writes its exact command and the hash of its input, so it can be replayed with no agent.
Done when: replaying a recorded run gives the same board.
