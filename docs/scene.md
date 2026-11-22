# Scene 2

The scene is the one contract between reading a board and routing it: a JSON file that says everything
the router is told. `astar` reads two formats:

- **1.1**, router-bench's (`router-bench/CONTRACT.md`): read as it always was. What is missing takes a
  default; what is not known is passed over.
- **2**, this page: says `"version": 2`. It is strict. A key this page does not list is an error that
  names the key and where it is (`nets[3]: unknown key "colour"`); so is a value of the wrong kind, and a
  name (a layer, a net, a class, a part) that points nowhere. A reader meeting something it does not know
  says so and stops.

Both go through the same validator (`Scene::check`, below) before the router sees them.

The reader is `src/scene/read.rs` (1.1) and `src/scene/read2.rs` (2); the types are `src/scene/mod.rs`.
`tests/scenes/every_field.json` is a scene 2 with every key on this page; `tests/scene.rs` holds what is
promised here; `tests/fuzz.rs` bends both fixtures 10,000 ways and wants an error or a scene every time.

**Carried, not used.** Scene 2 adds what 1.1 has no place for. The reader carries all of it into the
types; the router uses a field only once the track that owns it does. Until then the run says what it was
given and did not use (`Scene::carried_unused`, printed as a note). The last column below says who.

## Conventions

Millimetres, KiCad board coordinates (x right, y down). A layer is named by its name in `stack.copper`.
A point is `[x, y]`; a polygon is a list of three or more points; a path is a list of
`{"line": [x0, y0, x1, y1]}` and `{"arc": [sx, sy, mx, my, ex, ey]}` (start, any point on the arc, end).
A net is named by its `name`; `""` is no net. An optional key left out takes the default given.

## The keys

| key | what | used by |
|---|---|---|
| `version` | `2` | reader |
| `units` | `"mm"` when given; nothing else is read | reader |
| `source`, `bench` | an object, free: where the scene came from. Not read. | nobody |
| `stack` | the layers (below) | router; T6, T7 |
| `outline` | Edge.Cuts as a path | router |
| `rules` | the board's rules (below) | router; T2 |
| `classes`, `class_pairs` | the net classes and the clearance between two of them (below) | T2 |
| `nets` | the nets (below) | router; T2, T7 |
| `pads` | the pads (below) | router |
| `holes` | `[[x, y, d]]`: non-plated holes | router |
| `keepouts` | areas and what they forbid (below) | router; T3 |
| `copper` | copper already on the board (below) | T3, T6 |
| `parts` | the parts (below) | T4 |
| `pairs` | differential pairs (below) | T8 |
| `match` | groups of nets to make equally long (below) | T9 |
| `groups` | groups of parts (below) | T4 |
| `scope` | which nets and parts are in scope (below) | T3 |

### `stack`

    {"copper": ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"],
     "planes": [{"net": "GND", "layer": "In1.Cu"}],
     "layers": [{"name": "F.Cu", "type": "copper", "thickness": 0.035},
                {"name": "prepreg 1", "type": "prepreg", "thickness": 0.2,
                 "epsilon_r": 4.4, "loss_tangent": 0.02, "material": "FR4"}, ...]}

- `copper`: the copper layers, top to bottom. Two or more, each name once.
- `planes`: a net that owns a layer as its plane. Such a net is joined by reaching the plane (a via into
  it), as 1.1 has it.
- `layers` (optional): the whole stackup, top to bottom, copper and what lies between. `type` is `copper`,
  `core`, `prepreg` or `mask`; `thickness` in mm; `epsilon_r`, `loss_tangent`, `material` optional. Its
  copper entries are `copper`, in order.

### `rules`

    {"track": 0.2, "clearance": 0.15, "via": 0.6, "drill": 0.3, "edge": 0.3, "hole_clearance": 0.25,
     "hole_to_hole": 0.25,
     "floors": {"track": 0.09, "clearance": 0.09, "via": 0.45, "drill": 0.2, "annular": 0.1,
                "hole_to_hole": 0.2, "edge": 0.2},
     "layers": [{"layer": "In2.Cu", "track": 0.15, "clearance": 0.2}]}

- The six of 1.1, all required: the Default class's track width, clearance, via size and drill, the
  copper-to-edge rule, the copper-to-hole rule.
- `hole_to_hole` (optional): hole edge to hole edge.
- `min_spokes` (optional): the fewest spokes a pour's thermal relief may be left with (default 2).
- `floors` (optional, each key optional): what the fab can make. Nothing is drawn or narrowed below a
  floor, whatever a class or a flag says.
- `layers` (optional): rules that hold on one layer only. A value here replaces the class's on that layer,
  larger or smaller (KiCad's custom rules outrank its classes; only a floor pins them up).

### `classes` and `class_pairs`

    "classes": [{"name": "Default", "track": 0.2, "clearance": 0.15, "via": 0.6, "drill": 0.3},
                {"name": "Power", "track": 0.4, "clearance": 0.2, "via": 0.8, "drill": 0.4,
                 "layers": [{"layer": "In2.Cu", "clearance": 0.25}]},
                {"name": "Pair90", "pair_width": 0.2, "pair_gap": 0.15,
                 "impedance": {"ohms": 90, "differential": true, "tolerance": 10}}],
    "class_pairs": [{"a": "Power", "b": "Default", "clearance": 0.25}]

- A class: `name` (once; `Default`, when given, is the first), and optionally `track`, `clearance`, `via`,
  `drill` (the via sizes of the class), `pair_width`, `pair_gap`, `impedance`, `layers` (as in `rules`).
  The class `Default` is there whether the scene lists it or not.
- Between two nets the larger of their two clearances holds, unless `class_pairs` names their two classes:
  then that pair's `clearance` holds.
- `impedance`: `{"ohms", "differential"?: false, "tolerance"?: percent, "coplanar_gap"?: mm}`. On a class,
  a net or a pair.

### `nets`

    {"name": "VCC", "role": "rail", "class": "Power", "width": 0.4, "clearance": 0.2,
     "via": 0.8, "drill": 0.4, "impedance": {"ohms": 50}}

`name` (required, once). `role`: `signal` (the default), `rail` or `ground`: 1.1 called this `class`.
`class`: the name of its net class (default `Default`). `width`, `clearance`, `via`, `drill`: the net's
own, over its class's. `route_on`: the copper layers its tracks run on (`["B.Cu"]`); left out, any. Its
vias go through every layer as always. A pad of such a net on another layer is left by a stub to a via, no
further from the pad's copper than `--stub` (2.5 mm) and as short as there is room for; copper it
already has stays where it is. The nets to join are the nets with two or more pads; a net named by no
pad is fine.

### `pads`

    {"ref": "U1", "num": "3", "net": "SIG", "at": [3.8, 6.25], "fp": [6, 6], "size": [1, 0.3],
     "tht": false, "side": "B", "clearance": 0.2, "drill": 1.0, "pour": "solid", "thermal_gap": 0.5, "spoke_angle": 45,
     "shape": {"poly": [[x, y] ...], "r": 0}}

`ref`, `num`, `at`, `shape` required. `net`: a net of `nets`, or `""` or absent for none (a pad naming a
net that `nets` does not list is an error). `fp`: the centre of its footprint (default: `at`). `size`:
two numbers, not read. `tht`: it goes through every layer. `side`: `"F"` (default) or `"B"`.
`clearance`: the pad's own, over every class's. `drill`: its hole. `pour`: how a pour of its net joins
the pad when the pad says so itself, over the zone's `pads` (`solid`, `thermal`, `thermal_tht`, `none`);
`thermal_gap`: its own relief gap; `spoke_angle`: the angle its relief's spokes stand at in the pad's own
frame, degrees (default: 90, along its sides); for a round pad, on the board. `shape` is one of
`{"poly": [[x, y] ...], "r": R}` (a polygon grown by R), `{"circle": [x, y, r]}`,
`{"oval": [ax, ay, bx, by, r]}`, in board coordinates.

### `keepouts`

    {"name": "no vias beside J1", "layers": ["F.Cu"], "poly": [[x, y] ...], "forbid": ["vias"], "net": "RF"}

`poly` required. `layers`: default all. `forbid`: any of `tracks`, `vias`, `zones`, `pads`, `footprints`;
default `tracks`, `vias`, `zones` (what a 1.1 keep-out forbids). `net` (optional): the one net whose tracks
the area lets through (an opening in the solder mask over that net's pads). An area that forbids tracks is
in a track's way; one that forbids vias alone closes its via sites; one that forbids zones bounds the pours.
`pads` and `footprints` are not the router's affair.

### `copper`

    {"tracks": [{"net": "AUX", "layer": "B.Cu", "width": 0.2, "path": [line|arc ...], "locked": true}],
     "vias":   [{"net": "GND", "at": [2, 10], "size": 0.6, "drill": 0.3,
                 "layers": ["In2.Cu", "B.Cu"], "locked": false}],
     "zones":  [{"net": "GND", "layer": "In1.Cu", "outline": [[x, y] ...], "holes": [[[x, y] ...] ...],
                 "fill": [{"outline": [[x, y] ...], "holes": [[[x, y] ...] ...]}],
                 "priority": 0, "clearance": 0.2, "min_width": 0.2, "pads": "solid",
                 "thermal_gap": 0.5, "spoke_width": 0.5, "locked": false}]}

Copper already on the board. An object in scene 2 (1.1 has a list, counted and passed over).

- A track: `layer`, `width`, `path` required; `net` (default none); `locked`.
- A via: `at`, `size`, `drill` required; `layers`: its first and last copper layer when it is blind or
  buried (default: through); `locked`.
- A zone lies on one layer (a KiCad zone on three layers is three zones here). `layer`, `outline`
  required; `holes`: loops cut out of the outline; `fill`: the copper as KiCad last filled it, one entry an
  island, each with its own holes; `pads`: `solid` (default), `thermal`, `thermal_tht`, `none` (the zone
  joins no pad by itself); `thermal_gap`, `spoke_width`: the gap a thermal relief leaves round a pad and the
  width of its spokes; `priority`,
  `clearance`, `min_width`, `locked`.
- `locked`: the board's owner pinned it. Locked copper is never lifted (`docs/hooks.md`, copper kinds).

### `parts`, `groups`

    "parts":  [{"ref": "U1", "courtyard": [[x, y] ...], "side": "F", "fixed": false}],
    "groups": [{"name": "U1 and its capacitors", "parts": ["U1", "C1", "C2"]}]

A part: `ref` (required, once), `courtyard` (a polygon, or left out), `side`, `fixed`. A group: parts that
belong together; each must be in `parts`.

### `pairs`, `match`

    "pairs": [{"name": "USB", "p": "USB_DP", "n": "USB_DM", "width": 0.2, "gap": 0.15,
               "impedance": {"ohms": 90, "differential": true}, "max_skew": 0.5, "max_uncoupled": 2}],
    "match": [{"name": "DQ0", "nets": ["DQ0", "DQ1"], "tolerance": 0.5, "target": 25, "by": "length"}]

A pair: `name`, `p`, `n` required (two different nets); the rest optional, over the class's.
A match group: `name`, `nets`, `tolerance` (mm) required; `target`: the length to reach, mm (default: the
longest net's); `by`: `length` (default) or `time`.

### `scope`

    {"nets": ["SIG", "AUX"], "parts": ["U1", "R1"]}

Which nets are to be routed and which parts may be touched. Left out, or a key left out: all of them.
A net out of scope keeps the copper it has and gets none: its pads, its copper and the way out of its
pins are kept clear of by the nets that are routed, it is not counted as open, and the plan names it
(`not_asked`), so the check does not ask whether it is whole. `--nets` and `--except` on the command line
narrow the scope further, by name, with `*` for any run of letters. `parts` is read and not used yet.

## The validator

Every scene, 1.1 or 2, is checked before it is routed (`src/scene/check.rs`). The first thing wrong is
the error; the router then exits 2.

| what | limit |
|---|---|
| copper layers | 2 to 30 |
| nets | at most 60,000 |
| any coordinate | within 100,000 mm of the origin |
| the outline | more than 0 and at most 2,000 mm each way |
| a track width, a via size | more than 0, at most 100 mm |
| a clearance, a drill, a hole, a shape's `r`, a thickness | 0 to 100 mm |
| an impedance | more than 0, at most 1,000 ohms; its tolerance 0 to 100 percent |
| `epsilon_r`, `loss_tangent` | 1 to 100; 0 to 1 |
| a number | finite (the JSON reader refuses `1e999`); nesting at most 64 deep |

The grid the router lays over the board has limits of its own, said when it is built (`docs/hooks.md`).

## From 1.1 to 2

Set `"version": 2`. Rename each net's `class` to `role`. Take out the field engine's keys (`layers`,
`marked`, `swappable`) and a part's `max_move`; `bench` may stay. Make `copper` an object. Name every net a pad uses in `nets`.
Give `rules` all six of its numbers.
