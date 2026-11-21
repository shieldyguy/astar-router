#!/usr/bin/env python3
"""formats: boards of each KiCad file format, each read, routed, written and opened again (T1.7).

    formats.py [-v]

For each board below: `astar route IN.kicad_pcb -o OUT.kicad_pcb`; the written file is the input with the new
copper before its last bracket and nothing else changed; astar reads the written board again and finds the pads
it had and the copper it laid; the house KiCad opens it (`kicad-cli pcb export stats`) with no message the
input does not give and counts the vias that were laid. One line a board; exit 1 when a check fails.
The binary: $ASTAR, else target/release/astar. The kicad-cli: $KICAD_CLI, else the one on PATH or the house install.
"""
import json, os, re, shutil, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
REPO = os.path.dirname(ROOT)
BOARDS = [
    ("KiCad 7 (20221018): nets by number, tstamp, one item a line", os.path.join(ROOT, "src/io/fixtures/tiny7.kicad_pcb")),
    ("KiCad 9 (20241229): nets by number, uuid", os.path.join(REPO, "kicad_files/sonde_u.kicad_pcb")),
    ("KiCad 9 (20241229): a through-hole board", os.path.join(REPO, "kicad_files/interf_u_unrouted.kicad_pcb")),
    ("KiCad 10 (20260206): nets by name", os.path.join(REPO, "kicad_files/cap_chain.kicad_pcb")),
    ("KiCad 10 (20260206): pads inside a QFN", os.path.join(REPO, "kicad_files/qfn_interior_pads.kicad_pcb")),
    ("KiCad 10 (20260206): a bench board", os.path.join(ROOT, "corpus/boards/dock/dock.kicad_pcb")),
    ("10.99 (20261017): a reference twice, a footprint without one, a net named with quotes and brackets", os.path.join(ROOT, "src/io/fixtures/tiny.kicad_pcb")),
    ("10.99 (20261017): a house board, parts at any angle", os.path.join(ROOT, "corpus/boards/qa_keys-mini/qa_keys-mini.kicad_pcb")),
]


def kicad_cli():
    for c in (os.environ.get("KICAD_CLI"), shutil.which("kicad-cli"), os.path.expanduser("~/.local/kicad-gs/kicad-gs/kicad-cli")):
        if c and os.path.exists(c):
            return c
    return None


def stats(cli, board, tmp):
    out = os.path.join(tmp, "stats.txt")
    r = subprocess.run([cli, "pcb", "export", "stats", board, "-o", out], capture_output=True, text=True)
    said = sorted({re.sub(r"^\d\d:\d\d:\d\d: ", "", l) for l in (r.stdout + r.stderr).splitlines() if l.strip() and not l.startswith("Wrote board statistics")})
    vias = None
    if r.returncode == 0 and os.path.exists(out):
        m = re.search(r"Through vias: (\d+)", open(out).read())
        vias = int(m.group(1)) if m else None
    return r.returncode, said, vias


def scene(astar, board):
    r = subprocess.run([astar, "scene", board], capture_output=True, text=True)
    return json.loads(r.stdout) if r.stdout.strip() else None


def main():
    verbose = "-v" in sys.argv[1:]
    astar = os.environ.get("ASTAR") or os.path.join(ROOT, "target", "release", "astar")
    cli = kicad_cli()
    if cli is None:
        print("formats: no kicad-cli: the written boards cannot be opened by KiCad here")
        sys.exit(2)
    bad = 0
    for label, src in BOARDS:
        name = os.path.basename(src)
        if not os.path.exists(src):
            print(f"FAIL {name}: not at {src}")
            bad += 1
            continue
        tmp = tempfile.mkdtemp(prefix="astar-formats-")
        try:
            out, plan = os.path.join(tmp, name), os.path.join(tmp, "plan.json")
            why = []
            r = subprocess.run([astar, "route", src, "-o", out, "--plan", plan, "--seconds", "60"], capture_output=True, text=True)
            if r.returncode not in (0, 1) or not os.path.exists(out):
                why.append(f"astar route exit {r.returncode}: {r.stderr.strip()[-200:]}")
            else:
                before, after = open(src).read(), open(out).read()
                cut = before.rstrip().rfind(")")
                if not after.startswith(before[:cut].rstrip()):
                    why.append("the written board is not the input with copper added")
                p = json.load(open(plan))
                pieces = sum(len(t["path"]) for n in p["nets"] for t in n["tracks"])
                vias = sum(len(n["vias"]) for n in p["nets"])
                a, b = scene(astar, src), scene(astar, out)
                if a is None or b is None:
                    why.append("astar does not read the board it wrote")
                else:
                    if a["pads"] != b["pads"] or a["outline"] != b["outline"] or a["nets"] != b["nets"]:
                        why.append("the written board's pads, nets or outline are not the input's")
                    got = len(b["copper"]["tracks"]) - len(a["copper"]["tracks"])
                    # (a line or an arc with no length is not written)
                    if not (0 < got <= pieces) and pieces:
                        why.append(f"{pieces} pieces of track laid, {got} read back")
                    if len(b["copper"]["vias"]) - len(a["copper"]["vias"]) != vias:
                        why.append(f"{vias} vias laid, {len(b['copper']['vias']) - len(a['copper']['vias'])} read back")
                rc0, said0, v0 = stats(cli, src, tmp)
                rc1, said1, v1 = stats(cli, out, tmp)
                new = [m for m in said1 if m not in said0]
                if rc1 != 0 or new:
                    why.append(f"KiCad opening the written board: exit {rc1}, says {new[:2]}")
                elif rc0 == 0 and v1 != (v0 or 0) + vias:
                    why.append(f"KiCad counts {v1} vias: the input's {v0} and {vias} laid were expected")
                joined = re.search(r"(\d+) of (\d+) nets joined", r.stderr)
                tail = f"{joined.group(0)}, {pieces} pieces of track and {vias} vias laid" if joined else ""
            if why:
                bad += 1
                print(f"FAIL {name}: {label}: " + "; ".join(why))
            else:
                print(f"ok   {name}: {label}: {tail}; read again by astar and opened by KiCad")
            if verbose and r.stderr:
                print("     " + r.stderr.strip().replace("\n", "\n     ")[:1200])
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
    print(f"formats: {len(BOARDS) - bad} of {len(BOARDS)} boards read, routed, written and opened again")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
