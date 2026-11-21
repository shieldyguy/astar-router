#!/usr/bin/env bash
# The five bench boards must give the copper they gave before.
#
#     tests/same.sh             build, route the bench boards this checkout has, compare with tests/golden/
#     tests/same.sh --no-build  the same with the binary as it stands
#     tests/same.sh --record    write tests/golden/ from this build (only when copper is MEANT to change)
#
# The golden of a board is one line a net: the sha256 of the net's tracks and vias as the plan gives
# them, and one line for the planes. `timing_s`, `summary` and `left_out` are not part of it.
# Exit 0: every board's copper is the golden's. Exit 1: it is not, or a board could not be routed.
#
# The repo carries dock and g474 (corpus/boards). ccb, tb150 and ub400 are not in it: where a router-bench
# checkout is beside this one (or ASTAR_BENCH_BOARDS / ROUTER_BENCH names it) they are routed too, against
# goldens recorded on that machine (tests/golden/ ignores theirs). A board without a scene or without a
# golden is skipped, and said.
#
# ASTAR names the binary (default target/release/astar).

set -u
cd "$(dirname "$0")/.." || exit 1
BOARDS="dock g474 ccb tb150 ub400"
GOLDEN=tests/golden
ASTAR_BIN="${ASTAR:-target/release/astar}"
build=1
record=0
for a in "$@"; do
    case "$a" in
        --no-build) build=0 ;;
        --record) record=1 ;;
        *) echo "same.sh: unknown argument $a" >&2; exit 1 ;;
    esac
done

# where a board's scene is: the folder named, the repo's own copy, a bench checkout
scene_of() {
    local b="$1" d
    for d in "${ASTAR_BENCH_BOARDS:-}" corpus/boards "${ROUTER_BENCH:+$ROUTER_BENCH/boards}" /home/claude/router-bench/boards "$HOME/router-bench/boards" ../router-bench/boards; do
        if [ -n "$d" ] && [ -s "$d/$b/scene.json" ]; then echo "$d/$b/scene.json"; return 0; fi
    done
    return 1
}
HAVE=""
for b in $BOARDS; do
    if ! scene_of "$b" > /dev/null; then
        echo "same.sh: skip  $b: not in this checkout"
    elif [ "$record" != 1 ] && [ ! -s "$GOLDEN/$b.txt" ]; then
        echo "same.sh: skip  $b: no golden here (tests/same.sh --record makes one)"
    else
        HAVE="$HAVE $b"
    fi
done
BOARDS="$HAVE"
if [ -z "$BOARDS" ]; then
    echo "same.sh: FAIL: no bench board with a golden was found" >&2
    exit 1
fi

if [ "$build" = 1 ]; then
    if ! out=$(nice cargo build --release 2>&1); then
        echo "$out" >&2
        echo "same.sh: FAIL: the build failed" >&2
        exit 1
    fi
fi
if [ ! -x "$ASTAR_BIN" ]; then
    echo "same.sh: FAIL: no binary at $ASTAR_BIN" >&2
    exit 1
fi

TMP=$(mktemp -d "${TMPDIR:-/tmp}/astar-same.XXXXXX") || exit 1
fail=0
for b in $BOARDS; do
    scene=$(scene_of "$b")
    "$ASTAR_BIN" route "$scene" "$TMP/$b.json" 2> "$TMP/$b.err"
    rc=$?
    # 0 every net joined, 1 nets open, 3 a rule narrowed: a plan was written each time
    if [ "$rc" != 0 ] && [ "$rc" != 1 ] && [ "$rc" != 3 ]; then
        echo "same.sh: FAIL: $b: the router exited with an error:" >&2
        tail -n 5 "$TMP/$b.err" >&2
        fail=1
        continue
    fi
    if [ ! -s "$TMP/$b.json" ]; then
        echo "same.sh: FAIL: $b: the router wrote no plan" >&2
        fail=1
    fi
done
if [ "$fail" != 0 ]; then
    echo "same.sh: FAIL (plans kept in $TMP)" >&2
    exit 1
fi

python3 - "$TMP" "$GOLDEN" "$record" $BOARDS <<'EOF'
import hashlib, json, os, sys

tmp, golden, record, boards = sys.argv[1], sys.argv[2], sys.argv[3] == "1", sys.argv[4:]


def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def lines_of(plan):
    """One line a net, in the plan's order, and one for the planes."""
    out = ["%s  (planes)" % hashlib.sha256(canon(plan.get("planes", [])).encode()).hexdigest()]
    for n in plan.get("nets", []):
        body = {"tracks": n.get("tracks", []), "vias": n.get("vias", [])}
        out.append("%s  %s" % (hashlib.sha256(canon(body).encode()).hexdigest(), n.get("name", "")))
    return out


bad = 0
for b in boards:
    got = lines_of(json.load(open(os.path.join(tmp, b + ".json"))))
    path = os.path.join(golden, b + ".txt")
    if record:
        os.makedirs(golden, exist_ok=True)
        with open(path, "w") as f:
            f.write("\n".join(got) + "\n")
        print("same.sh: recorded %s: %d nets" % (b, len(got) - 1))
        continue
    if not os.path.isfile(path):
        print("same.sh: FAIL: %s: no golden at %s" % (b, path))
        bad += 1
        continue
    want = open(path).read().split("\n")
    want = [w for w in want if w]
    if got == want:
        print("same.sh: ok    %-6s %d nets, the copper is the golden's" % (b, len(got) - 1))
        continue
    bad += 1
    g = {l.split("  ", 1)[1]: l.split("  ", 1)[0] for l in got}
    w = {l.split("  ", 1)[1]: l.split("  ", 1)[0] for l in want}
    changed = sorted(k for k in g if k in w and g[k] != w[k])
    gone = sorted(k for k in w if k not in g)
    new = sorted(k for k in g if k not in w)
    order = not changed and not gone and not new
    print("same.sh: FAIL  %-6s THE COPPER DIFFERS FROM THE GOLDEN: %d nets changed, %d gone, %d new%s"
          % (b, len(changed), len(gone), len(new), "; the same nets in another order" if order else ""))
    for label, names in (("changed", changed), ("gone", gone), ("new", new)):
        if names:
            print("    %s: %s%s" % (label, ", ".join(names[:12]), " ..." if len(names) > 12 else ""))
sys.exit(1 if bad else 0)
EOF
rc=$?
if [ "$rc" != 0 ]; then
    echo "same.sh: FAIL: the bench five do not give the golden copper (plans kept in $TMP)" >&2
    exit 1
fi
rm -rf "$TMP"
[ "$record" = 1 ] && echo "same.sh: golden recorded" || echo "same.sh: PASS: the copper of$BOARDS is identical to the golden"
exit 0
