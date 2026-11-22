#!/usr/bin/env python3
"""touch_judge.py: KiCad and astar agree on what a board's own copper joins.

    tests/touch_judge.py [--astar BIN] [--keep DIR]

For tests/scenes/old_touch.json (a tie whose round end lies on a pad, a track over a pad, two tracks that
cross, pads that lie on each other, and a tie that stops just clear): the scene is written as a bare KiCad
board (tests/scene_board.py), KiCad's DRC says which nets are not joined, and astar routes the scene. The
nets astar lays copper for must be exactly the nets KiCad calls unconnected. Needs kicad-cli (KICAD_CLI or
PATH). Exit 0 they agree, 1 they do not, 2 it could not be run.
"""
import json, os, re, shutil, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SCENES = ["old_touch.json"]


def main():
    args = sys.argv[1:]
    astar = os.path.join(HERE, "..", "target", "release", "astar")
    keep = None
    while args:
        a = args.pop(0)
        if a == "--astar":
            astar = args.pop(0)
        elif a == "--keep":
            keep = args.pop(0)
        else:
            print(__doc__)
            return 2
    cli = os.environ.get("KICAD_CLI") or shutil.which("kicad-cli")
    if not cli:
        print("touch_judge: no kicad-cli (set KICAD_CLI)")
        return 2
    tmp = keep or tempfile.mkdtemp(prefix="astar-touch-")
    os.makedirs(tmp, exist_ok=True)
    bad = 0
    for name in SCENES:
        scene = os.path.join(HERE, "scenes", name)
        stem = os.path.join(tmp, name[:-5])
        subprocess.run([sys.executable, os.path.join(HERE, "scene_board.py"), scene, stem + ".kicad_pcb"], check=True)
        r = subprocess.run([cli, "pcb", "drc", "--severity-all", "--format", "json", "-o", stem + ".drc.json", stem + ".kicad_pcb"], capture_output=True, text=True)
        if not os.path.exists(stem + ".drc.json"):
            print(f"touch_judge: kicad-cli gave no report for {name}: {r.stderr.strip()[:200]}")
            return 2
        drc = json.load(open(stem + ".drc.json"))
        kicad = set()
        for u in drc.get("unconnected_items", []):
            for i in u["items"]:
                m = re.search(r"\[([^\]]+)\]", i["description"])
                if m:
                    kicad.add(m.group(1))
        subprocess.run([astar, "route", scene, stem + ".plan.json"], capture_output=True, text=True)
        plan = json.load(open(stem + ".plan.json"))
        ours = {n["name"] for n in plan["nets"] if n["tracks"] or n["vias"]}
        if kicad == ours:
            print(f"ok   {name}: KiCad and astar agree, not joined as the board stands: {', '.join(sorted(ours)) or 'none'}")
        else:
            bad += 1
            print(f"FAIL {name}: KiCad says not joined: {sorted(kicad)}; astar lays copper for: {sorted(ours)}")
    if not keep:
        shutil.rmtree(tmp, ignore_errors=True)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
