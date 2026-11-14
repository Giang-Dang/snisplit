# snisplit progress

Read this first when resuming. Last updated 2026-11-14. The plan itself is PLAN.md;
this file says where the work stands and what to do next.

## Where things stand

| Milestone | State |
|---|---|
| 0 Scaffold, licence, CI, WinDivert spike | Done (PLAN.md 5.1) |
| 1 Core: parsers, matcher, strategy, segment planner, engine | Done (PLAN.md 5.2, docs/milestone-1.md) |
| 2 Feasibility spike for the driverless design | **Done.** Owner said go on 2026-11-14 (docs/milestone-2-spike.md); crates decided (PLAN.md D11) |
| 3 Relay + DNS, HTTPS only, foreground | **In progress** (docs/milestone-3.md): relay, DNS server, upstream resolver, NRPT routing and `run`/`cleanup` merged; done-when 1 (no admin, medium via curl) passed; elevated acceptance (`tools/acceptance/m3.ps1`) pending |
| 4 and later | Not started |

Design: the owner chose a driverless design on 2026-11-14 (PLAN.md decision D1):
snisplit answers DNS for listed domains with a loopback address and relays the
connection, sending the first bytes in pieces. The same design is planned for Linux
and macOS (D4). The earlier WinDivert packet engine is parked, not deleted.

## Next step

Run the elevated milestone 3 acceptance with the owner (`tools/acceptance/m3.ps1`;
it stops `zapret-steam` for the Steam part, so ask first), record the result in
docs/milestone-3.md, then milestone 4 (`hoSt:` for HTTP). The notes below are the original plan
for the spike questions, kept for reference.

- Question 1 (do two writes get HTTPS through where one write fails) and question 2
  (an HTTP method) need test runs against the ISP with the `zapret-steam` service
  stopped. Always ask the owner before stopping it, every time.
- Question 3 (does a Steam game download use port 80) needs the owner to start a
  download while connections are observed.
- Question 4 (NRPT routing of only the listed names; Chrome; stale rules after a
  crash) needs admin rights to add and remove a rule.
- Question 6 (new crates: threads vs an async runtime, a DNS codec) is a written
  proposal for the owner.

Note: an earlier attempt to brief a developer agent for the milestone 2 test tools
was stopped by a safety check and is not to be resumed in that form. The worktree
for it was removed; no code from it exists.

## What exists in the code

- `crates/core`: TLS ClientHello and HTTP Host inspection, domain matcher, split
  strategy (reused by the relay); segment planner, flow table, engine, packet loop
  (parked). 12 real Steam ClientHello fixtures. The workspace has 291 passing tests.
- `crates/cli`: `snisplit run [--verbose]`, `--version`, `help`, built-in Steam
  profile. `run` currently exits with "unsupported": it was wired to the parked
  engine and will be rewired to the relay in milestone 3.
- `crates/backend-windows`: WinDivert FFI and the `sniff` / `capture` examples
  (parked; `capture` is how the fixtures were recorded).
- Local-only branch `m2/windows` (never pushed): runtime-loaded WinDivert and a
  `PacketIo` implementation. Parked; its two findings are recorded in PLAN.md
  section 3.

## Owner's standing rules

- No code comments of any kind. ASCII punctuation only in source.
- No tautology tests: every expected value independent of the code under test; for
  each test, name the bug that makes it fail.
- Unit tests over ~150 lines go in `<module>/tests.rs`; public-API tests in
  `crates/<crate>/tests/`.
- No new crates without an explicit decision. Decided so far: `tokio` with minimal
  features, outside `core` (PLAN.md D11).
- Docs, code and commit messages in English.
- Gates before every commit: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets` with `RUSTFLAGS=-D warnings`,
  `cargo test --workspace`.
- Commit locally; push only when the owner says so. Watch CI after each push.
- Subagents: pick and announce model and effort per agent; brief them with the rules
  above; verify their claims before reporting.
- Never stop or change the `zapret-steam` service or its files without asking.
  Do not work around the antivirus.
- Fixtures: TCP payload only, allowlisted Steam hosts only, no session-resumption
  tickets (a test enforces this).
