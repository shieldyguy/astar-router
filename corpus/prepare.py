#!/usr/bin/env python3
"""prepare: the corpus made from the boards on this machine (T13.1).

    prepare.py [NAME ...] [--source KIND] [--resume] [--jobs 2] [--list] [--classes]

For each board: a copy brought to the house KiCad's file format (`kicad-cli pcb upgrade`), made bare (tracks,
track arcs, vias, teardrop zones and zone fills taken out; zones, parts, rules and everything else kept), a scene
exported the way router-bench does it (`kicad-cli pcb route-field --scene`, then normalised), and the bare
board's own DRC. They go to corpus/boards/NAME/ (NAME.kicad_pcb, its .kicad_pro and .kicad_dru, scene.json,
bare-drc.json, field.json: the hub the export was given), and the board's facts to corpus/manifest.json.
With names, only those boards are made again and the manifest's other rows are kept.

The boards (SOURCES below says where each kind is looked for):
  bench    router-bench's five, as they are (already bare)
  krt      KRT's own kicad_files/
  house    the house library's finished blocks (library/blocks/B/kicad/B.kicad_pcb) and the placed or routed
           boards among pcb/tests/fixtures. Not taken: anything under build/ (seeds, before-* checkpoints,
           trials), sch/tests/fixtures and GS.kicad_blocks (copies of the blocks), library/rules (a template)
  qa       the KiCad fork's qa/data/pcbnew/field_test boards
  demo     the KiCad fork's demos/ (real designs, routed; here they are stripped bare)
  stress   KRT's stress corpus: open boards on GitHub, fetched by corpus/fetch_stress.py into a cache outside
           the repo. Third-party: never committed. Their bare boards and scenes go to CACHE/boards/NAME/ (CACHE:
           $ASTAR_CORPUS_CACHE, else /home/claude/corpus-cache), the manifest names each one's repository, commit
           and path. A stress board gets less time than the others (DRC 300 s, export 240 s, two tries)

A board is left out, with the reason in the manifest's `left_out`, when: KiCad cannot read it; it has fewer
than two nets to join; its parts sit exactly where an earlier board's do, with the same zones (a copy); or it
is a house board that is byte for byte its own seed (the first placement off the sheet, not a placed board);
or pads of it lie more than 1 mm outside the outline's box (parts never put on the board).
A board that gives no scene stays in the corpus: `scene` is null and `scene_why` says what the exporter said.
A scene is checked against its board: `pads_not_in_scene` counts the board's pads with a net that the scene
lacks (0 is right; a scene that lacks pads asks a router for less than KiCad will hold it to).

Committed: the bench five, and every other board whose folder is under 1.5 MB. The larger ones are listed in
corpus/boards/.gitignore and made again with `prepare.py NAME ...` (`committed` false in the manifest).
With corpus/baseline.jsonl there, each board gets a class from its baseline row: "must route" (it passed),
"hard" (it has a scene and did not pass), "known bad" (no scene, so astar cannot be given it yet).

A zone is a plane of the scene (`stack.planes`: "this net owns this layer") only when its outline holds every
pad of its net; any other zone (a pour under one chip, a split) stays copper on the bare board that KiCad
refills, and its net is routed as track (`zones_not_planes` counts them).

The exporter (Route Field) takes boards of four copper layers only. A board with two is exported from a
temp copy given two empty inner layers, and the scene's stack is then the board's own two: `scene` says
"four-layer copy" for these, "direct" for the others. It also takes a surface pad on the back for a terminal
only when the back layer is called B.Cu: a board that renames its copper layers is exported from a copy with
the plain names (`export_layer_names`).
"""
import argparse, collections, concurrent.futures as cf, copy, glob, hashlib, json, math, os, re, shutil, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
REPO = os.path.dirname(ROOT)
sys.path.insert(0, os.path.join(ROOT, "tests"))
import judge as J

BOARDS = os.path.join(HERE, "boards")
CACHE = os.environ.get("ASTAR_CORPUS_CACHE") or "/home/claude/corpus-cache"       # third-party boards: never in the repo
MANIFEST = os.path.join(HERE, "manifest.json")
HOME = os.path.dirname(REPO) if os.path.basename(os.path.dirname(REPO)) != "wt" else os.path.dirname(os.path.dirname(REPO))
SPOT = lambda *names: next((p for p in (os.path.join(d, *names) for d in (HOME, os.path.expanduser("~"), "/home/claude")) if os.path.exists(p)), None)
BENCH_FIVE = ["dock", "g474", "ccb", "tb150", "ub400"]
COMMIT_CAP = 1_500_000         # a board folder larger than this is made again by this script, not committed
CONTRACT = "1.1"


def slug(s):
    return re.sub(r"[^A-Za-z0-9_.-]+", "-", s).strip("-")


def board_dir(name, kind):
    """where a board's prepared files are: corpus/boards/NAME, or the cache for a third-party board"""
    return os.path.join(CACHE, "boards", name) if kind == "stress" else os.path.join(BOARDS, name)


def candidates():
    """(name, source kind, path, the field file to export with or None), in the order copies are told apart"""
    out = []
    bench = SPOT("router-bench", "boards")
    for b in BENCH_FIVE if bench else []:
        out.append((b, "bench", os.path.join(bench, b, b + ".kicad_pcb"), os.path.join(bench, b, "hints", "field.json")))
    for f in sorted(glob.glob(os.path.join(REPO, "kicad_files", "*.kicad_pcb"))):
        out.append(("krt_" + slug(os.path.basename(f)[:-10]), "krt", f, None))
    tools = SPOT("tools")
    if tools:
        for d in sorted(glob.glob(os.path.join(tools, "library", "blocks", "*"))):
            b = os.path.basename(d)
            f = os.path.join(d, "kicad", b + ".kicad_pcb")
            if not os.path.exists(f):        # a block kept only as its packaged copy
                f = os.path.join(tools, "library", "GS.kicad_blocks", b + ".kicad_block", b + ".kicad_pcb")
            if os.path.exists(f):
                out.append(("house_" + slug(b), "house", f, None))
        fx = os.path.join(tools, "pcb", "tests", "fixtures")
        found = sorted(glob.glob(os.path.join(fx, "*.kicad_pcb"))) + sorted(glob.glob(os.path.join(fx, "*", "*", "kicad", "*.kicad_pcb")))
        for f in found:
            rel = os.path.relpath(f, fx)
            if rel.startswith("library_split"):
                continue
            parts = rel.split(os.sep)
            name = parts[0][:-10] if len(parts) == 1 else f"{parts[0]}-{parts[1]}"
            out.append(("fix_" + slug(name), "house", f, None))
    kicad = SPOT("kicad")
    if kicad:
        for f in sorted(glob.glob(os.path.join(kicad, "qa", "data", "pcbnew", "field_test", "**", "*.kicad_pcb"), recursive=True)):
            out.append(("qa_" + slug(os.path.basename(f)[:-10]), "qa", f, None))
        for f in sorted(glob.glob(os.path.join(kicad, "demos", "**", "*.kicad_pcb"), recursive=True)):
            out.append(("demo_" + slug(os.path.basename(f)[:-10]), "demo", f, None))
    # KRT's stress corpus as corpus/fetch_stress.py fetched it, in the order of KRT's lists
    fetched = []
    for f in glob.glob(os.path.join(CACHE, "stress", "*", "source.json")):
        src = json.load(open(f))
        fetched.append((src["set"], src["name"], os.path.join(os.path.dirname(f), src["name"] + ".kicad_pcb")))
    order = lambda t: (not re.fullmatch(r"set\d+", t[0]), int(re.sub(r"\D", "", t[0]) or 0), t[0], t[1])
    for _, name, f in sorted(fetched, key=order):
        out.append((name, "stress", f, None))
    return out


# ---------------------------------------------------------------- the board made bare

def make_bare(text):
    """the board without its tracks, track arcs, vias, teardrop zones, tuning patterns and zone fills.
    Returns (text, tracks taken out, vias taken out)."""
    lines = text.split("\n")
    drop = [False] * len(lines)
    tracks = vias = 0
    for kind, i, j in J.block_spans(lines, ["segment", "arc", "via", "zone", "generated"]):
        if kind == "zone":
            if any("(teardrop" in ln for ln in lines[i:j + 1]):
                drop[i:j + 1] = [True] * (j + 1 - i)
                continue
            k = i
            while k <= j:
                if re.match(r"^\t\t\((filled_polygon|fill_segments)\b", lines[k]):
                    e = k
                    while e <= j and lines[e] != "\t\t)":
                        e += 1
                    drop[k:e + 1] = [True] * (e + 1 - k)
                    k = e + 1
                    continue
                k += 1
            continue
        if kind == "via":
            vias += 1
        elif kind in ("segment", "arc"):
            tracks += 1
        drop[i:j + 1] = [True] * (j + 1 - i)
    return "\n".join(ln for ln, d in zip(lines, drop) if not d), tracks, vias


_STR = r'"((?:\\.|[^"\\])*)"'


def footprints(text):
    """the board's parts: [{"ref", "at": (x, y), "rot", "back", "pads": [{"num", "type", "at": (local x, y), "net"}]}]"""
    out = []
    for _, b in J.top_blocks(text, ["footprint"]):
        ref = re.search(r'\(property "Reference" ' + _STR, b)
        # where the part is: (at x y [rot]) up to KiCad 10.0, (transform (translate x y) (rotate r) ...) in the house format
        at = re.search(r"\n\t\t\(at ([-\d.e]+) ([-\d.e]+)(?: ([-\d.e]+))?\)", b)
        if not at:
            at = re.search(r"\n\t\t\(transform\s*\(translate ([-\d.e]+) ([-\d.e]+)\)\s*(?:\(rotate ([-\d.e]+)\))?", b)
        if not at:
            continue
        pads = []
        lines = b.split("\n")
        k = 0
        while k < len(lines):
            m = re.match(r"^\t\t\(pad " + _STR + r" (\w+)", lines[k])
            if not m:
                k += 1
                continue
            e = k
            while e < len(lines) and lines[e] != "\t\t)":
                e += 1
            body = "\n".join(lines[k:e + 1])
            pat = re.search(r"\(at ([-\d.e]+) ([-\d.e]+)", body)
            net = re.search(r"\(net (?:\d+ )?" + _STR + r"\)", body)
            pads.append({"num": m.group(1), "type": m.group(2), "at": (float(pat.group(1)), float(pat.group(2))) if pat else (0.0, 0.0),
                         "net": net.group(1) if net else ""})
            k = e + 1
        layer = re.search(r'\n\t\t\(layer "([^"]+)"\)', b)
        out.append({"ref": ref.group(1) if ref else "", "at": (float(at.group(1)), float(at.group(2))), "rot": float(at.group(3) or 0),
                    "back": bool(layer and layer.group(1) == "B.Cu"), "pads": pads})
    return out


def has_outline(text):
    """is anything drawn on Edge.Cuts, by the board or by a part"""
    return bool(re.search(r'\(layer "Edge\.Cuts"\)', text))


def with_inner_layers(text):
    """a two-layer board's text with two empty inner layers, for the exporter alone"""
    m = re.search(r'\n\t\t\(0 "F\.Cu" [^\n]*\n', text)
    if not m:
        return None
    return text[:m.end()] + '\t\t(4 "In1.Cu" signal)\n\t\t(6 "In2.Cu" signal)\n' + text[m.end():]


def with_plain_layer_names(text):
    """the board's text with its copper layers under their own names (F.Cu, In1.Cu, B.Cu), for the exporter
    alone: it makes a surface pad on the back a terminal only on a board whose back layer is *called* B.Cu
    (pcbnew/router/field_scene.cpp, the pad loop), and a keep-out on a board that calls it bottom_copper"""
    m = re.search(r"\n\t\(layers\n(.*?)\n\t\)", text, re.S)
    if not m:
        return text
    body = re.sub(r'(\(\d+ "[^"]+\.Cu" \w+) "[^"]*"\)', r"\1)", m.group(1))
    return text[:m.start(1)] + body + text[m.end(1):]


def pads_not_in_scene(fps, scene):
    """the board's pads with a net (plated holes and surface pads) that the scene does not have, as
    (ref, num, net): a scene that lacks pads asks for less than the board needs"""
    want = collections.Counter((f["ref"], p["num"], p["net"]) for f in fps for p in f["pads"] if p["net"] and p["type"] != "np_thru_hole")
    have = collections.Counter((p.get("ref"), p.get("num"), p.get("net")) for p in scene["pads"] if p.get("net"))
    return sorted((want - have).elements())


# ---------------------------------------------------------------- the scene

def zones_of(text, canon):
    out = []
    for _, b in J.top_blocks(text, ["zone"]):
        if "(keepout" in b:
            continue
        net = re.search(r'\(net (?:\d+ )?"?([^")]*)"?\)', b)
        layers = re.search(r"\(layers? ([^)]*)\)", b)
        if not net or not net.group(1) or not layers:
            continue
        rings = [[(float(x), float(y)) for x, y in re.findall(r"\(xy ([-\d.e]+) ([-\d.e]+)\)", part)] for part in b.split("(polygon")[1:]]
        rings = [r for r in rings if len(r) >= 3]
        for L in re.findall(r'"([^"]+)"', layers.group(1)):
            out.append({"net": net.group(1), "layer": canon.get(L, L), "pts": rings[0] if rings else [], "rings": rings})
    return out


def inside_rings(x, y, rings):
    """is the point inside the area the loops draw (even-odd over every loop: an outline, its cut-outs)"""
    inside = False
    for r in rings:
        for (x0, y0), (x1, y1) in zip(r, r[1:] + r[:1]):
            if (y0 > y) != (y1 > y) and x < (x1 - x0) * (y - y0) / (y1 - y0) + x0:
                inside = not inside
    return inside


def planes_of(zones, pads):
    """the zones that are a plane for their net: one zone whose outline holds every pad of the net. A router is
    told "this net owns this layer: reach it and you are joined" (CONTRACT.md); a pour under one chip, or a zone
    half the board wide, is not that, and stays what it is on the bare board: copper KiCad refills.
    Returns (planes, the zones that are not one)."""
    by_net = collections.defaultdict(list)
    for p in pads:
        if p.get("net"):
            by_net[p["net"]].append(p["at"])
    planes, rest = [], []
    for z in zones:
        q = {"net": z["net"], "layer": z["layer"]}
        at = by_net.get(z["net"], [])
        if len(at) >= 2 and all(inside_rings(x, y, z["rings"]) for x, y in at):
            if q not in planes:
                planes.append(q)
        else:
            rest.append(q)
    return planes, [q for q in rest if q not in planes]


def normalise_scene(sc, text, name, how):
    """the exporter's scene made neutral, as router-bench's normalise_scene does: canonical layer names, the
    board's own copper stack and its zones as planes, the field engine's own hints taken out"""
    lt = J.layer_table(text)
    canon = {}
    for c, u in lt.items():
        canon[u] = c
        canon[c] = c
    cn = lambda L: canon.get(L, L)
    sc = copy.deepcopy(sc)
    if "layers" in sc:
        sc["layers"] = {k: cn(v) for k, v in sc["layers"].items()}
    for k in sc.get("keepouts", []):
        k["layers"] = [cn(L) for L in k.get("layers", [])]
    if how != "direct":
        # layers the board does not have were the exporter's copy's: a keep-out on none of the board's own goes
        keep = []
        for k in sc.get("keepouts", []):
            k["layers"] = [L for L in k["layers"] if L in lt]
            if k["layers"]:
                keep.append(k)
        sc["keepouts"] = keep
    for c in sc.get("copper", []):
        if "layer" in c:
            c["layer"] = cn(c["layer"])
    for p in sc.get("pads", []):
        if "layer" in p:
            p["layer"] = cn(p["layer"])
    planes, _ = planes_of(zones_of(text, canon), sc.get("pads", []))
    for k in ("chart", "field", "options"):
        sc.pop(k, None)
    by = "kicad-cli pcb route-field --scene (KiCad fork 10.99), normalised by corpus/prepare.py"
    if how != "direct":
        by += "; exported from a copy of the board given two empty inner layers (the exporter takes four-layer boards only), the stack is the board's own"
    out = {"bench": {"contract": CONTRACT, "board": name, "exported_by": by, "field_specific": ["layers", "marked", "swappable"]},
           "stack": {"copper": list(lt), "planes": planes}}
    out.update(sc)
    return out


def write_scene(scene, path):
    """the scene as JSON, one line a pad, a net, a part: small, and a change shows as the lines it touched"""
    one = lambda v: json.dumps(v, separators=(", ", ": "))
    out = []
    for k, v in scene.items():
        if isinstance(v, list) and v:
            out.append(f" {json.dumps(k)}: [\n  " + ",\n  ".join(one(x) for x in v) + "]")
        else:
            out.append(f" {json.dumps(k)}: {one(v)}")
    open(path, "w").write("{\n" + ",\n".join(out) + "\n}\n")


def export_scene(board, field, tmp, tag, timeout=600, tries=3):
    """(scene or None, the exporter's last words, tries). The exporter plans the board before it writes the
    scene, and the planner sometimes dies (a signal, std::bad_alloc) on a board it exports the next time: such
    an end is tried again, up to three times; a plain refusal (no outline, the layer count) is not."""
    out_scene = os.path.join(tmp, f"scene-{tag}.json")
    cmd = [J.kicad_cli(), "pcb", "route-field", board, "-f", field, "-o", os.path.join(tmp, f"routed-{tag}.kicad_pcb"),
           "--scene", out_scene, "--no-move-parts", "--budget", "2"]
    said = ""
    for k in range(1, tries + 1):
        with J.heavy_slot():
            code, out = J.sh(cmd, timeout)
        if os.path.exists(out_scene):
            try:
                return json.load(open(out_scene)), said, k
            except ValueError as e:
                return None, f"the exported scene is not JSON: {e}", k
        lines = [ln.strip() for ln in (out or "").split("\n") if ln.strip() and "Font '" not in ln]
        if code is None:
            return None, f"the exporter was still going after {timeout} s", k
        if code < 0:
            said = f"the exporter died (signal {-code})" + (f": {lines[-1][:200]}" if lines else "")
        else:
            said = (lines[-1] if lines else f"exit {code}, nothing said")[:300]
            if "bad_alloc" not in said:
                return None, said, k
    return None, said + f" ({tries} tries)", tries


# ---------------------------------------------------------------- facts

def area_arrays(fps):
    """parts whose pads fill a grid of at least 4 by 4 (BGA, LGA, PGA), told in the part's own frame"""
    out = []
    for f in fps:
        pads = [p for p in f["pads"] if p["type"] != "np_thru_hole"]
        if len(pads) < 16:
            continue
        xs = sorted({round(p["at"][0], 2) for p in pads})
        ys = sorted({round(p["at"][1], 2) for p in pads})
        if min(len(xs), len(ys)) < 4 or len(pads) < 0.5 * len(xs) * len(ys):
            continue
        steps = [b - a for a, b in zip(xs, xs[1:])] + [b - a for a, b in zip(ys, ys[1:])]
        out.append({"ref": f["ref"], "pads": len(pads), "grid": [len(xs), len(ys)], "pitch": round(min(steps), 3),
                    "tht": any(p["type"] == "thru_hole" for p in pads)})
    return sorted(out, key=lambda a: -a["pads"])


def pairs_by_name(names):
    """differential pairs as KiCad tells them by name: the last character before any trailing digits and
    underscores is + or -, or P or N, and the net with the other one exists; and the USB habit DP / DM"""
    have = set(names)
    out = []
    for n in sorted(names):
        k = len(n) - 1
        while k >= 0 and (n[k].isdigit() or n[k] == "_"):
            k -= 1
        if k < 1:
            continue
        mate = None
        if n[k] in "+P":
            mate = n[:k] + {"+": "-", "P": "N"}[n[k]] + n[k + 1:]
        if (mate is None or mate not in have) and n[k] == "P" and n[k - 1] == "D":
            mate = n[:k] + "M" + n[k + 1:]
        if mate in have and mate != n:
            out.append([n, mate])
    return out


def poly_area(pts):
    return abs(sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(pts, pts[1:] + pts[:1]))) / 2 if len(pts) > 2 else 0.0


def board_facts(text, fps):
    """what the board file itself says: layers, parts, pads, the nets with two or more pads, zones, pairs"""
    lt = J.layer_table(text)
    canon = {}
    for c, u in lt.items():
        canon[u] = c
        canon[c] = c
    pads = [p for f in fps for p in f["pads"]]
    count = collections.Counter(p["net"] for p in pads if p["net"] and p["type"] != "np_thru_hole")
    nets = sorted(n for n, c in count.items() if c >= 2)
    zones = [{"net": z["net"], "layer": z["layer"], "area_mm2": round(sum(poly_area(r) for r in z["rings"][:1]), 1)} for z in zones_of(text, canon)]
    return {"copper_layers": len(lt), "layers": list(lt), "parts": len(fps), "pads": len(pads),
            "pads_tht": sum(1 for p in pads if p["type"] == "thru_hole"),
            "pads_back": sum(len(f["pads"]) for f in fps if f["back"]),
            "nets": len(nets), "outline": has_outline(text),
            "area_arrays": area_arrays(fps), "zones": zones,
            "keepout_zones": sum(1 for _, b in J.top_blocks(text, ["zone"]) if "(keepout" in b),
            "pairs": pairs_by_name(nets)}, nets


def scene_facts(scene, row, board_nets):
    """what the scene adds: the board's size, the nets a router is asked to join, the rules it is given"""
    nets = J.routable_nets(scene)
    xs = [v for q in scene["outline"] for v in (q.get("line") or q.get("arc"))[0::2]]
    ys = [v for q in scene["outline"] for v in (q.get("line") or q.get("arc"))[1::2]]
    w, h = (max(xs) - min(xs), max(ys) - min(ys)) if xs else (0, 0)
    listed = {n["name"] for n in scene["nets"]}
    count = collections.Counter(p["net"] for p in scene["pads"] if p.get("net"))
    for z in row["zones"]:
        z["cover"] = round(min(1.0, z["area_mm2"] / (w * h)), 2) if w * h else None
    return {"size_mm": [round(w, 2), round(h, 2)], "nets": len(nets), "nets_on_board": len(board_nets),
            "net_classes": dict(collections.Counter(n["class"] for n in scene["nets"] if n["name"] in set(nets))),
            "nets_outside_scene": sorted(n for n, c in count.items() if c >= 2 and n not in listed),
            "nets_scene_lacks": sorted(set(board_nets) - set(nets))[:20],
            "scene_pads": len(scene["pads"]),
            "planes": sorted(f"{q['net']}@{q['layer']}" for q in scene["stack"]["planes"]),
            "zones_not_planes": len({(z["net"], z["layer"]) for z in row["zones"]} - {(q["net"], q["layer"]) for q in scene["stack"]["planes"]}),
            "scene_keepouts": len(scene.get("keepouts", [])), "holes": len(scene.get("holes", [])),
            "rules": scene.get("rules", {})}


def off_board(scene):
    """the pads more than 1 mm outside the box of the outline: parts that were never put on the board"""
    xs = [v for q in scene["outline"] for v in (q.get("line") or q.get("arc"))[0::2]]
    ys = [v for q in scene["outline"] for v in (q.get("line") or q.get("arc"))[1::2]]
    if not xs:
        return []
    return [p for p in scene["pads"] if not (min(xs) - 1 <= p["at"][0] <= max(xs) + 1 and min(ys) - 1 <= p["at"][1] <= max(ys) + 1)]


def signature(fps, layers):
    """where every pad of every net is: two boards with the same one are the same routing problem"""
    rows = sorted((f["ref"], f["at"], f["rot"], f["back"], p["num"], p["net"], p["at"]) for f in fps for p in f["pads"])
    return hashlib.sha1(json.dumps([rows, layers]).encode()).hexdigest()[:16]


def its_seed(src):
    """a house board that is byte for byte a *.seeded.kicad_pcb beside it (the first placement off the sheet)"""
    d = os.path.dirname(os.path.dirname(src))
    body = open(src, "rb").read()
    for f in glob.glob(os.path.join(d, "**", "*.seeded.kicad_pcb"), recursive=True):
        if f != src and open(f, "rb").read() == body:
            return f
    return None


# ---------------------------------------------------------------- one board

def prepare_one(name, kind, src, field):
    row = {"name": name, "source": kind, "path": os.path.relpath(src, REPO) if src.startswith(REPO + os.sep) else src}
    # a third-party board is given less time: one that takes KiCad five minutes is not a board to test on
    t_drc, t_export, tries = (300, 240, 2) if kind == "stress" else (900, 600, 3)
    if kind == "stress":
        s = json.load(open(os.path.join(os.path.dirname(src), "source.json")))
        row.update(path=f"{s['repo']}@{s['commit'][:12]}:{s['path']}", repo=s["repo"], commit=s["commit"], repo_path=s["path"],
                   set=s["set"], license=s.get("license"), krt=s.get("krt"))
    tmp = tempfile.mkdtemp(prefix=f"astar-prep-{name}-")
    try:
        if kind == "house" and its_seed(src):
            return dict(row, taken=False, why=f"a seed, not a placed board: the same bytes as {os.path.basename(its_seed(src))}")
        board = os.path.join(tmp, name + ".kicad_pcb")
        shutil.copy(src, board)
        for ext in (".kicad_pro", ".kicad_dru"):
            if os.path.exists(src[:-10] + ext):
                shutil.copy(src[:-10] + ext, board[:-10] + ext)
        if kind != "bench":
            code, out = J.sh([J.kicad_cli(), "pcb", "upgrade", "--force", board], 300)
            text = open(board, errors="replace").read()
            if code != 0 or not text.startswith("(kicad_pcb") or "\n\t(" not in text:
                return dict(row, taken=False, why=f"KiCad cannot bring it to its format (exit {code}): {(out or '').strip()[-200:]}")
        text = open(board).read()
        bare, tracks, vias = make_bare(text)
        open(board, "w").write(bare)
        ver = re.search(r"\(version (\d+)\)", bare[:200])
        row["file_version"] = int(ver.group(1)) if ver else None
        row["project"] = os.path.exists(board[:-10] + ".kicad_pro")
        row["source_copper"] = {"tracks": tracks, "vias": vias}
        fps = footprints(bare)
        f, board_nets = board_facts(bare, fps)
        row.update(f)
        row["signature"] = signature(fps, f["layers"])
        if f["nets"] < 2:
            return dict(row, taken=False, why=f"{f['parts']} parts, {f['nets']} nets with two pads: nothing to route")
        try:
            d = J.bare_drc(board, None, tmp, t_drc)
        except Exception as e:
            return dict(row, taken=False, why=f"the bare board's DRC did not run: {e}")
        row["bare_drc"] = {"errors": d["errors"], "warnings": {k: v for k, v in d["warnings"].items() if k not in J.IGNORED_WARNINGS},
                           "unconnected": d["unconnected"]}
        # the scene
        if not field:
            big = max(fps, key=lambda q: len(q["pads"]))
            field = os.path.join(tmp, "field.json")
            json.dump({"version": 3, "units": "mm", "shapes": [{"kind": "hub", "id": "hub1", "points": [[round(big["at"][0], 3), round(big["at"][1], 3)]]}]},
                      open(field, "w"))
            row["hub"] = f"on {big['ref']}, the part with the most pads ({len(big['pads'])})"
        else:
            row["hub"] = "router-bench's hint (placed by a person on the main MCU)"
        # the exporter is given a copy with the copper layers under their own names when the board renames them
        plain = bare if kind == "bench" else with_plain_layer_names(bare)      # the bench five: as router-bench made them
        src_board = board
        if plain != bare:
            dp = os.path.join(tmp, "plain")
            os.makedirs(dp)
            src_board = os.path.join(dp, name + ".kicad_pcb")
            open(src_board, "w").write(plain)
            J.copy_project(board, src_board)
            row["export_layer_names"] = "plain (the board renames its copper layers: " + ", ".join(u for c, u in J.layer_table(bare).items() if c != u) + ")"
        if f["copper_layers"] == 2 and kind == "stress":
            # the exporter refuses every two-layer board ("a field needs 4"): not asked again for each of hundreds
            sc, said, n_tried = None, "not tried: two copper layers", 0
        else:
            sc, said, n_tried = export_scene(src_board, field, tmp, "direct", t_export, tries)
        how = "direct"
        row["export_direct"] = "ok" if sc else said
        if sc is None and f["copper_layers"] == 2 and "copper layers" in said:
            four = with_inner_layers(plain)
            if four:
                d4 = os.path.join(tmp, "four")
                os.makedirs(d4)
                b4 = os.path.join(d4, name + ".kicad_pcb")
                open(b4, "w").write(four)
                J.copy_project(board, b4)
                sc, said, n_tried = export_scene(b4, field, tmp, "four", t_export, tries)
                how = "four-layer copy"
        scene = None
        if sc is not None:
            try:
                scene = normalise_scene(sc, bare, name, how)
                row.update(scene=how, **scene_facts(scene, row, board_nets))
                lack = pads_not_in_scene(fps, scene)
                row["pads_not_in_scene"] = len(lack)
                if lack:
                    row["pads_not_in_scene_eg"] = [f"{r}.{n} [{net}]" for r, n, net in lack[:6]]
                if n_tried > 1:
                    row["export_tries"] = f"exported at try {n_tried}; before that: {said}"
            except Exception as e:
                scene, said = None, f"the exported scene could not be read: {type(e).__name__}: {e}"
        if scene is None:
            row.update(scene=None, scene_why=said)
        if scene is not None:
            off = off_board(scene)
            if off:
                return dict(row, taken=False, why=f"not a placed board: {len(off)} of {len(scene['pads'])} pads ({len({p['ref'] for p in off})} parts) "
                            "lie more than 1 mm outside the outline's box")
        dst = board_dir(name, kind)
        if os.path.isdir(dst):
            shutil.rmtree(dst)
        os.makedirs(dst)
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
            if os.path.exists(board[:-10] + ext):
                shutil.copy(board[:-10] + ext, dst)
        if scene is not None:
            write_scene(scene, os.path.join(dst, "scene.json"))
        json.dump(d, open(os.path.join(dst, "bare-drc.json"), "w"), indent=1)
        shutil.copy(field, os.path.join(dst, "field.json"))
        return dict(row, taken=True)
    except Exception as e:
        import traceback
        return dict(row, taken=False, why=f"prepare failed: {type(e).__name__}: {e}; {traceback.format_exc().splitlines()[-3].strip()}")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*")
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--list", action="store_true", help="name the candidates and stop")
    ap.add_argument("--classes", action="store_true", help="make no board again: only write each board's class from corpus/baseline.jsonl")
    ap.add_argument("--source", help="only the boards of this source (bench, krt, house, qa, demo, stress)")
    ap.add_argument("--resume", action="store_true", help="keep the boards the manifest already has; make the rest")
    a = ap.parse_args()
    cands = candidates()
    if a.list:
        for name, kind, src, _ in cands:
            print(f"{name:44s} {kind:6s} {src}")
        return 0
    todo = [] if a.classes else [c for c in cands if (not a.names or c[0] in a.names) and (not a.source or c[1] == a.source)]
    unknown = set(a.names) - {c[0] for c in cands}
    if unknown:
        print(f"prepare: no such board: {', '.join(sorted(unknown))}", file=sys.stderr)
        return 2
    old = {}
    if (a.names or a.classes or a.source or a.resume) and os.path.exists(MANIFEST):
        m = json.load(open(MANIFEST))
        old = {r["name"]: r for r in m["boards"] + m["left_out"]}
    if a.resume:
        todo = [c for c in todo if c[0] not in old or (old[c[0]].get("taken") and not os.path.isdir(board_dir(c[0], c[1])))]
    os.makedirs(BOARDS, exist_ok=True)
    rows = dict(old)
    done = 0
    with cf.ThreadPoolExecutor(max(1, min(2, a.jobs))) as ex:
        for row in ex.map(lambda c: prepare_one(*c), todo):
            rows[row["name"]] = row
            print(f"{row['name']:44s} {'taken' if row['taken'] else 'left out: ' + row['why']}"
                  + (f"  {row['copper_layers']} layers, {row['nets']} nets, {row['pads']} pads, scene {row['scene'] or 'NONE: ' + row['scene_why']}" if row["taken"] else ""), flush=True)
            done += 1
            if done % 10 == 0:                # a long run can be stopped and taken up again (--resume)
                write_manifest(cands, rows, quiet=True)
    write_manifest(cands, rows)
    return 0


def write_manifest(cands, rows, quiet=False):
    """the manifest from every row made so far: copies told apart, classes from the baseline, what is committed"""
    # copies: the same pads in the same places as an earlier board, and the same zones
    base = {}
    if os.path.exists(os.path.join(HERE, "baseline.jsonl")):
        base = {r["board"]: r for r in (json.loads(ln) for ln in open(os.path.join(HERE, "baseline.jsonl")) if ln.strip())}
    seen = {}
    boards, left = [], []
    for name, kind, _, _ in cands:
        r = rows.get(name)
        if r is None:
            continue
        key = (r.get("signature"), json.dumps(sorted((z["net"], z["layer"]) for z in r.get("zones", []))))
        if r["taken"] and key in seen:
            r = dict(r, taken=False, why=f"the same parts in the same places as {seen[key]}, and the same zones")
            if not quiet:
                print(f"{name:44s} left out: {r['why']}")
        if r["taken"]:
            seen[key] = name
            d = board_dir(name, kind)
            size = sum(os.path.getsize(os.path.join(d, f)) for f in os.listdir(d)) if os.path.isdir(d) else 0
            # a third-party board is never committed: the manifest says where it came from (repo, commit, repo_path)
            r["committed"] = kind == "bench" or (kind != "stress" and size <= COMMIT_CAP)
            r.pop("class", None)
            if name in base:
                r["class"] = "must route" if base[name]["status"] == "pass" else "hard" if r.get("scene") else "known bad"
            elif not r.get("scene"):
                r["class"] = "known bad"
            boards.append(r)
        else:
            if not quiet:
                shutil.rmtree(board_dir(name, kind), ignore_errors=True)
            left.append(r)
    # rows of boards this machine cannot look for (a stress board with no cache here) stay as they were
    here = {c[0] for c in cands}
    for name, r in rows.items():
        if name not in here:
            (boards if r.get("taken") else left).append(r)
    with open(os.path.join(BOARDS, ".gitignore"), "w") as f:
        f.write("# too large to commit: made again by `corpus/prepare.py NAME ...` (written by prepare.py)\n")
        f.write("".join(f"/{r['name']}/\n" for r in boards if not r["committed"] and r["source"] != "stress"))
    code, ver = J.sh([J.kicad_cli(), "version"], 60)
    part = MANIFEST + ".part"
    json.dump({"made_by": "corpus/prepare.py", "kicad": (ver or "").strip().split("\n")[-1],
               "boards": boards, "left_out": left}, open(part, "w"), indent=1)
    os.replace(part, MANIFEST)
    if not quiet:
        by = collections.Counter(r["source"] for r in boards)
        print(f"prepare: {len(boards)} boards in the corpus ({', '.join(f'{k} {v}' for k, v in by.items())}), {len(left)} left out; {MANIFEST}")


if __name__ == "__main__":
    sys.exit(main())
