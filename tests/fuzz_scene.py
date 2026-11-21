#!/usr/bin/env python3
"""fuzz_scene: random small valid scenes through astar (T13.6, the skeleton).

    fuzz_scene.py [--n 100] [--seed 1] [--seconds 10] [--keep DIR] [--only K]

Each case is a board of a few parts made from the seed and the case number alone, so `--seed S --only K` makes
the same scene again. A scene has: an outline (a rectangle, sometimes with round corners), two or four copper
layers, rules drawn from sane ranges, zones as real boards have them (a ground plane on an inner layer, ground
poured on an outer layer or on two, a rail's zone on any layer), 2 to 9 parts (chips, two-row packages
down to 0.5 mm pitch, four-sided packages, through-hole headers) put down without touching, on the 0.1 mm grid
or off it, turned by quarter turns or by any angle, some on the back; nets of 2 to 5 pads, sometimes a ground
net and a wide rail; holes; a keep-out. Nothing is on the board yet (`copper` is empty).

What must hold for every case:
    no crash        astar exits 0, 1 (nets open) or 3 and writes a plan that is JSON (exit 2, "scene refused", on a valid scene counts)
    in time         it ends within its --seconds (plus 30 s of grace)
    well formed     every track is on a layer of the scene, every number is finite, every net is one the scene has
    same in, same out   a second run gives the same copper (when neither run was stopped by its time budget)
    the copper is right the exact check (`astar check`) finds every net the plan calls joined joined, and the plan's
                    copper clear of other nets' copper, of holes, of the edge and of keep-outs (--no-check: not asked)

A line a finding, then a count. Exit 0 when nothing was found, 1 when something was. The scene of every
finding is kept (in --keep DIR, else a temp folder that is named).  It does not judge the copper: whether a
net called joined is joined, and clearance, are the exact check's (T13.7).
"""
import argparse, json, math, os, random, re, shutil, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))


def rot(x, y, deg):
    c, s = math.cos(math.radians(deg)), math.sin(math.radians(deg))
    return x * c - y * s, x * s + y * c


def r4(v):
    return round(v, 4)


def part_pads(rng, kind, clearance):
    """a part in its own frame: [(pad number, x, y, w, h, shape kind, through-hole)]"""
    pads = []
    if kind == "chip":
        L = rng.choice([1.0, 1.6, 2.0, 3.2])
        w, h = L * 0.35, L * 0.5
        pads = [("1", -L / 2 + w / 2, 0, w, h, "rect", False), ("2", L / 2 - w / 2, 0, w, h, "rect", False)]
    elif kind == "two_row":
        n = rng.choice([3, 4, 7, 8, 10])
        pitch = rng.choice([1.27, 0.95, 0.65, 0.5])
        w = min(0.6 * pitch, pitch - clearance - 0.05)
        span = rng.choice([2.0, 3.0, 5.4])
        for k in range(n):
            x = (k - (n - 1) / 2) * pitch
            pads.append((str(k + 1), x, span / 2, w, 1.2, rng.choice(["rect", "oval"]), False))
            pads.append((str(2 * n - k), x, -span / 2, w, 1.2, "rect", False))
    elif kind == "four_side":
        n = rng.choice([4, 6, 8, 12])
        pitch = rng.choice([0.8, 0.65, 0.5, 0.4])
        w = min(0.6 * pitch, pitch - clearance - 0.05)
        half = n * pitch / 2 + 0.9
        k = 1
        for side in range(4):
            for i in range(n):
                x, y = rot((i - (n - 1) / 2) * pitch, half, side * 90)
                pw, ph = (w, 0.8) if side % 2 == 0 else (0.8, w)
                pads.append((str(k), x, y, pw, ph, "rect", False))
                k += 1
        if rng.random() < 0.5:
            e = max(0.5, half * 2 - 3.2)
            pads.append((str(k), 0, 0, e, e, "rect", False))
    elif kind == "header":
        n = rng.choice([2, 3, 4, 6])
        rows = rng.choice([1, 1, 2])
        k = 1
        for i in range(n):
            for j in range(rows):
                pads.append((str(k), (i - (n - 1) / 2) * 2.54, (j - (rows - 1) / 2) * 2.54, 1.7, 1.7, rng.choice(["circle", "oval"]), True))
                k += 1
    return pads


def shape_of(kind, x, y, w, h, deg):
    if kind == "circle":
        return {"circle": [r4(x), r4(y), r4(min(w, h) / 2)]}
    if kind == "oval":
        r = min(w, h) / 2
        dx, dy = rot((w - h) / 2 if w >= h else 0, (h - w) / 2 if h > w else 0, deg)
        if abs(dx) + abs(dy) < 1e-9:
            return {"circle": [r4(x), r4(y), r4(r)]}
        return {"oval": [r4(x - dx), r4(y - dy), r4(x + dx), r4(y + dy), r4(r)]}
    pts = []
    for cx, cy in ((-w / 2, -h / 2), (w / 2, -h / 2), (w / 2, h / 2), (-w / 2, h / 2)):
        px, py = rot(cx, cy, deg)
        pts.append([r4(x + px), r4(y + py)])
    return {"poly": pts, "r": 0}


def make_scene(seed, k):
    rng = random.Random(seed * 1_000_003 + k)
    W, H = r4(rng.uniform(14, 60)), r4(rng.uniform(12, 40))
    x0, y0 = r4(rng.uniform(0, 120)), r4(rng.uniform(0, 80))
    if rng.random() < 0.5:                     # the board itself on the grid
        x0, y0, W, H = round(x0, 1), round(y0, 1), round(W, 1), round(H, 1)
    four = rng.random() < 0.5
    layers = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"] if four else ["F.Cu", "B.Cu"]
    clearance = r4(rng.choice([0.1, 0.127, 0.15, 0.2, rng.uniform(0.1, 0.25)]))
    via = r4(rng.choice([0.45, 0.6, 0.8, rng.uniform(0.4, 0.8)]))
    rules = {"track": r4(rng.choice([0.127, 0.15, 0.2, 0.25, rng.uniform(0.1, 0.3)])), "clearance": clearance, "via": via,
             "drill": r4(max(0.15, via - 2 * rng.uniform(0.1, 0.15))), "edge": r4(rng.uniform(0.2, 0.5)),
             "hole_clearance": r4(rng.uniform(0.127, 0.25))}
    # the outline: a rectangle, one time in three with round corners (arcs)
    cr = r4(rng.uniform(1, 3)) if rng.random() < 0.33 else 0
    x1, y1 = r4(x0 + W), r4(y0 + H)
    if cr:
        q = cr * (1 - math.sqrt(0.5))
        outline = [{"line": [r4(x0 + cr), y0, r4(x1 - cr), y0]}, {"arc": [r4(x1 - cr), y0, r4(x1 - q), r4(y0 + q), x1, r4(y0 + cr)]},
                   {"line": [x1, r4(y0 + cr), x1, r4(y1 - cr)]}, {"arc": [x1, r4(y1 - cr), r4(x1 - q), r4(y1 - q), r4(x1 - cr), y1]},
                   {"line": [r4(x1 - cr), y1, r4(x0 + cr), y1]}, {"arc": [r4(x0 + cr), y1, r4(x0 + q), r4(y1 - q), x0, r4(y1 - cr)]},
                   {"line": [x0, r4(y1 - cr), x0, r4(y0 + cr)]}, {"arc": [x0, r4(y0 + cr), r4(x0 + q), r4(y0 + q), r4(x0 + cr), y0]}]
    else:
        outline = [{"line": [x0, y0, x1, y0]}, {"line": [x1, y0, x1, y1]}, {"line": [x1, y1, x0, y1]}, {"line": [x0, y1, x0, y0]}]
    # parts, put down without touching: a part's box (its pads' reach) stays 1 mm from every other's and from the edge
    pads, parts, boxes = [], [], []
    margin = rules["edge"] + 1.0 + cr
    for n in range(rng.randint(2, 9)):
        kind = rng.choice(["chip", "chip", "chip", "two_row", "two_row", "four_side", "header"])
        local = part_pads(rng, kind, clearance)
        deg = rng.choice([0, 90, 180, 270, 0, 90, rng.uniform(0, 360)])
        reach = max(math.hypot(abs(x) + w / 2, abs(y) + h / 2) for _, x, y, w, h, _, _ in local)
        for _ in range(60):
            cx, cy = rng.uniform(x0 + margin + reach, x1 - margin - reach) if W > 2 * (margin + reach) else None, \
                     rng.uniform(y0 + margin + reach, y1 - margin - reach) if H > 2 * (margin + reach) else None
            if cx is None or cy is None:
                break
            if rng.random() < 0.6:             # on the 0.1 mm grid, or anywhere
                cx, cy = round(cx, 1), round(cy, 1)
            cx, cy = r4(cx), r4(cy)
            if all(math.hypot(cx - bx, cy - by) > reach + br + 1.0 for bx, by, br in boxes):
                break
        else:
            continue
        if cx is None or cy is None:
            continue
        boxes.append((cx, cy, reach))
        ref = {"chip": "R", "two_row": "U", "four_side": "U", "header": "J"}[kind] + str(n + 1)
        back = kind != "header" and rng.random() < 0.2
        for num, x, y, w, h, sk, tht in local:
            px, py = rot(x, y, deg)
            p = {"ref": ref, "num": num, "net": "", "at": [r4(cx + px), r4(cy + py)], "fp": [cx, cy], "size": [r4(w), r4(h)], "tht": tht,
                 "shape": shape_of(sk, cx + px, cy + py, w, h, deg)}
            if back:
                p["side"] = "B"
            pads.append(p)
        parts.append({"ref": ref, "courtyard": [[r4(cx - reach), r4(cy - reach)], [r4(cx + reach), r4(cy - reach)],
                                               [r4(cx + reach), r4(cy + reach)], [r4(cx - reach), r4(cy + reach)]]})
    # nets: a ground and a rail some of the time, then signals of 2 to 5 pads; some pads stay without a net
    free = list(range(len(pads)))
    rng.shuffle(free)
    nets, planes = [], []

    def take(name, n):
        got = [free.pop() for _ in range(min(n, len(free)))]
        for i in got:
            pads[i]["net"] = name
        return len(got)

    if rng.random() < 0.6 and len(free) >= 4:
        take("GND", rng.randint(2, max(2, len(free) // 4)))
        nets.append({"name": "GND", "class": "ground"})
        if four and rng.random() < 0.7:
            planes.append({"net": "GND", "layer": "In1.Cu"})
        # as real boards have them: ground poured on an outer layer, or on two layers
        if rng.random() < 0.25:
            for L in rng.sample(layers, rng.randint(1, 2)):
                if {"net": "GND", "layer": L} not in planes:
                    planes.append({"net": "GND", "layer": L})
    if rng.random() < 0.4 and len(free) >= 4:
        take("+5V", rng.randint(2, 4))
        nets.append({"name": "+5V", "class": "rail", "width": r4(max(rules["track"], rng.choice([0.3, 0.4, 0.5])))})
        if rng.random() < 0.25:                # a rail's zone, on any layer, beside whatever ground has
            planes.append({"net": "+5V", "layer": rng.choice(layers)})
    k_net = 0
    while len(free) >= 2 and rng.random() < 0.93:
        k_net += 1
        name = rng.choice(["N", "/sheet/N", "D+", "Net-(U1-Pad"]) + str(k_net)
        take(name, rng.randint(2, 5))
        net = {"name": name, "class": "signal"}
        if rng.random() < 0.05:
            net["clearance"] = r4(clearance + 0.1)
        nets.append(net)
    # holes and a keep-out, where no part is
    holes, keepouts = [], []
    for _ in range(rng.randint(0, 3)):
        d = rng.choice([1.0, 2.2, 3.2])
        hx, hy = r4(rng.uniform(x0 + d + cr, x1 - d - cr)), r4(rng.uniform(y0 + d + cr, y1 - d - cr))
        if all(math.hypot(hx - bx, hy - by) > br + d / 2 + 0.5 for bx, by, br in boxes):
            holes.append([hx, hy, d])
            boxes.append((hx, hy, d / 2))
    if rng.random() < 0.3:
        kw, kh = rng.uniform(1, 5), rng.uniform(1, 5)
        kx, ky = rng.uniform(x0 + 1, x1 - kw - 1), rng.uniform(y0 + 1, y1 - kh - 1)
        if all(math.hypot(kx + kw / 2 - bx, ky + kh / 2 - by) > br + math.hypot(kw, kh) / 2 for bx, by, br in boxes):
            keepouts.append({"layers": rng.sample(layers, rng.randint(1, len(layers))),
                             "poly": [[r4(kx), r4(ky)], [r4(kx + kw), r4(ky)], [r4(kx + kw), r4(ky + kh)], [r4(kx), r4(ky + kh)]]})
    return {"bench": {"contract": "1.1", "board": f"fuzz-{seed}-{k}", "exported_by": "tests/fuzz_scene.py"},
            "stack": {"copper": layers, "planes": planes}, "version": 1, "units": "mm", "outline": outline, "rules": rules,
            "nets": nets, "pads": pads, "holes": holes, "keepouts": keepouts, "copper": [], "parts": parts}


def copper(plan):
    return json.dumps({k: plan.get(k) for k in ("nets", "planes", "left_out", "moves")}, sort_keys=True)


def run(astar, scene_path, plan_path, seconds):
    """(plan or None, what went wrong or "")"""
    if os.path.exists(plan_path):
        os.remove(plan_path)
    p = subprocess.Popen(["nice", astar, "route", scene_path, plan_path, "--seconds", str(seconds)], stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, start_new_session=True)
    try:
        out, _ = p.communicate(timeout=seconds + 30)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, 9)
        p.communicate()
        return None, f"not in time: still going {seconds + 30} s in"
    lines = [ln.strip() for ln in (out or "").split("\n") if ln.strip()]
    if p.returncode not in (0, 1, 3):          # astar route: 1 nets open, 3 narrowed: a plan each time; 2 refused
        panic = next((ln for ln in lines if "panicked at" in ln or "astar: BUG" in ln), "")
        after = lines[lines.index(panic) + 1] if panic and lines.index(panic) + 1 < len(lines) else ""
        how = f"signal {-p.returncode}" if p.returncode < 0 else f"exit {p.returncode}"
        kind = "crash" if panic or p.returncode != 2 else "scene refused"
        return None, f"{kind} ({how}): {(panic + ' ' + after).strip() or (lines[-1] if lines else '')}"[:400]
    try:
        return json.load(open(plan_path)), ""
    except (OSError, ValueError) as e:
        return None, f"crash: exit {p.returncode} but the plan does not read: {e}"


def checked(astar, scene_path, plan_path):
    """what astar's own exact check (`astar check`, src/check/) finds in the plan beyond the nets the plan itself
    says it left out, or "": the kinds found, and the first finding of the first kind"""
    r = subprocess.run([astar, "check", scene_path, plan_path], capture_output=True, text=True)
    if r.returncode == 0:
        return ""
    if r.returncode != 1:
        return f"the check could not run (exit {r.returncode}): {r.stderr.strip()[-200:]}"
    lines = [ln for ln in r.stdout.split("\n") if ln.strip() and not ln.startswith("open ")]
    if not lines:
        return ""
    kinds = sorted({ln.split(" ", 1)[0] for ln in lines})
    return f"the check finds {', '.join(kinds)}: {lines[0][:200]}"


def ill_formed(plan, scene):
    """what is wrong with the plan's form, or """""
    layers = set(scene["stack"]["copper"])
    names = {n["name"] for n in scene["nets"]} | {p["net"] for p in scene["pads"] if p["net"]}
    for n in plan.get("nets", []):
        if n.get("name") not in names:
            return f"copper on a net the scene does not have: {n.get('name')!r}"
        for t in n.get("tracks", []):
            if t.get("layer") not in layers:
                return f"a track of {n['name']} on {t.get('layer')!r}, not a layer of the scene"
            if not (isinstance(t.get("width"), (int, float)) and math.isfinite(t["width"]) and t["width"] > 0):
                return f"a track of {n['name']} of width {t.get('width')!r}"
            for seg in t.get("path", []):
                v = seg.get("line") or seg.get("arc") or []
                if len(v) not in (4, 6) or not all(isinstance(x, (int, float)) and math.isfinite(x) for x in v):
                    return f"a track of {n['name']} with a piece that is not a line or an arc of finite numbers: {seg}"
        for v in n.get("vias", []):
            at = v.get("at") or []
            if len(at) != 2 or not all(isinstance(x, (int, float)) and math.isfinite(x) for x in at):
                return f"a via of {n['name']} at {at!r}"
    for q in plan.get("planes", []):
        if q.get("layer") not in layers or q.get("net") not in names:
            return f"a plane {q.get('net')!r} on {q.get('layer')!r}"
    return ""


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--n", type=int, default=100)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--seconds", type=float, default=10)
    ap.add_argument("--only", type=int, help="one case number")
    ap.add_argument("--keep", help="where the scenes of the findings go")
    ap.add_argument("--quiet", action="store_true", help="no line a finding: only the count of each kind, its smallest case, and the total")
    ap.add_argument("--no-check", action="store_true", help="do not ask astar check about the copper")
    ap.add_argument("--astar", default=os.environ.get("ASTAR") or os.path.join(HERE, "..", "target", "release", "astar"))
    a = ap.parse_args()
    if not os.path.exists(a.astar):
        print(f"fuzz_scene: BROKEN: no binary at {a.astar}")
        return 2
    tmp = tempfile.mkdtemp(prefix="astar-fuzz-")
    keep = a.keep or os.path.join(tmp, "found")
    found = []
    t0 = time.time()
    cases = [a.only] if a.only is not None else range(a.n)
    joined = asked = cut = 0
    kinds = {}
    for k in cases:
        scene = make_scene(a.seed, k)
        sp, pp = os.path.join(tmp, "scene.json"), os.path.join(tmp, "plan.json")
        json.dump(scene, open(sp, "w"))
        plan, why = run(a.astar, sp, pp, a.seconds)
        if plan is not None:
            why = ill_formed(plan, scene)
            if why:
                why = "ill formed: " + why
        if plan is not None and not why:
            again, why = run(a.astar, sp, pp, a.seconds)
            # a run stopped by its time budget ends where the clock says: only whole runs are compared
            whole = lambda p: (p.get("timing_s") or {}).get("total", 0) < 0.8 * a.seconds
            if again is not None and not (whole(plan) and whole(again)):
                cut += 1
            elif again is not None and copper(again) != copper(plan):
                why = "same in, not the same out: a second run gave other copper"
        if plan is not None and not why and not a.no_check:
            why = checked(a.astar, sp, pp)
        if plan is not None:
            n = sum(1 for q in scene["nets"])
            asked += n
            joined += n - len(plan.get("left_out", []))
        if why:
            os.makedirs(keep, exist_ok=True)
            f = os.path.join(keep, f"fuzz-{a.seed}-{k}.json")
            shutil.copy(sp, f)
            found.append((k, why))
            kind = re.sub(r"\(\d+\) ", "", why)[:150]           # the same finding whatever the thread's number
            if why.startswith("the check finds"):
                kind = why.split(":", 1)[0]
            kinds.setdefault(kind, []).append((len(scene["pads"]), k))
            if not a.quiet:
                print(f"FOUND seed {a.seed} case {k} ({len(scene['stack']['copper'])} layers, {len(scene['pads'])} pads, {len(scene['nets'])} nets): {why}\n      scene: {f}", flush=True)
    n = len(list(cases))
    for kind, cs in sorted(kinds.items(), key=lambda kv: -len(kv[1])):
        print(f"  {len(cs):4d} x {kind}\n         the smallest: case {min(cs)[1]} ({min(cs)[0]} pads), {os.path.join(keep, f'fuzz-{a.seed}-{min(cs)[1]}.json')}")
    print(f"fuzz_scene: {n} scenes (seed {a.seed}), {len(found)} findings, {time.time() - t0:.0f} s; {cut} stopped by the {a.seconds:g} s budget "
          f"(not compared run to run); by astar's own count {joined} of {asked} nets joined")
    if not found:
        shutil.rmtree(tmp, ignore_errors=True)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
