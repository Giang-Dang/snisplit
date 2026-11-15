# snisplit progress

Read this first when resuming. Last updated 2026-11-15 (window redesign, 0.2.0). The plan itself is PLAN.md;
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
| 8 QUIC | **Measured** 2026-11-14 (docs/milestone-8.md): not filtered by server name on this ISP; no QUIC work. Awaiting the owner: one routed browser run to time the fallback to TCP |
| 9 UI | **Done** 2026-11-15 (docs/milestone-9.md): `snisplit-gui` (egui) starts and stops the relay and lists, checks, adds and removes sites; the owner's elevated run passed every step: Start, fetch through the relay, add a site, Stop, Start again, close while running, 0 rules left |
| 9b Window redesign | **Built** 2026-11-15 (docs/window-redesign.md): version 0.2.0; a 400 px window with one round button, a switch per site (`off.list`, which `run` honours too), rows that open in place, plain failure causes, leftovers removal, Activity with Save to a file. Seen in a real window: Off, a row opened, Test on medium.com (debug build with a temporary trace), Activity, close, and without input Medium switched off, every site off and the small header; the rest of the smoke run was stopped because the window received input the test driver did not send. The owner's elevated `tools/acceptance/m9b.ps1` run on a build from 0c8d793 passed (Start, switch, add and remove while on, Stop, close while on: 0 rules, exit code 0). On 2026-11-15 the redesign and changes 1-4 (the post-window wait for rule removal, leftovers shown at launch, one instance at a time with "Already open", a broken file named like a built-in as its own row) passed the owner's elevated acceptance: `m9c.ps1` and `m9b.ps1`, 39 checks each, all PASS (docs/window-redesign.md, last section). Still not covered by any run: a real UI panic, the lock with two accounts, logoff or shutdown with the window open, Escape and the Keep / Remove order on screen |
| 7 Releases | **Built** 2026-11-15 (docs/milestone-7.md): README, release workflow, version 0.1.0, draft release `v0.1.0` with the Windows archive. Awaiting the owner: try the downloaded archive, publish the draft, a friend installs |
| 10 Linux and macOS | Deferred (PLAN.md D13) |

Design: the owner chose a driverless design on 2026-11-14 (PLAN.md decision D1):
snisplit answers DNS for listed domains with a loopback address and relays the
connection, sending the first bytes in pieces. The same design is planned for Linux
and macOS (D4). The earlier WinDivert packet engine is parked, not deleted.

## Next step

Milestones 6 to 9 are built (PLAN.md D13 to D15). Everything that needs
administrator rights or a second person waits for the owner, in this order:

1. Done 2026-11-15: `tools/acceptance/m9.ps1` passed (docs/milestone-9.md).
1b. Done 2026-11-15: the redesigned window (0.2.0, release build in `target\release`)
   passed the owner's elevated `m9c.ps1` and `m9b.ps1`, 39 checks each
   (docs/window-redesign.md, last section).
2. While the window is running: open a listed site in a browser that used it over
   HTTP/3 before, and see that it falls back to TCP quickly (docs/milestone-8.md).
3. The draft release: download, check, publish, a friend installs (docs/milestone-7.md).

After that: milestone 10 (Linux and macOS) when the owner asks for it.
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
  (parked). 12 real Steam ClientHello fixtures. The workspace has 640 passing tests on Windows.
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
  from 127.0.0.4, then user sites from `%APPDATA%\snisplit\sites`; Windows NRPT routing through
  PowerShell. The crate is also a library `snisplit` (session, profiles, sites, system DNS).
- `crates/gui`: `snisplit-gui`, the egui window (Windows only; a stub elsewhere), its state
  machine, the view texts and the effect driver (milestone 9, redesigned in 0.2.0:
  docs/window-redesign.md).
- `%APPDATA%\snisplit\off.list`: the sites switched off, one name per line; read by the
  window and by `run` without `--profile` or `--profile-file`.
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
