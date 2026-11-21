#!/usr/bin/env python3
"""judge_plants: is the judge blind?  A good plan with one error planted in it must fail, for the right reason.

    judge_plants.py [--astar BIN] [--board-dir DIR] [--check-only]

DIR holds NAME.kicad_pcb and scene.json (default: the repo's dock, corpus/boards/dock). The board is routed once; the plan must
pass the judge as it is, and then fail it three times:

    a planted open        one signal net's copper taken out        -> that net named open, nothing else new
    a planted short       a track from one net's pad to another's  -> new shorting_items
    a planted clearance   a track 0.03 mm beside another net's     -> new clearance

Each of the four plans is also given to astar's own exact check (`astar check`, the third judge), which must
say the same: clean; open-unsaid for that net; short; clearance. --check-only asks it alone, without KiCad.

One line per check; exit 0 when all hold, 1 when one does not, 2 when the test itself could not run.
"""
import argparse, copy, json, math, os, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import judge as J


def planted_open(plan, scene):
    planes = {q["net"] for q in scene["stack"]["planes"]} | {q["net"] for q in plan.get("planes", [])}
    p = copy.deepcopy(plan)
    for n in p["nets"]:
        if n["name"] not in planes and n.get("tracks"):
            name = n["name"]
            p["nets"].remove(n)
            return p, name
    raise RuntimeError("no signal net in the plan to open")


def planted_short(plan, scene):
    front = [q for q in scene["pads"] if q.get("net") and not q.get("tht") and q.get("side") != "B"]
    a = front[0]
    b = min((q for q in front if q["net"] != a["net"]), key=lambda q: math.dist(q["at"], a["at"]))
    p = copy.deepcopy(plan)
    net = next((n for n in p["nets"] if n["name"] == a["net"]), None)
    if net is None:
        net = {"name": a["net"], "tracks": [], "vias": []}
        p["nets"].append(net)
    net["tracks"].append({"layer": "F.Cu", "width": scene["rules"]["track"], "path": [{"line": a["at"] + b["at"]}]})
    return p, f"{a['net']} to {b['net']}"


def planted_clearance(plan, scene):
    p = copy.deepcopy(plan)
    for n in p["nets"]:
        for t in n.get("tracks", []):
            for seg in t["path"]:
                if "line" not in seg:
                    continue
                x0, y0, x1, y1 = seg["line"]
                d = math.hypot(x1 - x0, y1 - y0)
                if d < 1.0:
                    continue
                # a short piece of another net, beside the middle of this run, 0.03 mm of air between them
                off = t["width"] + 0.03
                nx, ny = -(y1 - y0) / d * off, (x1 - x0) / d * off
                ux, uy = (x1 - x0) / d, (y1 - y0) / d
                mx, my = (x0 + x1) / 2, (y0 + y1) / 2
                other = next(m for m in p["nets"] if m["name"] != n["name"])
                other.setdefault("tracks", []).append({"layer": t["layer"], "width": t["width"], "path": [
                    {"line": [mx - 0.2 * ux + nx, my - 0.2 * uy + ny, mx + 0.2 * ux + nx, my + 0.2 * uy + ny]}]})
                return p, f"{other['name']} beside {n['name']}"
    raise RuntimeError("no straight run of 1 mm in the plan")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--astar", default=os.path.join(HERE, "..", "target", "release", "astar"))
    ap.add_argument("--board-dir", default=os.path.join(HERE, "..", "corpus", "boards", "dock"))
    ap.add_argument("--check-only", action="store_true", help="astar's own check alone, without KiCad (the light gate)")
    a = ap.parse_args()
    bd = os.path.abspath(a.board_dir)
    board = os.path.join(bd, os.path.basename(bd) + ".kicad_pcb")
    scene_path = os.path.join(bd, "scene.json")
    for f in (a.astar, board, scene_path):
        if not os.path.exists(f):
            print(f"judge_plants: BROKEN: {f} is not there")
            return 2
    bad = 0
    with tempfile.TemporaryDirectory(prefix="astar-plants-") as tmp:
        plan_path = os.path.join(tmp, "plan.json")
        r = subprocess.run([a.astar, "route", scene_path, plan_path], capture_output=True, text=True)
        if r.returncode not in (0, 1, 3) or not os.path.exists(plan_path):
            print(f"judge_plants: BROKEN: astar exit {r.returncode}: {r.stderr[-200:]}")
            return 2
        scene, plan = json.load(open(scene_path)), json.load(open(plan_path))
        cache = os.path.join(tmp, "bare-drc.json")

        checked = {}

        def run(p, tag):
            f = os.path.join(tmp, tag + ".json")
            json.dump(p, open(f, "w"))
            c = subprocess.run([a.astar, "check", scene_path, f], capture_output=True, text=True)
            checked[tag] = (c.returncode, sorted({ln.split(" ", 1)[0] for ln in c.stdout.split("\n") if ln.strip()}), c.stdout)
            if a.check_only:
                return None
            return J.judge(scene_path, f, board, cache=cache)

        def say(ok, what, detail):
            nonlocal bad
            bad += not ok
            print(f"{'ok  ' if ok else 'FAIL'} {what}: {detail}")

        try:
            row = run(plan, "good")
            if row:
                say(row["pass"], "the plan as routed passes", f"landed {row['landed']}/{row['nets']}, new DRC {row['drc_new']}")
            say(checked["good"][0] == 0, "astar check: the plan as routed is clean", f"exit {checked['good'][0]} {checked['good'][1] or ''}")
            p, what = planted_open(plan, scene)
            row = run(p, "open")
            if row:
                say(not row["pass"] and row["unlanded_nets"] == [what] and not row["drc_new"], "a planted open is caught",
                    f"{what} taken out -> open {row['unlanded_nets']}, new DRC {row['drc_new']}")
            code, kinds, out = checked["open"]
            say(code == 1 and kinds == ["open-unsaid"] and out.startswith(f"open-unsaid {what} "), "astar check: a planted open is caught",
                f"{what} taken out -> {kinds}")
            p, what = planted_short(plan, scene)
            row = run(p, "short")
            if row:
                say(not row["pass"] and row["drc_new"].get("shorting_items", 0) > 0, "a planted short is caught",
                    f"{what} -> new DRC {row['drc_new']}")
            code, kinds, out = checked["short"]
            say(code == 1 and "short" in kinds, "astar check: a planted short is caught", f"{what} -> {kinds}")
            p, what = planted_clearance(plan, scene)
            row = run(p, "clearance")
            if row:
                say(not row["pass"] and row["drc_new"].get("clearance", 0) > 0, "a planted clearance error is caught",
                    f"{what} -> new DRC {row['drc_new']}")
            code, kinds, out = checked["clearance"]
            say(code == 1 and "clearance" in kinds, "astar check: a planted clearance error is caught", f"{what} -> {kinds}")
        except Exception as e:
            print(f"judge_plants: BROKEN: {type(e).__name__}: {e}")
            return 2
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
