#!/usr/bin/env python3
"""parity: astar's own scene of a board beside the house KiCad's export of it, value by value (T1.8).

    parity.py [NAME ...] [-v] [--json OUT.json]

For each corpus board (corpus/boards/NAME/NAME.kicad_pcb; all of them when none is named) `astar scene` reads
the board, and where the export left a scene (corpus/boards/NAME/scene.json, made by `kicad-cli pcb route-field
--scene` and corpus/prepare.py) the two are compared: layers, planes, rules, outline, holes, pads (place, net,
side, shape, clearance), nets (role, width, clearance), keep-outs, parts. Lengths agree within 1 um.

Every difference is put in a class. A class docs/parity.md explains is counted as explained; anything else is
unexplained and is listed (all of them with -v). Exit 0 when nothing is unexplained and every board was read.
The binary: $ASTAR, else target/release/astar.
"""
import argparse, collections, json, math, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
BOARDS = os.path.join(ROOT, "corpus", "boards")
TOL = 0.001          # mm: one micron
ARC = 0.003          # mm: where a curve is compared as chords (a courtyard's box, an outline's arc)

# the classes docs/parity.md explains, each with its line there
EXPLAINED = {
    "plane: a small zone": "the export calls every zone its net's plane; astar only one that covers half the board",
    "plane: one a net": "a net with pours over the board on several layers has one plane in astar's scene: an inner layer before an outer one, then the larger; the export lists them all and the router took the last",
    "net: not listed by the export": "the export lists the nets it would route: two or more of the pads it took, not named unconnected-*",
    "net: role": "ground by the largest pour on the field's ground layer (export) or by name and plane (astar); rails agree",
    "net: clearance": "the export passes a class clearance only when wider than the rule; so does astar, by the same rule",
    "pad: not in the export": "the export leaves out pads it cannot use: surface pads on neither outer layer, pads of footprints it skips",
    "pad: shape is not a rectangle": "the export gives the box round a trapezoid, chamfered or custom pad; astar its outline",
    "pad: round corners": "same copper: the export writes a rounded rectangle as the full box and its corner radius, astar as the box drawn in by the radius, grown by it",
    "pad: clearance below the rule": "astar passes every pad clearance the board gives; the export only those wider than the rule",
    "pad: clearance of a net not listed": "the export gives a pad of a net it does not list its class clearance as its own",
    "pad: ref of a second footprint": "two footprints of one reference: astar names the later ones REF~2; a footprint with none is #uuid",
    "net: class width": "astar passes the track width of a net's class whenever it is not the Default's; the export only for a rail (wider, and not ground)",
    "pad: on the back, a keep-out in the export": "a surface pad on the back is a pad in astar's scene; where the export had no layer for it, it made it a keep-out",
    "keepout: a hole in it": "a rule area with holes: the export takes its outer ring and keeps tracks out of the holes too; astar cuts the holes out",
    "keepout: layers the export does not route on": "the export knows three layers (front, lane, hop); astar gives a keep-out the layers the board gives it",
    "hole: a slot": "the export gives a slot as one round hole of its length; astar as a row of round holes of its width",
    "keepout: vias kept out": "the export spreads an area that keeps vias out over every layer it routes; astar keeps its layers and says what it forbids",
    "keepout: an inner pad": "a surface pad on an inner layer is a keep-out there in both; they are compared as keep-outs",
    "keepout: layers of the copy": "a two-layer board was exported from a four-layer copy: the copy's inner layers are not the board's",
    "outline: made up": "the board has no Edge.Cuts: astar takes the box round its parts (the export gives no scene at all)",
    "part: no reference": "the export leaves out footprints without a reference",
    "copper: drawn in footprints": "copper drawn in a footprint or on the board (a tab, an antenna) is in astar's scene; the export has no place for it",
    "export: none": "the export gave no scene for this board (no outline, or the exporter crashed)",
}


def close(a, b, tol=TOL):
    return abs(a - b) <= tol


def pts_close(a, b, tol=TOL):
    return len(a) == len(b) and all(close(x, y, tol) for x, y in zip(a, b))


def centre(s, m, e):
    ax, ay, bx, by, cx, cy = *s, *m, *e
    d = 2 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by))
    if abs(d) < 1e-12:
        return None
    ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d
    uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d
    return ux, uy


def piece_key(q):
    """a piece of an outline as something to compare: its two ends (in either order), and for an arc its centre
    and the side its middle lies on"""
    if "line" in q:
        v = q["line"]
        return ("line", (v[0], v[1]), (v[2], v[3]), None)
    v = q["arc"]
    return ("arc", (v[0], v[1]), (v[4], v[5]), (v[2], v[3]))


def same_piece(a, b):
    ka, kb = piece_key(a), piece_key(b)
    if ka[0] != kb[0]:
        return False
    fwd = pts_close(ka[1], kb[1]) and pts_close(ka[2], kb[2])
    rev = pts_close(ka[1], kb[2]) and pts_close(ka[2], kb[1])
    if not (fwd or rev):
        return False
    if ka[0] == "line":
        return True
    ca, cb = centre(ka[1], ka[3], ka[2]), centre(kb[1], kb[3], kb[2])
    if ca is None or cb is None:
        return ca is cb
    # the same circle, and the two middles on the same side of the chord
    side = lambda k: (k[2][0] - k[1][0]) * (k[3][1] - k[1][1]) - (k[2][1] - k[1][1]) * (k[3][0] - k[1][0])
    sa, sb = side(ka), side(kb) * (1 if fwd else -1)
    return pts_close(ca, cb, ARC) and (sa > 0) == (sb > 0)


def match_lists(mine, theirs, same):
    """pair each of theirs with one of mine; what is left on either side"""
    left = list(mine)
    lone = []
    for t in theirs:
        for i, m in enumerate(left):
            if same(m, t):
                del left[i]
                break
        else:
            lone.append(t)
    return left, lone


def shape_box(sh, corner_radius=False):
    """the box round a pad's copper. The export's poly with r is a box with round corners (r inside the box);
    astar's is a polygon grown by r."""
    if "circle" in sh:
        x, y, r = sh["circle"]
        return (x - r, y - r, x + r, y + r)
    if "oval" in sh:
        ax, ay, bx, by, r = sh["oval"]
        return (min(ax, bx) - r, min(ay, by) - r, max(ax, bx) + r, max(ay, by) + r)
    xs = [p[0] for p in sh["poly"]]
    ys = [p[1] for p in sh["poly"]]
    g = 0 if corner_radius else sh.get("r", 0)
    return (min(xs) - g, min(ys) - g, max(xs) + g, max(ys) + g)


def is_rect(poly):
    if len(poly) != 4:
        return False
    d = lambda a, b: math.hypot(a[0] - b[0], a[1] - b[1])
    return close(d(poly[0], poly[2]), d(poly[1], poly[3])) and close(d(poly[0], poly[1]), d(poly[2], poly[3]))


def poly_area(pts):
    return abs(sum(p[0] * q[1] - q[0] * p[1] for p, q in zip(pts, pts[1:] + pts[:1]))) / 2


class Diff:
    def __init__(self):
        self.rows = []          # (class, detail)

    def add(self, cls, detail):
        self.rows.append((cls, detail))

    def explained(self):
        return [r for r in self.rows if r[0] in EXPLAINED]

    def unexplained(self):
        return [r for r in self.rows if r[0] not in EXPLAINED]


def compare(mine, exp, d):
    # layers and planes
    if mine["stack"]["copper"] != exp["stack"]["copper"]:
        d.add("stack: copper layers", f'{mine["stack"]["copper"]} vs {exp["stack"]["copper"]}')
    box = outline_box(mine)
    board_area = (box[2] - box[0]) * (box[3] - box[1])
    mp = {(p["net"], p["layer"]) for p in mine["stack"]["planes"]}
    ep = {(p["net"], p["layer"]) for p in exp["stack"]["planes"]}
    zone_area = collections.defaultdict(float)
    for z in mine["copper"]["zones"]:
        zone_area[(z["net"], z["layer"])] = max(zone_area[(z["net"], z["layer"])], poly_area(z["outline"]))
    for p in sorted(ep - mp):
        small = zone_area.get(p, 0) < 0.5 * board_area
        other = next((q for q in mp if q[0] == p[0]), None)
        d.add("plane: a small zone" if small else "plane: one a net" if other else "plane: only in the export", f"{p[0]} on {p[1]}" + (f" (its plane is {other[1]})" if other and not small else ""))
    for p in sorted(mp - ep):
        d.add("plane: only in astar", f"{p[0]} on {p[1]}")

    # rules
    for k in ("track", "clearance", "via", "drill", "edge", "hole_clearance"):
        if not close(mine["rules"][k], exp["rules"][k], 1e-9):
            d.add("rule: " + k, f'{mine["rules"][k]} vs {exp["rules"][k]}')

    # outline
    left, lone = match_lists(mine["outline"], exp["outline"], same_piece)
    for q in left:
        d.add("outline: only in astar", json.dumps(q))
    for q in lone:
        d.add("outline: only in the export", json.dumps(q))

    # holes
    left, lone = match_lists(mine["holes"], exp["holes"], lambda a, b: pts_close(a, b))
    if left or lone:
        # a slot: the export's one hole is as long as the row of astar's
        rows = [h for h in left]
        for h in list(lone):
            near = [m for m in rows if math.hypot(m[0] - h[0], m[1] - h[1]) <= h[2] / 2 + TOL and m[2] < h[2]]
            if near:
                d.add("hole: a slot", f"{h} is {len(near)} holes of {near[0][2]}")
                lone.remove(h)
                for m in near:
                    left.remove(m)
        for h in left:
            d.add("hole: only in astar", str(h))
        for h in lone:
            d.add("hole: only in the export", str(h))

    # nets
    mn = {n["name"]: n for n in mine["nets"]}
    en = {n["name"]: n for n in exp["nets"]}
    pad_count = collections.Counter(p.get("net", "") for p in mine["pads"])
    for name, n in en.items():
        m = mn.get(name)
        if m is None:
            d.add("net: only in the export", name)
            continue
        if m.get("role", "signal") != n.get("class", "signal"):
            d.add("net: role", f'{name}: {m.get("role", "signal")} vs {n.get("class")}')
        elif not close(m.get("width", mine["rules"]["track"]), n.get("width") or exp["rules"]["track"], 1e-9):
            d.add("net: class width" if n.get("class", "signal") != "rail" and m.get("class") else "net: width", f'{name}: {m.get("width")} vs {n.get("width")}')
        if not close(m.get("clearance", 0), n.get("clearance", 0), 1e-9):
            d.add("net: clearance", f'{name}: {m.get("clearance")} vs {n.get("clearance")}')
    exp_pad_count = collections.Counter(p.get("net", "") for p in exp["pads"])
    for name in mn:
        if name not in en:
            few = exp_pad_count[name] < 2 or name.startswith("unconnected")
            d.add("net: not listed by the export" if few else "net: missing from the export", f"{name} ({pad_count[name]} pads, {exp_pad_count[name]} in the export)")

    # pads: the export's are a part of astar's, paired by reference, number and place
    def base(ref):
        return ref.split("~")[0]
    key = lambda p, ref=None: (ref if ref is not None else p["ref"], p["num"], round(p["at"][0], 3), round(p["at"][1], 3))
    mine_by = collections.defaultdict(list)
    for p in mine["pads"]:
        mine_by[key(p, "" if p["ref"].startswith("#") else base(p["ref"]))].append(p)
    listed = set(en)
    for e in exp["pads"]:
        got = mine_by.get(key(e))
        if not got:
            # within a micron but across a rounding edge
            got = next((v for k, v in mine_by.items() if v and k[0] == e["ref"] and k[1] == e["num"] and pts_close(v[0]["at"], e["at"])), None)
        if not got:
            d.add("pad: only in the export", f'{e["ref"]}.{e["num"]} at {e["at"]}')
            continue
        m = got.pop(0)
        who = f'{m["ref"]}.{m["num"]}'
        if m["ref"] != e["ref"]:
            d.add("pad: ref of a second footprint", f'{who} is {e["ref"]}.{e["num"]} in the export')
        if m.get("net", "") != e.get("net", ""):
            d.add("pad: net", f'{who}: {m.get("net")} vs {e.get("net")}')
        if not pts_close(m.get("fp", m["at"]), e.get("fp", e["at"])):
            d.add("pad: fp", f'{who}: {m.get("fp")} vs {e.get("fp")}')
        if bool(m.get("tht")) != bool(e.get("tht")):
            d.add("pad: tht", f'{who}: {m.get("tht")} vs {e.get("tht")}')
        if m.get("side", "F") != e.get("side", "F"):
            d.add("pad: side", f'{who}: {m.get("side", "F")} vs {e.get("side", "F")}')
        ms, es = m["shape"], e["shape"]
        mk, ek = next(k for k in ("circle", "oval", "poly") if k in ms), next(k for k in ("circle", "oval", "poly") if k in es)
        mb, eb = shape_box(ms), shape_box(es, corner_radius=True)
        if mk == "poly" and ek == "poly" and not is_rect(ms["poly"]):
            # the export's box in the pad's own frame holds astar's outline
            if not all(a >= b - TOL for a, b in zip(mb[:2], eb[:2])) or not all(a <= b + TOL for a, b in zip(mb[2:], eb[2:])):
                d.add("pad: shape", f'{who}: an outline that leaves the export\'s box: {mb} vs {eb}')
            else:
                d.add("pad: shape is not a rectangle", who)
        elif mk != ek:
            # a rounded rectangle whose radius is half its width is an oval or a circle
            if pts_close(mb, eb) and ek == "poly" and close(es.get("r", 0) * 2, min(eb[2] - eb[0], eb[3] - eb[1]), 2 * TOL):
                d.add("pad: round corners", who)
            else:
                d.add("pad: shape", f"{who}: {mk} vs {ek}")
        elif mk == "poly":
            r = ms.get("r", 0)
            if r > 0:
                # astar's rectangle drawn in by r: put it back out and it is the export's box
                q = ms["poly"]
                unit = lambda a, b: ((a[0] - b[0]) / math.hypot(a[0] - b[0], a[1] - b[1]), (a[1] - b[1]) / math.hypot(a[0] - b[0], a[1] - b[1]))
                full = []
                for i in range(4):
                    u, v = unit(q[i], q[i - 1]), unit(q[i], q[(i + 1) % 4])
                    full.append([q[i][0] + r * (u[0] + v[0]), q[i][1] + r * (u[1] + v[1])])
                if not close(r, es.get("r", 0)) or not match_lists(full, es["poly"], pts_close) == ([], []):
                    d.add("pad: shape", f"{who}: {full} r {r} vs {es['poly']} r {es.get('r')}")
                else:
                    d.add("pad: round corners", who)
            elif es.get("r", 0) > 0:
                d.add("pad: shape", f"{who}: r 0 vs {es.get('r')}")
            elif not match_lists(ms["poly"], es["poly"], pts_close) == ([], []):
                d.add("pad: shape", f"{who}: corners {ms['poly']} vs {es['poly']}")
        elif not pts_close(ms[mk], es[ek]):
            d.add("pad: shape", f"{who}: {ms[mk]} vs {es[ek]}")
        mc, ec = m.get("clearance", 0), e.get("clearance", 0)
        if not close(mc, ec, 1e-9):
            if ec == 0 and mc <= mine["rules"]["clearance"] + 1e-9:
                d.add("pad: clearance below the rule", f"{who}: {mc}")
            elif e.get("net") and e["net"] not in listed and mc < ec:
                d.add("pad: clearance of a net not listed", f"{who}: {mc} vs {ec}")
            else:
                d.add("pad: clearance", f"{who}: {mc} vs {ec}")
    extra = [p for v in mine_by.values() for p in v]

    # keep-outs: the same polygon on the same layers. The export knows three layers: front, lane, hop.
    roles = {v for k, v in exp.get("layers", {}).items() if k in ("front", "lane", "hop")}
    def same_ko(a, b):
        return match_lists(a["poly"], b["poly"], pts_close) == ([], [])
    def holed(a, b):
        return len(a["poly"]) > len(b["poly"]) and match_lists(b["poly"], a["poly"], pts_close)[0] == []
    left, lone = match_lists(mine.get("keepouts", []), exp.get("keepouts", []), lambda a, b: same_ko(a, b) and sorted(a["layers"]) == sorted(b["layers"]))
    for k in list(lone):
        m = next((x for x in left if same_ko(x, k) or holed(x, k)), None)
        if m is not None:
            left.remove(m)
            lone.remove(k)
            M, E = set(m["layers"]), set(k["layers"])
            if holed(m, k):
                d.add("keepout: a hole in it", f'{m.get("name", "")} on {m["layers"]}: {len(m["poly"])} points, the export\'s ring {len(k["poly"])}')
            elif "vias" in m.get("forbid", []) and (M & roles) <= E:
                d.add("keepout: vias kept out", f'{m.get("name", "")} on {m["layers"]}, the export on {k["layers"]}')
            elif E == M & roles:
                d.add("keepout: layers the export does not route on", f'{m["layers"]} vs {k["layers"]}')
            else:
                d.add("keepout: layers", f'{m["layers"]} vs {k["layers"]}')
            continue
        # a surface pad on the back that the export could not use is a keep-out there
        box = shape_box({"poly": k["poly"]})
        # (the box round a pad that is no rectangle is KiCad's polygon of it: larger by its arc error, 5 to 10 um)
        loose = lambda p: 2 * TOL if "poly" not in p["shape"] or is_rect(p["shape"]["poly"]) else 0.02
        pad = next((p for p in extra if p.get("side") == "B" and pts_close(shape_box(p["shape"]), box, loose(p))), None)
        if pad is not None:
            lone.remove(k)
            extra.remove(pad)
            d.add("pad: on the back, a keep-out in the export", f'{pad["ref"]}.{pad["num"]}')
    for k in left:
        if k.get("name", "").startswith("pad "):
            d.add("keepout: an inner pad", k["name"])
        elif not (set(k["layers"]) & roles):
            d.add("keepout: layers the export does not route on", f'{k.get("name", "")} on {k["layers"]}')
        else:
            d.add("keepout: only in astar", f'{k.get("name", "")} on {k["layers"]}, {len(k["poly"])} points')
    for k in lone:
        d.add("keepout: only in the export", f'on {k["layers"]}, {len(k["poly"])} points at {k["poly"][0]}')
    for p in extra:
        d.add("pad: not in the export", f'{p["ref"]}.{p["num"]} at {p["at"]}')

    # parts
    mparts = {p["ref"]: p for p in mine["parts"]}
    eparts = {}
    for p in exp.get("parts", []):
        eparts.setdefault(p["ref"], p)
    for ref, e in eparts.items():
        m = mparts.get(ref)
        if m is None:
            d.add("part: only in the export", ref)
            continue
        if bool(m.get("fixed")) != bool(e.get("fixed")):
            d.add("part: fixed", f'{ref}: {m.get("fixed")} vs {e.get("fixed")}')
        if m.get("side", "F") != e.get("side", "F"):
            d.add("part: side", f'{ref}: {m.get("side", "F")} vs {e.get("side", "F")}')
        mc, ec = m.get("courtyard"), e.get("courtyard")
        if bool(mc) != bool(ec) or (mc and not match_lists(mc, ec, lambda a, b: pts_close(a, b, ARC)) == ([], [])):
            d.add("part: courtyard", f"{ref}: {mc} vs {ec}")
    for ref in mparts:
        if ref not in eparts:
            d.add("part: no reference" if ref.startswith("#") else ("pad: ref of a second footprint" if "~" in ref else "part: only in astar"), ref)

    cu = mine["copper"]
    if cu["tracks"] or cu["vias"] or any(not z.get("pads") for z in cu["zones"]):
        drawn = len([t for t in cu["tracks"] if t.get("locked")]) + len([z for z in cu["zones"] if not z.get("pads")])
        if drawn:
            d.add("copper: drawn in footprints", f"{drawn} pieces")
    if exp.get("copper"):
        d.add("copper: in the export", f'{len(exp["copper"])} pieces')


def outline_box(sc):
    xs, ys = [], []
    for q in sc["outline"]:
        v = q.get("line") or q.get("arc")
        xs += v[0::2]
        ys += v[1::2]
    return (min(xs), min(ys), max(xs), max(ys))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*")
    ap.add_argument("-v", "--verbose", action="store_true")
    ap.add_argument("--json")
    a = ap.parse_args()
    astar = os.environ.get("ASTAR") or os.path.join(ROOT, "target", "release", "astar")
    names = a.names or sorted(n for n in os.listdir(BOARDS) if os.path.isdir(os.path.join(BOARDS, n)))
    out, totals = {}, collections.Counter()
    read = accepted = compared = agree = 0
    for name in names:
        board = os.path.join(BOARDS, name, name + ".kicad_pcb")
        if not os.path.exists(board):
            print(f"{name}: no board at {board} (corpus/prepare.py {name} makes it)")
            continue
        r = subprocess.run([astar, "scene", board], capture_output=True, text=True)
        row = {"exit": r.returncode, "notes": [l[len("astar: board: "):] for l in r.stderr.splitlines() if l.startswith("astar: board: ")]}
        out[name] = row
        if not r.stdout.strip():
            row["error"] = r.stderr.strip()[-300:]
            print(f"{name}: NOT READ: {row['error']}")
            continue
        read += 1
        mine = json.loads(r.stdout)
        if r.returncode == 0:
            accepted += 1
        else:
            row["refused"] = r.stderr.strip().splitlines()[-1][-300:]
        d = Diff()
        if any("no outline" in n for n in row["notes"]):
            d.add("outline: made up", "")
        scene = os.path.join(BOARDS, name, "scene.json")
        if os.path.exists(scene):
            compared += 1
            compare(mine, json.load(open(scene)), d)
        else:
            d.add("export: none", "")
        ex, un = d.explained(), d.unexplained()
        if os.path.exists(scene) and not un:
            agree += 1
        row["explained"] = dict(collections.Counter(c for c, _ in ex))
        row["unexplained"] = [list(x) for x in un]
        for c, _ in d.rows:
            totals[c] += 1
        tail = "" if r.returncode == 0 else f"  REFUSED: {row['refused']}"
        print(f"{name}: {len(mine['pads'])} pads, {len(mine['nets'])} nets; {len(ex)} explained, {len(un)} unexplained{tail}")
        shown = un if a.verbose else un[:6]
        for c, detail in shown:
            print(f"    {c}: {detail[:200]}")
        if len(un) > len(shown):
            print(f"    ... and {len(un) - len(shown)} more (-v)")
    print()
    for c, k in sorted(totals.items(), key=lambda kv: (kv[0] not in EXPLAINED, -kv[1])):
        print(f"  {k:6d}  {'explained  ' if c in EXPLAINED else 'UNEXPLAINED'}  {c}")
    un_total = sum(k for c, k in totals.items() if c not in EXPLAINED)
    print(f"\nparity: {len(names)} boards; read {read}; scenes astar takes {accepted}; compared with the export {compared}; "
          f"no unexplained difference on {agree}; {sum(totals.values()) - un_total} differences explained, {un_total} unexplained")
    if a.json:
        json.dump(out, open(a.json, "w"), indent=1)
    sys.exit(0 if un_total == 0 and read == len(names) else 1)


if __name__ == "__main__":
    main()
