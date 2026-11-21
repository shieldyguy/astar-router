#!/usr/bin/env python3
"""fetch_stress: KRT's stress corpus (open boards on GitHub), fetched by git into a cache outside the repo.

    fetch_stress.py [SET ...] [--only NAME ...] [--cache DIR] [--cap-gb 3] [--list] [--again]

The boards are the ones KRT's own lists name: tests/stress/manifest_set*.json (33 sets; each row a repository,
a branch or a commit, and a path). KRT's fetch scripts download them from raw.githubusercontent.com, which
this machine's proxy refuses; the same files come by git: for each repository a shallow clone without blobs
(one commit, its trees), and from it the named board file and its `.kicad_pro` / `.kicad_dru` siblings alone.
The clone is thrown away once the files are out.

    CACHE/stress/NAME/NAME.kicad_pcb [.kicad_pro .kicad_dru]     the files as the repository has them
    CACHE/stress/NAME/source.json                                 set, repo, ref asked for, commit, path, licence, KRT's facts
    CACHE/stress/fetched.json                                     every row: fetched, or skipped and why

CACHE is --cache, else $ASTAR_CORPUS_CACHE, else /home/claude/corpus-cache. Nothing here is committed:
third-party boards never go into this repository (corpus/README.md). corpus/prepare.py makes them bare and
exports their scenes into CACHE/boards/NAME/, and corpus/manifest.json lists them with source "stress", their
repository, commit and path, so anyone can fetch the same bytes again.

SET names sets ("set3", "3", "set1monster"); none: all of them. A board already in the cache is not fetched
again (--again: it is). A row that cannot be fetched (no such repository, branch or path; not on GitHub; the
cache at its cap) is skipped, named at the end, and written to fetched.json. Reading only: nothing is ever
pushed to those repositories (the clone has no credentials of its own and is deleted).
"""
import argparse, glob, json, os, re, shutil, subprocess, sys, time, urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
LISTS = os.path.join(REPO, "tests", "stress")
CACHE = os.environ.get("ASTAR_CORPUS_CACHE") or "/home/claude/corpus-cache"
RAW = re.compile(r"^https://raw\.githubusercontent\.com/([^/]+)/([^/]+)/(.+)$")


def slug(s):
    return re.sub(r"[^A-Za-z0-9_.-]+", "-", s).strip("-")


def rows(sets=None):
    """every row of KRT's lists as {name, set, repo, ref, path, ...}, or with `skip` when it cannot come by git;
    names are made unique in the lists' order"""
    out, seen = [], set()
    order = lambda f: (not re.search(r"set\d+\.json$", f), int((re.search(r"set(\d+)", f) or [0, 0])[1]), f)
    for f in sorted(glob.glob(os.path.join(LISTS, "manifest_set*.json")), key=order):
        tag = os.path.basename(f)[len("manifest_"):-len(".json")]
        if sets and tag not in sets:
            continue
        for r in json.load(open(f)):
            repo, path, url = r.get("repo") or "", r.get("path") or "", r.get("raw_url") or ""
            stem = r.get("short_name")
            if not stem:
                # the early lists have no short name: the repository's, and the file's when that says more
                bare = lambda t: re.sub(r"[^a-z0-9]", "", t.lower())
                stem, f = repo.split("/")[-1], re.sub(r"\.kicad_pcb$", "", os.path.basename(path or r.get("file") or ""))
                if f and bare(f) not in bare(stem):
                    stem += "_" + f
            name = "stress_" + slug(stem)
            k = 2
            while name in seen:
                name = f"stress_{slug(stem)}-{k}"
                k += 1
            seen.add(name)
            row = {"name": name, "set": tag, "repo": repo, "path": path, "license": r.get("license"), "note": r.get("note"),
                   "krt": {k: r[k] for k in ("layers_est", "footprints", "routable_nets", "max_pads", "tier", "packages", "lane") if k in r}}
            m = RAW.match(urllib.parse.unquote(url))               # a path with spaces is %20 in the url
            if not m or not path or f"{m.group(1)}/{m.group(2)}" != repo or not m.group(3).endswith("/" + path):
                out.append(dict(row, skip=f"not a file of a GitHub repository: {url or 'no url'}"))
                continue
            row["ref"] = m.group(3)[:-len(path) - 1]          # a branch, a tag or a commit: what stands before the path
            out.append(row)
    return out


def git(args, cwd=None, timeout=300):
    """(exit code, stdout, stderr's last line) of a git command that never asks for a password"""
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0", GIT_ASKPASS="true")
    try:
        r = subprocess.run(["git"] + args, cwd=cwd, capture_output=True, timeout=timeout, env=env)
    except subprocess.TimeoutExpired:
        return 124, b"", f"git {args[0]} was still going after {timeout} s"
    err = r.stderr.decode(errors="replace").strip().split("\n")[-1] if r.stderr else ""
    return r.returncode, r.stdout, err


def fetch_one(row, cache):
    """the row's files into cache/stress/NAME/: (ok, why not, bytes written)"""
    dst = os.path.join(cache, "stress", row["name"])
    tmp = os.path.join(cache, "tmp", row["name"])
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    try:
        url = f"https://github.com/{row['repo']}.git"
        code, _, err = git(["init", "-q", tmp])
        git(["remote", "add", "origin", url], cwd=tmp)
        # one commit, no blobs; the blobs asked for below come one by one (a partial clone's promisor remote)
        git(["config", "remote.origin.promisor", "true"], cwd=tmp)
        git(["config", "remote.origin.partialclonefilter", "blob:none"], cwd=tmp)
        code, _, err = git(["fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", row["ref"]], cwd=tmp)
        if code != 0:
            return False, f"git fetch {row['ref']}: {err[:200]}", 0
        code, out, err = git(["rev-parse", "FETCH_HEAD"], cwd=tmp)
        commit = out.decode().strip()
        base = row["path"][:-len(".kicad_pcb")]
        code, out, err = git(["ls-tree", "--name-only", "FETCH_HEAD", "--"] + [base + e for e in (".kicad_pcb", ".kicad_pro", ".kicad_dru")], cwd=tmp)
        have = set(out.decode(errors="replace").split("\n"))
        if row["path"] not in have:
            return False, f"no {row['path']} at {commit[:12]}", 0
        os.makedirs(dst, exist_ok=True)
        size = 0
        for ext in (".kicad_pcb", ".kicad_pro", ".kicad_dru"):
            if base + ext not in have:
                continue
            code, blob, err = git(["show", f"FETCH_HEAD:{base + ext}"], cwd=tmp, timeout=600)
            if code != 0:
                if ext == ".kicad_pcb":
                    return False, f"git show {row['path']}: {err[:200]}", 0
                continue
            open(os.path.join(dst, row["name"] + ext), "wb").write(blob)
            size += len(blob)
        head = open(os.path.join(dst, row["name"] + ".kicad_pcb"), "rb").read(64)
        if b"kicad_pcb" not in head and b"PCBNEW" not in head:
            shutil.rmtree(dst, ignore_errors=True)
            return False, f"{row['path']} is not a KiCad board (it starts {head[:30]!r})", 0
        src = {k: row.get(k) for k in ("name", "set", "repo", "ref", "path", "license", "note", "krt")}
        src.update(commit=commit, url=f"https://github.com/{row['repo']}/blob/{commit}/{row['path']}",
                   files=sorted(os.listdir(dst)), bytes=size)
        json.dump(src, open(os.path.join(dst, "source.json"), "w"), indent=1)
        return True, "", size
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def cache_bytes(cache):
    total = 0
    for d, _, fs in os.walk(os.path.join(cache, "stress")):
        total += sum(os.path.getsize(os.path.join(d, f)) for f in fs)
    return total


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("sets", nargs="*", help="sets to fetch (set3, 3, set1monster); none: all")
    ap.add_argument("--only", nargs="*", default=[], help="these boards alone, by name")
    ap.add_argument("--cache", default=CACHE)
    ap.add_argument("--cap-gb", type=float, default=3.0, help="stop fetching when the cached boards reach this (3)")
    ap.add_argument("--list", action="store_true", help="name the rows and stop")
    ap.add_argument("--again", action="store_true", help="fetch also what the cache already has")
    a = ap.parse_args()
    sets = {s if s.startswith("set") else "set" + s for s in a.sets}
    todo = [r for r in rows(sets) if not a.only or r["name"] in a.only]
    if a.list:
        for r in todo:
            print(f"{r['name']:44s} {r['set']:12s} {r['repo']}  {r.get('ref', '-')}  {r['path']}" + (f"   SKIP: {r['skip']}" if r.get("skip") else ""))
        print(f"{len(todo)} rows, {sum(1 for r in todo if r.get('skip'))} cannot come by git")
        return 0
    os.makedirs(os.path.join(a.cache, "stress"), exist_ok=True)
    log_path = os.path.join(a.cache, "stress", "fetched.json")
    log = json.load(open(log_path)) if os.path.exists(log_path) else {}
    cap = a.cap_gb * (1 << 30)
    used = cache_bytes(a.cache)
    t0 = time.time()
    got = had = 0
    skipped = []
    for r in todo:
        dst = os.path.join(a.cache, "stress", r["name"])
        if r.get("skip"):
            why = r["skip"]
        elif not a.again and os.path.exists(os.path.join(dst, "source.json")):
            had += 1
            continue
        elif used >= cap:
            why = f"the cache is at its cap ({a.cap_gb:g} GB)"
        else:
            ok, why, size = fetch_one(r, a.cache)
            if ok:
                got += 1
                used += size
                log[r["name"]] = {"fetched": True, "set": r["set"], "repo": r["repo"], "bytes": size}
                print(f"  ok   {r['name']:44s} {size / 1e6:6.1f} MB  {r['repo']}", flush=True)
                continue
        skipped.append((r["name"], why))
        log[r["name"]] = {"fetched": False, "set": r["set"], "repo": r["repo"], "why": why}
        print(f"  SKIP {r['name']:44s} {why}", flush=True)
        json.dump(log, open(log_path, "w"), indent=1)
    json.dump(log, open(log_path, "w"), indent=1)
    print(f"fetch_stress: {got} fetched, {had} already in the cache, {len(skipped)} skipped; {used / 1e9:.2f} GB in {a.cache}/stress; {time.time() - t0:.0f} s")
    for name, why in skipped:
        print(f"  skipped {name}: {why}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
