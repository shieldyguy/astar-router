#!/usr/bin/env bash
# gate.sh: what a change passes before it merges (plan/README.md, "Gates").
#
#     ./gate.sh --light   build; cargo test when the crate has tests; tests/same.sh (the bench five give their
#                         golden copper); the proof's own units (tests/proof_units.py); the bench five routed,
#                         every net joined by astar's own count and clean by astar's own exact check
#                         (`astar check`); one board routed twice, the same copper; a planted open, short and
#                         clearance error each caught by `astar check`. Seconds.
#     ./gate.sh           the light gate, then the judge (tests/judge.py: zones refilled, the house KiCad's
#                         DRC, against the bare board's own) on the bench five: every net landed, 0 new DRC
#                         errors; the planted errors (tests/judge_plants.py) caught by KiCad and by
#                         `astar check` alike; what the five lack (tests/gate_more.py: copper already on the
#                         board, a pour, a fine-pitch part on two layers), judged the same way; and the fuzz.
#     ./gate.sh --smoke   all of that, then the corpus smoke (corpus/run.py --smoke): the named boards routed
#                         and judged, none crashed, none worse than its row in corpus/baseline.jsonl.
#
# One line a check. Exit 0 when every check passed, 1 when one did not.
# ASTAR names the binary (default target/release/astar); ASTAR_BENCH_BOARDS a folder of bench boards. The repo
# carries dock and g474 of the bench five; ccb, tb150 and ub400 join in where a router-bench checkout is found.

set -u
cd "$(dirname "$0")" || exit 1
mode=full
for a in "$@"; do
    case "$a" in
        --light) mode=light ;;
        --smoke) mode=smoke ;;
        -h|--help) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "gate.sh: unknown argument $a" >&2; exit 1 ;;
    esac
done

fail=0
t_all=$(date +%s.%N)
since() { awk -v a="$1" -v b="$(date +%s.%N)" 'BEGIN { printf "%.1f", b - a }'; }
# check NAME COMMAND...: the command's own lines as they are when it prints one a check, else one line here
check() {
    local name="$1" t out rc
    shift
    t=$(date +%s.%N)
    out=$("$@" 2>&1)
    rc=$?
    if [ "$rc" = 0 ]; then
        echo "ok   $name ($(since "$t") s)"
    else
        echo "FAIL $name (exit $rc, $(since "$t") s)"
        echo "$out" | tail -n 15 | sed 's/^/     /'
        fail=1
    fi
}
# lines NAME COMMAND...: a command that prints one line a check itself
lines() {
    local name="$1" t rc
    shift
    t=$(date +%s.%N)
    "$@" 2>&1
    rc=$?
    [ "$rc" = 0 ] || { echo "FAIL $name (exit $rc, $(since "$t") s)"; fail=1; }
}

check "build (cargo build --release)" nice cargo build --release
if [ "$fail" != 0 ]; then
    echo "gate ($mode): FAIL: no build, nothing else was run"
    exit 1
fi

if grep -rqs '#\[test\]' src tests --include='*.rs'; then
    # the debug profile: a release test build relinks five test binaries with fat LTO (30 to 110 s)
    check "cargo test" nice cargo test --quiet
else
    echo "ok   cargo test: the crate has no tests yet"
fi

if [ -x tests/same.sh ]; then
    check "tests/same.sh: the bench five give their golden copper" tests/same.sh --no-build
else
    echo "ok   tests/same.sh: not there yet"
fi

lines "the proof's own units" python3 tests/proof_units.py
if [ "$mode" = light ]; then
    lines "the bench five" python3 tests/gate_bench.py
    lines "astar check's planted errors" python3 tests/judge_plants.py --check-only
else
    lines "the bench five, judged" python3 tests/gate_bench.py --judge
    lines "the judge's planted errors" python3 tests/judge_plants.py
    lines "old copper, a pour and fine pitch, judged" python3 tests/gate_more.py
    lines "what old copper joins, by KiCad and by astar" python3 tests/touch_judge.py
    # 10,000 bent scenes and every value of two fixtures changed alone, through the whole router: no panic
    check "the fuzz (tests/fuzz.rs, the two long ones)" nice cargo test --release --test fuzz -- --ignored
fi

if [ "$mode" = smoke ]; then
    if [ -f corpus/run.py ]; then
        lines "the corpus smoke" python3 corpus/run.py --smoke
    else
        echo "FAIL the corpus smoke: corpus/run.py is not there"
        fail=1
    fi
fi

if [ "$fail" = 0 ]; then
    echo "gate ($mode): PASS in $(since "$t_all") s"
    exit 0
fi
echo "gate ($mode): FAIL in $(since "$t_all") s"
exit 1
