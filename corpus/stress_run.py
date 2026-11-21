#!/usr/bin/env python3
"""stress_run: astar over KRT's stress boards as fetched by corpus/fetch_stress.py, each judged by KiCad.

    stress_run.py [--budget 20] [--jobs 2] [--out corpus/out/stress.jsonl] [--astar BIN] [NAME ...]

Each fetched board (CACHE/stress/NAME/NAME.kicad_pcb) is brought to the house KiCad's format, stripped of
its routing (prepare.make_bare), read by
astar's own reader, routed, and judged like any corpus board (run.run_one with --own-scene). Rows already in
the output file are kept, so a stopped run picks up where it was. Third-party boards: nothing here is committed
but the rows.
"""
import argparse, concurrent.futures as cf, glob, json, os, shutil, subprocess, sys, re
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import prepare, run


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("names", nargs="*")
    ap.add_argument("--budget", type=float, default=20)
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--out", default=os.path.join(HERE, "out", "stress.jsonl"))
    ap.add_argument("--astar", default=os.path.join(HERE, "..", "target", "release", "astar"))
    a = ap.parse_args()
    run.OWN_SCENE = True
    rev = subprocess.run(["git", "-C", HERE, "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    done = set()
    if os.path.exists(a.out):
        done = {json.loads(l)["board"] for l in open(a.out) if l.strip()}
    todo = []
    for src in sorted(glob.glob(os.path.join(run.CACHE, "stress", "*", "*.kicad_pcb"))):
        name = os.path.basename(src)[:-len(".kicad_pcb")]
        if name in done or (a.names and name not in a.names):
            continue
        todo.append((name, src))
    os.makedirs(os.path.dirname(a.out), exist_ok=True)

    def one(item):
        name, src = item
        bd = os.path.join(run.CACHE, "boards", name)
        os.makedirs(bd, exist_ok=True)
        bare = os.path.join(bd, name + ".kicad_pcb")
        try:
            text = open(src, errors="replace").read()
            if not os.path.exists(bare):
                # to the house KiCad's file format first: make_bare reads that format only, and a board
                # left in an older one keeps its routing
                shutil.copy(src, bare)
                subprocess.run([prepare.J.kicad_cli(), "pcb", "upgrade", "--force", bare], capture_output=True, timeout=300)
                made = prepare.make_bare(open(bare, errors="replace").read())[0]
                if re.search(r"^\s*\((segment|via)\b", made, re.M):
                    os.remove(bare)
                    return {"board": name, "source": "stress", "status": "not prepared", "why": "the board keeps tracks after stripping"}
                open(bare, "w").write(made)
                for ext in (".kicad_pro", ".kicad_dru"):
                    if os.path.exists(src[:-len(".kicad_pcb")] + ext):
                        shutil.copy(src[:-len(".kicad_pcb")] + ext, bd)
            layers = len(set(re.findall(r'\(\d+ "?([FB]|In\d+)\.Cu"? (?:signal|power|mixed|jumper)', text)))
            entry = {"name": name, "source": "stress", "copper_layers": layers, "nets": None, "scene": True}
            return run.run_one(entry, a.astar, a.budget, rev, 3)
        except Exception as e:  # a board that breaks the harness is a row, not a stop
            return {"board": name, "source": "stress", "status": "judge failed", "why": f"{type(e).__name__}: {e}"[:200]}

    with cf.ThreadPoolExecutor(a.jobs) as ex, open(a.out, "a") as f:
        for row in ex.map(one, todo):
            f.write(json.dumps(row) + "\n")
            f.flush()
    print(f"{len(todo)} boards run; rows in {a.out}")


if __name__ == "__main__":
    main()
