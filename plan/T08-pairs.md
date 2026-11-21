# T8 Pairs

**Wave 3. Needs T7 (width and gap), T4 and T5.8 (leaving a part together), T3.3 (protection).**
Mission: a differential pair routed as a pair.

Owns: `src/passes/pairs*.rs`, the pair job kind. Gives: protected pairs to T9.
Reference: KRT `docs/differential-pairs.md` (711 lines), `diff_pair_*.py`; the house chain's step 10 (a pair
that falls back to single tracks is a placement stop). Boards: lvds_converter_dualclk, the `qfn_*diff*`
set, dock's USB pair, the house `usb_beside` fixture.
The spike T8.0 (wave 1, a note only): dock's USB pair routed as one wide virtual track, then split.

### T8.1 Which nets are pairs
Build: from names (P/N, +/-), from the class's pair width and gap, from an explicit list; how much
uncoupled length a pair may have.
Done when: every pair KRT's name rules find on the corpus is found.

### T8.2 The coupled search
Build: the pair searched as one track as wide as both members and their gap, with turns limited so both
members fit; then split into the two members by offset.
Done when: straight and bent fixtures hold the gap within 2 um along the coupled run, KiCad DRC clean.

### T8.3 The ends
Build: from two pads to the coupled run by a short symmetric fan; bare pads, connector pads, pads on
different layers. Uncoupled length is kept short and reported.
Done when: fixtures for each end type; uncoupled length under the declared limit.

### T8.4 Pair vias
Build: the two vias and their ground return vias placed as one pattern on legal sites.
Done when: a layer-changing pair fixture passes DRC with its return vias present.

### T8.5 Pairs in the negotiation
Build: a pair is a job like any other in the clash loop; pairs go first and are protected after.
Done when: lvds_converter lands all pairs with the rest of the board routed round them.

### T8.6 Polarity
Build: when the members arrive crossed, uncross at a via, or swap pins where the scene says they are
swappable, and report it.
Done when: a crossed fixture routes without a single-ended fallback.

### T8.7 Skew
Build: small bumps on the shorter member near the end that caused the mismatch, to a tolerance.
Done when: fixture pairs are within tolerance by the independent checker.

### T8.8 Pairs through parts, and pairs with three ends
Build: a pair through series caps, ESD parts or a choke; a pair with three or more terminals. Where a
stretch cannot be coupled it is routed single with a named reason.
Done when: a fixture of a USB pair through an ESD part and a three-terminal fixture pass.

### T8.9 The finish and the report
Build: arcs concentric for both members; the gap checked exactly along the whole run; per pair: coupled
share, gap least and most, length of each member.
Done when: on the corpus, coupled share is at or above KRT's for every pair both route.
