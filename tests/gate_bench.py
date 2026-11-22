#!/usr/bin/env python3
"""gate_bench: the bench five for gate.sh. One line a check; exit 1 when a check fails, 2 when it could not run.

    gate_bench.py            route dock g474 ccb tb150 ub400: every net joined by astar's own count, and
                             by astar's own exact check (`astar check`: joined, and the copper clear);
                             one board routed twice: the same copper
    gate_bench.py --judge    and each plan through tests/judge.py: every net landed, 0 new DRC errors

The boards: $ASTAR_BENCH_BOARDS, else corpus/boards, else /home/claude/router-bench/boards (each NAME/scene.json
and NAME/NAME.kicad_pcb). The binary: $ASTAR, else target/release/astar. The bare boards' DRC is kept in
target/gate/, named by the board's hash. Plans and kept boards go to a temp folder, removed when all is well.
"""
import argparse, concurrent.futures as cf, hashlib, json, os, shutil, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import judge as J

FIVE = ["dock", "g474", "ccb", "tb150", "ub400"]
TWICE = "tb150"


def boards_dir():
    """the folder with the most of the five: the one named, the repo's own corpus, a bench checkout. The repo
    carries dock and g474; ccb, tb150 and ub400 come from a router-bench checkout where there is one."""
    spots = [os.environ.get("ASTAR_BENCH_BOARDS"), os.path.join(ROOT, "corpus", "boards"), "/home/claude/router-bench/boards",
             os.path.expanduser("~/router-bench/boards"), os.path.join(ROOT, "..", "router-bench", "boards")]
    have = lambda d: [b for b in FIVE if os.path.exists(os.path.join(d, b, "scene.json")) and os.path.exists(os.path.join(d, b, b + ".kicad_pcb"))]
    if spots[0]:
        return spots[0] if os.path.isdir(spots[0]) else None      # a folder named is the one used
    spots = [d for d in spots if d and os.path.isdir(d)]
    return max(spots, key=lambda d: len(have(d)), default=None)


def copper(plan):
    """what a plan lays, without its timing and its own summary"""
    return json.dumps({k: plan.get(k) for k in ("nets", "planes", "left_out", "moves")}, sort_keys=True)


def route(astar, scene, plan):
    t0 = time.time()
    if os.path.exists(plan):
        os.remove(plan)
    r = subprocess.run(["nice", astar, "route", scene, plan], capture_output=True, text=True)
    dt = time.time() - t0
    if r.returncode not in (0, 1, 3, 5) or not os.path.exists(plan):   # 1: nets open, 3: narrowed, 5: a fault the board came with; a plan each time
        return None, dt, f"astar exit {r.returncode}: {(r.stderr or r.stdout).strip()[-200:]}"
    return json.load(open(plan)), dt, ""


def check(astar, scene, plan):
    """(clean, what astar's own exact check says, seconds): `astar check` on a plan file"""
    t0 = time.time()
    r = subprocess.run([astar, "check", scene, plan, "--max", "3"], capture_output=True, text=True)
    said = (r.stderr.strip().split("\n") or [""])[-1].replace("astar check: ", "")
    first = r.stdout.strip().split("\n")[0] if r.stdout.strip() else ""
    return r.returncode == 0, said + (f"; the first: {first[:160]}" if first else ""), time.time() - t0


def file_hash(board):
    h = hashlib.sha1()
    for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
        f = board[:-len(".kicad_pcb")] + ext
        if os.path.exists(f):
            h.update(open(f, "rb").read())
    return h.hexdigest()[:12]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--judge", action="store_true")
    ap.add_argument("--jobs", type=int, default=2)
    a = ap.parse_args()
    astar = os.environ.get("ASTAR") or os.path.join(ROOT, "target", "release", "astar")
    bd = boards_dir()
    if not os.path.exists(astar) or not bd:
        print(f"FAIL bench five: {'no binary at ' + astar if not os.path.exists(astar) else 'no bench boards found (set ASTAR_BENCH_BOARDS)'}")
        return 2
    tmp = tempfile.mkdtemp(prefix="astar-gate-")
    bad = 0
    plans = {}
    five = [b for b in FIVE if os.path.exists(os.path.join(bd, b, "scene.json"))]
    if len(five) < len(FIVE):
        print(f"ok   skipped, not in this checkout: {', '.join(b for b in FIVE if b not in five)}")
    if not five:
        print("FAIL bench five: none of them found (set ASTAR_BENCH_BOARDS)")
        return 2
    twice = TWICE if TWICE in five else five[-1]
    for b in five:
        scene = os.path.join(bd, b, "scene.json")
        plan, dt, err = route(astar, scene, os.path.join(tmp, b + ".json"))
        if plan is None:
            print(f"FAIL route {b:6s} {err}")
            bad += 1
            continue
        nets = len(J.routable_nets(json.load(open(scene))))
        left = plan.get("left_out", [])
        ok = not left
        bad += not ok
        plans[b] = plan
        s = (plan.get("timing_s") or {}).get("total", dt)
        print(f"{'ok  ' if ok else 'FAIL'} route {b:6s} {nets - len(left)}/{nets} joined by astar's count, {s:.2f} s"
              + ("" if ok else f": open {', '.join(x['net'] for x in left[:6])}"))
        clean, said, dt = check(astar, scene, os.path.join(tmp, b + ".json"))
        bad += not clean
        print(f"{'ok  ' if clean else 'FAIL'} check {b:6s} {said} ({dt:.2f} s)")
    if twice in plans:
        again, dt, err = route(astar, os.path.join(bd, twice, "scene.json"), os.path.join(tmp, twice + "-again.json"))
        ok = again is not None and copper(again) == copper(plans[twice])
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'} same in, same out: {twice} routed twice, {'the same copper' if ok else err or 'THE COPPER DIFFERS'}")
    if a.judge:
        cache_dir = os.path.join(ROOT, "target", "gate")
        os.makedirs(cache_dir, exist_ok=True)

        def one(b):
            board = os.path.join(bd, b, b + ".kicad_pcb")
            cache = os.path.join(cache_dir, f"bare-drc-{b}-{file_hash(board)}.json")
            try:
                return J.judge(os.path.join(bd, b, "scene.json"), os.path.join(tmp, b + ".json"), board, cache=cache,
                               keep=os.path.join(tmp, "judged-" + b), name=b)
            except Exception as e:
                return {"board": b, "pass": False, "why": f"the judge could not run: {type(e).__name__}: {e}"}

        with cf.ThreadPoolExecutor(max(1, min(2, a.jobs))) as ex:
            for row in ex.map(one, [b for b in FIVE if b in plans]):
                ok = row["pass"]
                bad += not ok
                if "nets" in row:
                    print(f"{'ok  ' if ok else 'FAIL'} judge {row['board']:6s} {row['landed']}/{row['nets']} landed, {row['drc_new_total']} new DRC errors, "
                          f"{sum(row['warnings_new'].values())} new warnings, {row['track_mm']} mm, {row['vias']} vias, judged in {row['judge_s']} s"
                          + ("" if ok else f": {row['why']}"))
                else:
                    print(f"FAIL judge {row['board']:6s} {row['why']}")
    if bad:
        print(f"     (plans{' and judged boards' if a.judge else ''} kept in {tmp})")
    else:
        shutil.rmtree(tmp, ignore_errors=True)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
