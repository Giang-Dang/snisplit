# Milestone 5: foreground use, crash-safe DNS rules, zapret removed

Rescoped on 2026-11-14 by the owner after Kaspersky terminated `snisplit service
install` twice: snisplit runs in the foreground, started when needed, and the
service is dropped. The plan below is kept as written; the service it
describes was built, proven in CI, and removed (see "Rescoped" at the end).

Done when (rescoped):

1. Routed `snisplit run` is ready in under 1.5 s on this PC. Partly met: 0.7-1.3 s in
   the two service-era runs; 703, 1789 and 2000 ms in the foreground run at 23:19.
2. Ctrl+C at any moment of start-up or later leaves 0 snisplit rules. Met at 50 ms
   to 2000 ms in two elevated runs.
3. Closing the window with X, and restarting the PC while `snisplit run` runs, leave
   0 rules; if a rule is left, the next `run` removes it. Close with X: met (23:20).
   Restart: skipped by the owner; if a rule is left, the next `run` removes it.
4. A killed process breaks only listed names, and the next `run` or `cleanup`
   removes its rules. Shown in milestones 3 and 4.
5. With zapret removed, the Steam client logs in and downloads a game through
   `snisplit run`. Met: zapret removed 2026-11-14; Steam left open kept working; the
   owner reports a game download through `snisplit run --verbose` worked.

Dropped with the service: the original done-when items 1 (service install and
uninstall), 2 (service restart after a kill), 3 (reboot with the service) and 5
(the CI service job).

Original done-when (the service plan):

1. `snisplit service install` (elevated) installs an auto-start Windows service that
   relays the chosen profiles; `snisplit service uninstall` removes the service, its
   files and every snisplit DNS rule.
2. Killing the service process (`Stop-Process -Force`) leaves non-listed names
   resolving exactly as before, and the service restarts on its own and relays the
   listed names again within 15 s.
3. After a reboot, with zapret removed (a separate step the owner approves when it
   runs), the service is running, listed names go through the relay, `example.org`
   resolves as without snisplit, and the Steam client logs in and downloads a game.
4. Ctrl+C at any moment of `snisplit run` start-up leaves 0 snisplit rules.
5. A Windows CI job installs, starts, queries, kills (and sees the restart), stops
   and uninstalls the service. The owner allowed pushing `ci/m5-*` branches for this
   (2026-11-14); `main` is pushed only when the owner says so.

## Measured before planning (2026-11-14, no admin, zapret running)

| What | Time |
|---|---|
| `powershell.exe -Command 1` (process start only) | 171 ms |
| `Get-DnsClientNrptRule` | 422 ms |
| `Clear-DnsClientCache` | 417 ms |
| System DNS servers (`Get-NetAdapter` + `Get-DnsClientServerAddress`, as `serve` does) | 1357 ms |
| `run --no-routing`, launch to ready line (m4 log) | 1236 ms |
| `run --profile medium`, routed (m4 log) | 2245 ms |
| `run`, steam and medium, routed (m4 log) | 3467 ms |

The 13-21 s start-up in PLAN.md section 6 is out of date: milestone 4's run took
3.5 s. Most of what is left is PowerShell process starts and the DNS server lookup.
`Add-DnsClientNrptRule -Namespace` takes `System.String[]`, so one rule per profile
can carry every namespace.

`windows-sys` added to a scratch copy of the workspace (`cargo add windows-sys@0.61
-p snisplit`): `Cargo.lock` lists no new package. tokio 1.54.1 and mio 1.2.5 already
pull `windows-sys` 0.61.2.

## Rules for every contributor

Same as docs/milestone-4.md: no comments of any kind, ASCII only, gates
(`cargo fmt --all --check`; `cargo clippy --workspace --all-targets` with
`RUSTFLAGS=-D warnings`, for the host and for `--target x86_64-unknown-linux-gnu`;
`cargo test --workspace`), tests with independent expected values, unit tests over
about 150 lines in `<module>/tests.rs`.

Added for this milestone:

- No contributor installs the service, adds DNS rules or runs `snisplit run` without
  `--no-routing` on the owner's PC. Elevated checks run only in the owner's acceptance
  run and in CI.
- `unsafe` stays in the `cfg(windows)` modules that call Windows. Each `unsafe` block
  is one call, its inputs built in safe code right before it.
- Never stage with `git add -A` or `git add .`; the untracked `sites/` folder stays
  out of every commit.

## Decisions

1. **windows-sys (PLAN.md D12, owner, 2026-11-14).** `windows-sys` 0.61 is a direct
   dependency of `crates/cli` only, under `cfg(windows)`, with features
   `Win32_Foundation`, `Win32_System_Services`, `Win32_NetworkManagement_IpHelper`,
   `Win32_NetworkManagement_Ndis` and `Win32_Networking_WinSock`. It stays on the line
   tokio uses, so the tree keeps one copy. Chosen over hand-written declarations
   because the adapter API's structs have unions and pointers.
2. **Service shape.** Name `snisplit`, display name `snisplit`, account LocalSystem
   (NRPT needs admin), start type automatic. Commands:
   - `snisplit service install [--profile <name>]... [--profile-file <path>]...
     [--upstream <ipv4>]... [--verbose]`;
   - `snisplit service uninstall`;
   - `snisplit service run [same options]`, which only the Service Control Manager
     starts; run from a terminal it says so and exits with an error.
   `--no-routing` is not accepted by the service commands.
3. **Installed files.** Install copies the running executable to
   `%ProgramFiles%\snisplit\snisplit.exe` and each profile file to
   `%ProgramFiles%\snisplit\profiles\<n>.profile` (n from 1, in command-line order),
   parses them first so a bad file fails the install, and registers the copy with a
   quoted path. A LocalSystem service must not run a binary or read a file that a
   normal user can replace; Program Files is admin-only by default, so no ACL work is
   needed. Install refuses when the service already exists ("uninstall first").
4. **Recovery.** `sc.exe failure snisplit reset= 86400 actions=
   restart/5000/restart/5000/restart/30000` and `sc.exe failureflag snisplit 1`, so a
   crash, a kill or a stop with an error exit restarts the service.
5. **Log file.** `%ProgramFiles%\snisplit\logs\snisplit.log`, one line per message,
   each prefixed with a UTC time (`2026-11-14T19:23:05Z`). At 10 MiB the file is
   renamed to `snisplit.old.log` (one old copy). `run` keeps printing to stdout.
6. **Start-up order** (shared by `run` and the service):
   1. listen for the stop request (Ctrl+C or the service stop);
   2. remove stale snisplit rules (any left by a crash or a reboot);
   3. find upstream DNS servers: `run` fails at once when there are none, as now;
      the service waits, retrying every 2 s and logging once a minute, since the
      network may not be up yet at boot;
   4. bind the relay and the DNS servers;
   5. add the rules and flush the cache;
   6. ready line.
   A stop request at any step ends start-up; if step 2 has begun, every snisplit rule
   is removed before exit, even when the stop interrupted the PowerShell child (a
   console Ctrl+C reaches it too). Step 1 uses a listener that is registered when it
   is created (`tokio::signal::windows::ctrl_c()` returns a registered stream), not a
   future that registers on first poll. PowerShell children are started with
   `CREATE_NEW_PROCESS_GROUP` so a console Ctrl+C does not reach them; the acceptance
   run checks that, and the remove-after-stop rule stays either way.
7. **Batched routing.** One PowerShell process at start (remove stale rules, add one
   rule per profile with all its namespaces, flush) and one at stop (remove, flush,
   count). Blocking calls run on `spawn_blocking`. Target: routed `run` ready in
   under 1.5 s on this PC.
8. **System DNS servers natively.** `GetAdaptersAddresses` (IPv4, adapters whose
   operational status is up, loopback addresses skipped, order kept, duplicates
   dropped) replaces the PowerShell lookup.
9. **Upstream refresh.** The relay crate gets a shared `Upstreams` handle used by
   `UdpResolver` and `DnsServer`. When no `--upstream` is given, `run` and the service
   re-read the system DNS servers every 30 s and swap the list when it changed (a log
   line says so). An empty list never replaces a non-empty one.
10. **Service status.** The service reports `RUNNING` as soon as its control handler
    is registered, before the upstream wait, so a boot without network does not sit
    in `START_PENDING`. Status therefore says nothing about readiness: CI and
    acceptance check behaviour (a snisplit rule exists and `Resolve-DnsName -Server`
    answers). On stop it reports `STOP_PENDING` with a 30 s wait hint and a rising
    checkpoint, which covers the PowerShell rule removal, then `STOPPED` with exit
    code 0, or `ERROR_SERVICE_SPECIFIC_ERROR` and code 1 when `serve` failed.
11. **Shutdown.** `SERVICE_CONTROL_SHUTDOWN` is handled like stop. If Windows ends the
    process before the rules are removed, step 2 of the next start removes them.
12. **zapret removal.** At the end of the acceptance run, after every other check
    passed with zapret stopped, and only with the owner's approval at that moment:
    the same commands as the bundle's `steam_service_remove.cmd` (stop and delete
    `zapret-steam`, stop and delete `windivert`). The bundle folder stays untouched.
    Rollback: run the bundle's `steam_service_install.cmd` as administrator. The
    acceptance script prints this command before and after the removal.
13. **Log text for an empty first write.** `relayed host:80 in 0 pieces` becomes
    `relayed host:80 with the request head still arriving`, so the second
    space-separated token stays `host:port` for the acceptance scripts. Service log
    lines are `<time> <the line run prints>`; scripts match on the text after the
    first space.

## Contract (lead commit, before the streams fork)

- `windows-sys` added as in decision 1.
- `crates/cli/src/session.rs`: `serve(options: RunOptions, start: StartPolicy, stop:
  impl Future<Output = ()> + Send, out: Output) -> io::Result<()>`, moved from
  `main.rs` together with `install_rules`, `remove_rules`, `routing_failure`, the
  `describe_*` functions and their tests. `StartPolicy { wait_for_upstreams: bool }`
  selects decision 6 step 3 behaviour (a bool, so no variant is unused before stream
  C). `Output` is `Arc<dyn Fn(&str) + Send + Sync>` and receives every line `run`
  prints today. `session::remove_rules(out: &Output) -> io::Result<()>` is the one
  entry point for `cleanup`, `service uninstall` and `serve`.
- `crates/cli/src/system_dns.rs`: `fn servers() -> io::Result<Vec<Ipv4Addr>>`, at
  first a wrapper around the current PowerShell lookup.
- `crates/relay/src/upstreams.rs`: `Upstreams` with `new(Vec<SocketAddr>)`,
  `current() -> Vec<SocketAddr>`, `replace(Vec<SocketAddr>) -> bool`, `Clone`.
  `UdpResolver::new` and `DnsServer::bind` take it and read `current()` on every
  query; every call site is updated (decision 9 without the 30 s re-read, which is
  stream A's).
- Decision 13 in `describe_event`.
- `main.rs` keeps only argument dispatch, `run`, `show_profile` and `finish`.

## Streams and file ownership

| Stream | Model, effort | Owns | Done check |
|---|---|---|---|
| A: start-up and routing | opus, high | `crates/cli/src/session.rs` (+ tests), `routing.rs`, `routing/tests.rs`, `system_dns.rs` (+ tests), `plan.rs` (+ tests) | decisions 6-9, gates; no routed run |
| C: service | opus, high | `crates/cli/src/service/**`, `args.rs`, `args/tests.rs`, `main.rs`, `.github/workflows/ci.yml` | decisions 2-5, 10, 11; CI service job green on a `ci/m5-*` push made by the lead |
| Lead | - | contract (including the relay `Upstreams` change and `windows-sys`), integration, `tools/acceptance/m5.ps1`, docs | acceptance |

A and C fork from the contract commit and touch no common file. C calls
`session::serve` and `session::remove_rules` only through the contract signatures.

## Blast radius

| Symbol | Callers | Change |
|---|---|---|
| `serve` (main.rs) | `run` | moves to `session.rs`, gains `mode`, `stop`, `out` |
| `UdpResolver::new` | `main.rs`; `relay/tests/dns_server.rs`; `relay/tests/upstream.rs` | takes `Upstreams` |
| `DnsServer::bind`, `bind_with` | `main.rs`; `relay/tests/dns_server.rs`; `relay/src/dns_server/tests.rs` | take `Upstreams` |
| `routing::install`, `remove_all`, `flush_cache`, `count` | `main.rs` (`install_rules`, `remove_rules`, `serve`, `cleanup`) | replaced by one start script and one stop script; `cleanup` uses the stop script |
| `routing::system_dns_servers` | `serve` | moves to `system_dns::servers`, native |
| `println!` in `serve` and `remove_rules` | `run`, `cleanup` | go through `Output`; stdout text unchanged except decision 12 |
| `args::Command` | `main` | gains `Service(ServiceCommand)` |

## Risks

| Risk | Handling |
|---|---|
| Wrong `SERVICE_STATUS` sequence: SCM marks the service hung or never restarts it | CI job checks `RUNNING` after start, `STOPPED` after stop; acceptance kills the process and checks for a new PID |
| Kaspersky reacts to a LocalSystem service that answers DNS | Stop and report to the owner; no workaround |
| Network not up at boot | Decision 6 step 3; acceptance reboot checks it |
| Ctrl+C kills the PowerShell child mid-install | Decision 6: remove after any stop; acceptance sends Ctrl+C at several delays |
| After zapret removal and reboot, a broken service leaves Steam unreachable | `snisplit service uninstall` and the rollback command restore the old state in one step each |
| CI runner already uses loopback port 53 or NRPT | CI job uses `--profile medium` on 127.0.0.3 and checks with `Resolve-DnsName -Server`; failures are investigated, not skipped |
| A friend's DNS server is a loopback address (a local resolver) | Skipped as today; the service then waits; the log line names the cause |

## Acceptance (lead, `tools/acceptance/m5.ps1`, elevated, the owner runs it)

Part 1, zapret stopped (owner asked first):
- routed `run`: launch-to-ready time against the baseline table; Ctrl+C sent at
  0.2, 0.5, 1 and 2 s after launch, 0 rules after each;
- `service install --profile steam --profile medium` from `target\release`; listed
  names resolve to the relay; store HTTP and HTTPS and a content server over HTTP
  pass; `example.org` unchanged;
- `Stop-Process -Force` on the service: `example.org` resolves during the gap; a new
  PID within 15 s; listed names relayed again;
- Steam left running across the service start: the owner confirms the client still
  works (open since milestone 3);
- a `Get-DnsClientCache` dump during the Steam window, to name `23.49.104.166`
  (carried from milestone 4);
- `service uninstall`: no service, no rules, no Program Files folder; then reinstall.

Part 2, separate approval: zapret removal (decision 12), then a reboot.

Part 3, after the reboot (`m5.ps1 -AfterReboot`): service running; rules present;
listed names relayed; `example.org` unchanged; Steam logs in and downloads a game;
received bytes per host from the service log (throughput recorded as in milestone 4,
not compared with zapret). Whether a Steam client that auto-starts at boot, before
the service is ready, needs a restart is recorded; nothing is done about it in this
milestone.

## Dropped

- Reinstall over an existing service (install refuses; uninstall first). Milestone 7
  may add upgrades.
- Profile changes without reinstall (edit the files under Program Files and restart
  the service works, but no command for it).
- Tray and status command: milestone 9.
- Linux and macOS services: milestone 10.
- Writing NRPT rules to the registry directly: undocumented, and PowerShell is fast
  enough once batched.

## Decisions made during implementation

- Start-up uses one gated PowerShell child instead of two scripts (stream A): it
  removes stale rules, prints a marker, waits on stdin, and adds the rules only when
  released; closing stdin abandons it. Stale removal runs while upstreams are found
  and sockets bound. A stale-removal failure surfaces at step 5 with its own message.
- A stop before the ready line prints `stopping` and the rules-left line; the ready
  line is not printed. Explicit `--upstream` values that are all loopback fail at
  once even for the service; a failed system lookup is retried while waiting.
- The service reports `ERROR_SERVICE_SPECIFIC_ERROR` code 2 when its log cannot be
  opened. Install also runs the profile clash check, clears stale profile copies,
  and skips copying the executable onto itself. Elevation is detected from
  `OpenSCManagerW` returning access denied. Uninstall stops and waits a second time
  after `DeleteService`, in case a recovery restart fired in between.
- Bind errors name the address (`binding 127.0.0.4:80 failed: ...`). Found in CI:
  GitHub's Windows runners reserve TCP port 80 (an administered port exclusion,
  `System` listening on `:::80`), so binding 127.0.0.x:80 fails with 10013. The CI
  service job and the bound start-up tests use a TLS-only profile file
  (`http-ports none`) on 127.0.0.4. The same would happen on a PC where http.sys or
  IIS holds port 80: the service then fails and restarts on the recovery schedule,
  and its log names the port. Handling that (skipping the HTTP listener with a log
  line) is left for milestone 7, when friends install it.
- Stream C ran `snisplit service install --no-routing` once from the debug build,
  against its brief; it stopped in the argument parser before any SCM, file or DNS
  call.

## Results before the owner's acceptance run

- Unrouted start-up (`run --no-routing --profile medium`, launch to ready line):
  median 33 ms over 7 runs, baseline median 1165 ms (stream A, release builds).
- CI `windows service` job, run 43608073645 on branch `ci/m5-service`, passed:
  rule present and the name relayed 0.5 s after install; after `Stop-Process -Force`
  a new process relayed again after 5.9 s; after `Stop-Service` 0 rules, exit code 0
  and no restart; uninstall left no service, no `C:\Program Files\snisplit` and 0
  rules. The registered command line and failure actions (`sc qc`, `sc qfailure`)
  match decisions 3 and 4.
- Earlier failing runs of the same job showed the recovery schedule working:
  restarts 5 s, 5 s, then 30 s apart.
- Workspace: 450 tests; all gates pass on `main`.
- The install folder comes from `%ProgramFiles%` only when it is an absolute path;
  an unset, empty or relative value is an error, so uninstall's recursive delete
  can never point at a folder under the current directory (lead review).
- `m5.ps1 -SwitchToSnisplit` proves snisplit with zapret stopped (store HTTPS and a
  content server over HTTP must return 200) before it removes `zapret-steam` and
  `windivert`; on failure it uninstalls snisplit and starts zapret again. Service
  logs are copied to the output folder before every uninstall.

## Acceptance results

### Part 1, elevated, 2026-11-14 22:10-22:12 (zapret stopped with the owner's consent)

- Direct control with zapret stopped: store HTTP and HTTPS `000`.
- Routed `run`, launch to ready line: 821, 759, 692 ms (milestone 4: 3467 ms;
  target under 1.5 s). Store HTTPS through the relay `200` each time; 0 rules after
  each Ctrl+C.
- Ctrl+C at 50, 100, 200, 500 ms (before the ready line) and at 1000 and 2000 ms
  (after it): each exited on its own with code 0, `stopping`, `snisplit DNS rules
  left: 0`. At 0 ms the event was sent before the child had started, so it never
  arrived; the script killed the process and ran `cleanup` (0 rules). A test artefact,
  not a snisplit failure.
- `service install --profile steam --profile medium --verbose` registered the
  service (System event 7045, 22:11:01), but its start failed with "Access is
  denied" (event 7000, same second). Kaspersky's behaviour detection flagged
  `target\release\snisplit.exe` as `PDM:Trojan.Win32.Generic` and later removed that
  file. The copy in `C:\Program Files\snisplit` was not removed. Which behaviour
  triggered it is not known yet; the same file had just done ten routed runs without
  an alert. Kaspersky was not worked around; the owner decides what happens next.
- A script bug hid the failure: `Log` wrote to the pipeline, so `Install-Service`
  returned its log lines along with `$false`, and `if (-not (Install-Service))` never
  threw. The script went on: store and content checks `000` (no service, zapret
  stopped), the kill step found PID 0, so the kill and recovery checks did not run.
- `Start-Service snisplit` at 22:15 then worked: the Program Files copy ran as
  LocalSystem, added its rules and printed its ready line (store relayed after
  1545 ms). Not checked in that run: traffic through it.
- Uninstall failed because the release binary was gone. The `finally` block removed
  the rules by name server and restarted zapret, but the `snisplit` service stayed
  installed and running (automatic start, no rules) with its folder.
- Not reached: kill and recovery, Steam left running, the DNS cache dump (empty),
  uninstall. The switch from zapret (approved by the owner) is on hold.
- Script fixes after this run: `Log` writes to the file and the host only; uninstall
  falls back to `sc.exe stop`/`delete`, rule removal by comment and name server, and
  folder removal when the binary is missing; `tools/acceptance/m5-restore.ps1` does
  the same on its own and starts zapret.
- Kaspersky's report (owner, 2026-11-14) also shows a separate detection by the file
  antivirus with cloud protection: stream C's release build in its worktree, named
  `VHO:Backdoor.Win64.Agent.gen` (the report labels its precision "behaviour
  analysis"; threat level high), was backed up and deleted from `target\release` and
  `target\release\deps`, with `bash.exe` and `cargo.exe` as initiators. The report
  has no times. This is consistent with the stream C release binary that would not
  start and then vanished. The build installed in Part 1 was a different file (main,
  other hash); its `PDM:Trojan.Win32.Generic` entry is not in this report. Builds
  without the service code were never flagged; whether the current binary is safe to
  keep on disk for foreground use is not established.

### Part 1 retry, elevated, 2026-11-14 22:35-22:37 (owner chose to retry the same design)

- Same code; a fresh release build (MD5 `caeba4b7...`) that stayed on disk and ran
  before the run started.
- Routed `run` ready after 1303, 815, 664 ms, store HTTPS `200` through the relay,
  0 rules after each stop. Ctrl+C at 50, 100, 200, 500, 1000, 2000 ms: each exited on
  its own, 0 rules.
- `service install` registered the service (event 7045, 22:36:12); its start failed
  with "Access is denied" (event 7000, 22:36:13).
- Kaspersky's report (CSV exported by the owner) has times for both runs. Each time
  one `snisplit.exe` process from `target\release` (process IDs 74304 and 15084) was
  detected as `PDM:Trojan.Win32.Generic` ("behaviour analysis") and terminated at
  22:11:01 and 22:36:13, the same second the service was created and its start was
  refused; a backup was made and the file was deleted about a minute later
  (22:11:56, 22:37:04). The process is the one running `service install`: the
  routed foreground runs of the same file minutes earlier raised nothing, and the
  install's exit line ("started") was never printed. The report does not name the
  action that triggered the verdict.
- With the fixed script the failed install was caught: no relaying within 60 s, the
  binary-free removal deleted the service and the folder, zapret was restarted. After
  the run (checked by the lead): no `snisplit` service, no install folder, 0 rules,
  zapret and `winws` running, store HTTPS `200`.
- Outcome: the self-installing service is flagged on this PC twice, each time at the
  install step, while foreground runs of the same file pass. Per the rule agreed
  before the retry, there is no further retry and no variant.

### Owner's foreground trial, 2026-11-14 evening

- The owner stopped `zapret-steam` and ran `snisplit run` from an administrator
  terminal with the Steam client already open. The owner reports that Steam worked
  without a restart and that Kaspersky raised no alert. Checked by the lead while it ran: one `snisplit` process, no
  service, 2 snisplit rules, `store.steampowered.com` resolves to 127.0.0.2, store
  HTTPS `200`, zapret stopped, no `winws`. One trial; it answers the milestone 3
  question about a Steam client left running for this PC, as far as one run can.
- The owner chose foreground use: snisplit runs only when needed. Whether zapret is
  first set to Manual or removed is still the owner's open decision.

### zapret removed, 2026-11-14 22:59 (owner's decision, `tools/acceptance/remove-zapret.ps1`)

- Before: `zapret-steam` running, `windivert` present, `winws` running, the snisplit
  binary present, 0 snisplit rules. Read-only check beforehand: only `zapret-steam`
  ran `winws`; no service depended on `windivert`.
- `net stop zapret-steam` and `sc.exe delete zapret-steam` succeeded. `sc.exe stop
  windivert` reported the driver STOPPED; `sc.exe delete windivert` then returned
  1060 (the service does not exist). After: neither service present, no `winws`.
- Checked by the lead afterwards: both services gone, the binary present, 0 rules;
  with snisplit not running, store HTTPS fails (`000`), as expected.
- Rollback: `snisplit cleanup`, then the bundle's `steam_service_install.cmd` as
  administrator. The bundle folder was not touched.

## Rescoped: the service removed, the run stops on every console event

- Commit `6de6196` removes the service module, its commands and tests, the CI service
  job and its `ci/**` trigger, the `Win32_System_Services` feature, the upstream wait
  only the service used, and the service parts of `m5.ps1`. Its parent is the last
  commit with the service. `m5-restore.ps1` stays as the binary-free rule cleanup.
- Commit `4da1a44`: `run` stops through the same rule-removing path on Ctrl+C,
  Ctrl+Break, a closed console window, logoff and shutdown (SIGINT, SIGTERM, SIGHUP
  on Unix). The intent: tokio's console handler does not return after a close,
  logoff or shutdown event (read in its source), which should leave the stop path
  time to run; how long Windows waits is not assumed. PowerShell children now get
  `CREATE_NO_WINDOW`, meant to keep the rule removal from going down with a closing
  console. Neither is shown in a real close yet.
  Not checked yet: a routed close with X and a restart while running (both in
  `m5.ps1`, the second with `-AfterRestart`). The lead could not automate an X close
  here: conhost hands new windows to Windows Terminal.

## Adding a site today

A profile file adds sites without code changes; a built-in prints a template. In
Windows PowerShell 5.1, `>` writes UTF-16, which the profile parser rejects, so
write the file as ASCII:

```
snisplit show-profile medium | Set-Content -Encoding ascii my-sites.profile
```

Change the `name` line to a new name (a built-in name is rejected) and the
`domains` lines to the sites (for example `example.com`). Built-in profiles run only
when neither `--profile` nor `--profile-file` is given, so name them too:
`snisplit run --profile steam --profile medium --profile-file my-sites.profile`.
Keep a personal site list outside the repo.

Before trusting a site, check it without admin rights: run `snisplit run
--no-routing --profile-file my-sites.profile` and compare
`curl --resolve example.com:443:127.0.0.4 https://example.com/` with the same request
sent directly. Splitting helps only where the block is by SNI or Host; a site blocked
by IP address or DNS poisoning of its real address is not fixed by it. Milestone 6
(`probe`) automates this check and is the base for the UI in milestone 9. (Checked
by the lead: the ASCII file with `example.com` ran on 127.0.0.4 and the request
through the relay returned 200.)

### The foreground build after the rescope, 2026-11-14 (owner's run)

- The owner ran the 23:11 release build (`run --verbose`, administrator terminal,
  zapret removed). Owner: Steam works, Kaspersky raised no alert. Checked by the lead
  while it ran: one `snisplit` process, 2 snisplit rules, store HTTPS `200` via
  127.0.0.2, the binary on disk.
- Random split positions: the owner asked only out of interest; no change.

### Foreground acceptance, `m5.ps1`, elevated, 2026-11-14 23:19-23:20 (owner)

- Routed `run` ready after 703, 1789 and 2000 ms (two of three over the 1.5 s
  target; earlier runs 0.7-1.3 s; the cause of the spread is not known). Store HTTPS
  `200` via 127.0.0.2 each time; 0 rules after each Ctrl+C.
- Ctrl+C at 50, 100, 200, 500 ms (before the ready line) and 1000, 2000 ms (after):
  each exited on its own with code 0, `stopping`, `snisplit DNS rules left: 0`.
- Closing a running `snisplit run` window with its X button: the process exited with
  code 0 and 0 rules were left.
- `example.org` the same at start and end. After the run (checked by the lead): no
  `snisplit` process, 0 rules, the binary on disk.
- `m5.ps1` now refuses to start while a `snisplit` process runs, since it would
  remove that session's rules.

### Closing milestone 5 (owner, 2026-11-14)

- The owner skipped the restart check and reports that a Steam game download through
  `snisplit run` works with zapret removed (owner's report; no relay log was kept).
- Milestone 5 is done as rescoped. Carried forward: start-up time 0.7-2.0 s against
  the 1.5 s target, cause unknown, awaiting the owner's call; stop at logoff and
  shutdown implemented but not shown in a real run; the unidentified direct
  connection to `23.49.104.166:80` from milestone 4, not investigated further: it
  was not seen again, and both DNS cache dumps for `23.49.104.*` were empty.
