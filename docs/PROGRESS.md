# snisplit progress

Read this first when resuming. Last updated 2026-11-14. The plan itself is PLAN.md;
this file says where the work stands and what to do next.

## Where things stand

| Milestone | State |
|---|---|
| 0 Scaffold, licence, CI, WinDivert spike | Done (PLAN.md 5.1) |
| 1 Core: parsers, matcher, strategy, segment planner, engine | Done (PLAN.md 5.2, docs/milestone-1.md) |
| 2 Feasibility spike for the driverless design | **Done.** Owner said go on 2026-11-14 (docs/milestone-2-spike.md); crates decided (PLAN.md D11) |
| 3 Relay + DNS, HTTPS only, foreground | **Done** 2026-11-14 (docs/milestone-3.md): Steam URLs, Chrome, Steam client login and Store, medium.com through the relay with zapret stopped; Ctrl+C, kill and `cleanup` leave no rules |
| 4 HTTP respelling, profile files, IPv6-only names | **Done** 2026-11-14 (docs/milestone-4.md): every HTTP request respelled to `hoSt:`; Steam HTTP store, content servers and a real game download (1.66 GB, all on port 80) through the relay with zapret stopped; `--profile-file`; IPv6-only names forwarded |
| 5 Foreground use, crash-safe DNS rules, zapret removed (rescoped) | **Done** 2026-11-14 (docs/milestone-5.md): the Windows service was built, proven in CI and removed after Kaspersky terminated its install twice; zapret removed; the foreground build runs with no Kaspersky alert; Ctrl+C at any moment and closing the window leave 0 rules; Steam login and a game download through `snisplit run` (owner). Start-up 0.7-2.0 s, accepted (D14) |
| 6 `probe` | **Done** 2026-11-15 (docs/milestone-6.md): `snisplit probe <domain>` reproduces section 1's no-bypass and split rows (store.steampowered.com, medium.com blocked, cut at byte 1 and `hoSt:` pass; example.org not blocked) and prints a profile file; library entry point `snisplit_relay::probe::probe` for the UI |
| 7 and later | 8: see docs/milestone-8.md; 7, 9 and 10 not started |

Design: the owner chose a driverless design on 2026-11-14 (PLAN.md decision D1):
snisplit answers DNS for listed domains with a loopback address and relays the
connection, sending the first bytes in pieces. The same design is planned for Linux
and macOS (D4). The earlier WinDivert packet engine is parked, not deleted.

## Next step

Milestones 6 to 9 are delegated to the lead, Windows only (PLAN.md D13 to D15):
`probe` (6, done: docs/milestone-6.md) and the QUIC measurement (8) first, then the egui UI (9), then the draft
release (7). Nothing overnight may need elevation: checks run with `--no-routing`
or directly against the network, and routed checks go into one script for the owner.
Kaspersky: do not retry a service install or build variants to find one that passes; a
false-positive report needs the owner's go. The notes below are the original plan for the spike
questions, kept for reference.

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
  (parked). 12 real Steam ClientHello fixtures. The workspace has 472 passing tests.
- `snisplit_core::probe` and `snisplit_relay::probe`: the `probe` attempts, a hand-built
  ClientHello, reply classification, verdict and suggested profile (milestone 6).
- `crates/core` also has the DNS codec (`dns`), the relay's first-flight decision
  (`first_flight`) and the HTTP `Host` respeller (`http_stream`).
- `crates/relay`: tokio TCP relay (cut writes; every HTTP request head respelled to
  `hoSt:`; a `closed` event per connection), per-profile DNS server (listed names with
  no A record are forwarded unchanged), UDP upstream resolver shared by both.
- `crates/cli`: `snisplit run [--profile] [--profile-file] [--no-routing] [--upstream]
  [--verbose]` (stops on Ctrl+C, a closed window, logoff and shutdown), `cleanup`,
  `show-profile`, `probe <domain> [--upstream] [--timeout] [--runs]`, `--version`, `help`; built-in profiles are
  files in `crates/cli/profiles/` (steam 127.0.0.2, medium 127.0.0.3), profile files
  from 127.0.0.4; Windows NRPT routing through PowerShell.
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
- zapret was removed from this PC on 2026-11-14; reinstalling it is the owner's
  call. Do not work around the antivirus.
- Fixtures: TCP payload only, allowlisted Steam hosts only, no session-resumption
  tickets (a test enforces this).
