# T9 Matching

**Wave 3. Needs T8 (pairs as units), T7 (delay).** Mission: groups of nets brought to the same length or
the same time.

Owns: `src/passes/match*.rs`. Reference: KRT `docs/length-matching.md`, `length_matching.py`.

### T9.1 Groups
Build: explicit groups with a tolerance and a target; groups found from names (DDR byte lanes: DQ with DQS).
Done when: KRT's auto-groups on its DDR corpus boards are reproduced.

### T9.2 What length is
Build: track and arcs, plus the via barrel between the layers used; as time, the delay of each layer.
Done when: equal to KRT's figures on routed corpus boards within 0.01 mm and 0.1 ps.

### T9.3 Room kept
Build: before a group is routed, estimate what each member must gain and price the space beside it so the
room is still there afterwards.
Done when: on the DDR fixture every member finds room for its meander without a re-route.

### T9.4 Meanders
Build: bumps added to straight runs, each checked exactly against all other copper, their size and spacing
from the rules, corners as arcs.
Done when: fixture groups are within tolerance, DRC clean, no bump closer to itself than three widths.

### T9.5 Pairs as one
Build: both members of a pair meander together; skew is settled after.
Done when: a matched pair group keeps its gap through every bump.

### T9.6 Kept
Build: a matched net is protected from later ripping (T3.3) and its group is recorded so a re-run keeps it.
Done when: a later step cannot unmatch a group without naming its nets.

### T9.7 Report and check
Build: per group: each length, the spread, the tolerance; the independent checker knows length and time.
Done when: on KRT's matched corpus boards astar's spread is within tolerance wherever KRT's is.
