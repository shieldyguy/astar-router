# T7 Impedance

**Wave 2. Small.** Mission: a net asked to be 50 ohm, or a pair asked to be 90, gets the width and gap its
layer needs.

Owns: `src/rules/impedance.rs`, the stackup part of the scene. Needs: sizes by layer (T2.5). Gives: widths
and gaps to T8, delay per mm to T9.
Reference: KRT `py_router/impedance.py`, `docs/api-impedance.md`, `py_tools/check_impedance.py`.

### T7.1 The stackup
Build: layers with thickness, dielectric constant and copper weight in the scene; the reference plane of
each signal layer.
Done when: read from the corpus boards that declare one; a board with none says so.

### T7.2 The formulas
Build: microstrip, stripline, coplanar over ground, and their pair forms; delay per mm.
Done when: within 1% of KRT's `impedance.py` on a table of 200 cases.

### T7.3 Width for a target
Build: target in, width per layer out (and gap for a pair), held at the fab floor with a narrowing entry
when the floor wins.
Done when: fixtures on a 4-layer and a 6-layer stack give KRT's widths within 5 um.

### T7.4 Width by layer in the route
Build: a net's width follows the layer it is on; the change happens at the via.
Done when: a 50 ohm net crossing from outer to inner layer is the right width on each, DRC clean.

### T7.5 Kept and checked
Build: a net's impedance declaration survives a re-route; `astar check --impedance` gives the achieved
value per stretch.
Done when: KRT's `check_impedance.py` agrees on the fixtures.
