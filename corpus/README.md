# corpus: the boards astar is proved on

Whose the boards are decides where they may go (Chris's list, 2026-11-18):

- **May be public**: dock, g474, every `house_*`, every `fix_*`, every `qa_*`.
- **Private**: ccb, tb150, ub400 and every `demo_*`. They stay out of a public repository, in every commit.
- **Not ours to pass on**: every `krt_*` (KRT's own boards) and the stress boards (third parties'). They are
  left out of any repository but the fork of KRT's.

This repository carries the first group only. The other boards keep their rows in the manifest and the
baseline (`committed` false); `prepare.py NAME` makes them in `boards/` from a router-bench, kicad or KRT
checkout on the machine, where `boards/.gitignore` keeps them out of a commit.

What is here:

    manifest.json     every board with its facts (source, layers, nets, zones, class) and where it came from
    boards/NAME/      the bare board, its project, its scene, the bare board's own DRC (made by prepare.py)
    prepare.py        makes boards/ and the manifest from the boards on this machine
    fetch_stress.py   fetches KRT's stress corpus (open boards on GitHub) into a cache outside the repo
    run.py            astar over the corpus, each board judged by KiCad; rows.jsonl
    baseline.jsonl    the last whole run's rows: what "no worse" is measured against
    scoreboard.py     the baseline as a page (SCOREBOARD.md): a row a board, a summary by source and by failure class
    smoke.txt         the boards the gate's smoke run routes and judges (`./gate.sh --smoke`)

Third-party boards (source `stress`: KRT's stress sets, fetched from their public repositories) are **not
committed**: the manifest names each one's repository, commit and path, `fetch_stress.py` gets it again, and
the files live in a cache outside the repo (`/home/claude/corpus-cache`, or `$ASTAR_CORPUS_CACHE`). Their
licences are their owners'; nothing is pushed to those repositories.

Not committed either: `out/` (plans, routed boards and DRC reports of the last run), `rows.jsonl`, and the
boards larger than 1.5 MB (`boards/.gitignore`; `prepare.py NAME` makes them again).
