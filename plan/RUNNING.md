# Running the plan on this machine

Chris, 2026-11-16: do the whole plan, in this repo, commit and light-test often, do not stop to ask. Stop
only if the work is provably making a worse tool than KRT by our own tests; then dig in and find out why.

## The picks (Chris said not to ask; these are the lead's picks from the plan doc)

- Home: krt-better, branch `claude/astar-router`, folder `astar-router/` (Chris's ruling).
- Boards in: astar reads KiCad files itself (T1). KiCad's scene export is the stopgap and the cross-check.
- Judges: the house KiCad's DRC is the authority. KRT's checkers are the second opinion where they run here.
- `.kicad_pro` is left alone. Octilinear with arcs. A kipy plugin for the editor. Placement is out.

## The machine

- 2 cores, 7 GB. Two builds or DRC runs at once at most across all sessions; `nice` anything heavy.
- No crates.io and no PyPI: Rust std only; Python's standard library plus what is installed (PIL and
  matplotlib are). A blocked host is not retried and not routed round.
- Public GitHub repositories can be read with `git clone` / `git fetch` (the attached private ones too).
  The proxy refuses `raw.githubusercontent.com` (403: do not retry it), PyPI and crates.io. No other internet.
- KiCad, the house build 10.99: `. ~/.local/kicad-gs/env.sh` gives `kicad-cli`. There is no `pcbnew` module.
- Beside this repo: `/home/claude/router-bench` (bench.py, CONTRACT.md, `boards/*/scene.json` and the bare
  boards), `/home/claude/tools` (house boards, `pcb/docs/routing.md`), `/home/claude/topo-router`,
  `/home/claude/kicad`, `/home/claude/parts`. KRT is this repo (`py_router/` and so on); parts of it need
  packages this machine cannot install.
- A scene from a board today: router-bench's `bench.py prepare` (`kicad-cli pcb route-field ... --scene`,
  then `normalise_scene`). A plan onto a board today: `bench/shim/emit_plan.py` `write_board`.
- The container can be reclaimed. Anything not pushed is lost: push after every integration.

## How a session works a track

1. Worktree: `git -C /home/claude/krt-better worktree add /home/claude/wt/tNN -b tNN/work claude/astar-router`.
   If it exists already, use it: `git fetch -q . && git rebase claude/astar-router`.
2. Read `plan/README.md`, your track file, `plan/status/TNN.md` if it exists, `../README.md`. Do the next
   packages in order. You are orchestrator and implementer in one: do the work yourself.
3. Build: `cd astar-router && nice cargo build --release`.
4. Light test after every change: `./gate.sh --light` (seconds). Before a package is called done:
   `./gate.sh` (the bench five through KiCad's DRC).
5. Commit every working step. The message says what changed and the numbers. Every commit ends with:

       Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>
       Claude-Session: https://claude.ai/code/session_015eXUEeiedGwPpEn87Pst4w

6. Integrate after every package: `git fetch -q . && git rebase claude/astar-router`, the light gate, then
   `git push . HEAD:claude/astar-router` (fast-forward only; if refused, rebase and try again) and
   `git push origin claude/astar-router`. Small and often beats big and late.
7. In the same commit, `plan/status/TNN.md`: one line a package: state (done, partial, blocked), commit,
   the numbers. This file is how the next session picks up where you stopped.
8. End with a report of at most 15 lines: packages done, numbers, what is next, what the lead must settle.

## Rules that bite

- Only what worked is committed. No knob that measured neutral. No write-up of a failed try in the repo
  beyond one line in your status file.
- Never push to `drandyhaas/KiCadRoutingTools`. Never touch `~/tools/vendor`. Never commit to the default
  branch `krt-better`. Everything goes to `claude/astar-router`.
- Stay in the files your track owns (`plan/README.md` says which). What you need elsewhere goes in your report.
- Numbers come with their boards and their command.
- If a change makes the bench five or the corpus worse and you cannot see why within the package, back it
  out, write the one line, and move on.
