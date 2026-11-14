# Milestone 9: a small desktop UI (Windows)

Done when (PLAN.md section 4, row 9): the owner adds a site, sees whether it works, and
starts and stops snisplit without a terminal. Stack decided in D15: `egui` through
`eframe`, crate `crates/gui`, binary `snisplit-gui`, one elevated app, no helper process.

Status, 2026-11-15: built and checked overnight without administrator rights. The
elevated paths (Start that adds DNS rules, traffic through the relay, a restart after a
site change, closing while running) are covered by state-machine tests and wait for the
owner's run of `tools/acceptance/m9.ps1` (last section).

## What exists

- `crates/cli` is now a library `snisplit` plus the `snisplit` binary. `main.rs` keeps
  argument parsing (`args`) and the printed probe table (`probe`); everything a second
  front end needs is in the library.
- `crates/cli/src/sites.rs`: the per-user site folder.
- `crates/gui` (`snisplit-gui`):
  - `state.rs`: the UI as a pure state machine, no egui types: events in, effects out;
  - `report.rs`: a probe report turned into table rows, a verdict sentence and an offer;
  - `driver.rs`: runs the effects on a tokio runtime off the UI thread and sends results
    back over a channel, waking the window with a repaint request;
  - `window.rs` (Windows only, in the binary): the eframe window;
  - `main.rs`: the Windows entry point, and off Windows a stub that prints that the UI is
    Windows-only for now;
  - `build.rs`: embeds the `requireAdministrator` manifest on MSVC.
- `tools/acceptance/m9.ps1`: the owner's elevated walk-through.

## The user site folder

`%APPDATA%\snisplit\sites\<name>.profile` on Windows. Elsewhere, from environment
variables only (no new crate): `$XDG_CONFIG_HOME/snisplit/sites` when that variable is an
absolute path, otherwise `$HOME/.config/snisplit/sites`. This folder is unrelated to the
untracked `sites/` folder at the repo root.

- `snisplit run` with neither `--profile` nor `--profile-file` loads the built-in profiles
  and then every `.profile` file in the folder, sorted by file name. Loopback addresses
  follow the existing rule: built-ins first (steam 127.0.0.2, medium 127.0.0.3), then the
  site files from 127.0.0.4 in that order. Removing a site shifts the later ones; rules
  are rebuilt on every start, so nothing depends on a fixed address.
- A missing folder is not an error. A file that cannot be read or parsed is reported in the
  output by its file name only (never the full path, which holds the user name) and
  skipped; the others still load.
- A site file must be named after its profile: `name shop` lives in `shop.profile`. A
  mismatch is reported and skipped. This keeps one identity per site, so remove-by-name
  cannot hit the wrong file and two files cannot claim one name. Built-in names are
  rejected by the existing parser rule.
- **A domain that a built-in profile already covers.** Decision: the built-in wins.
  Covered means the same match the relay uses (`DomainList`, suffix at a label boundary):
  `store.steampowered.com` and `steampowered.com` are covered by the built-in steam
  profile, `notmedium.com` is not covered by medium. At `run`, such a domain is left out
  of the site with a line like `site news: cdn.medium.com left out, the built-in profile
  medium already covers it`; a site left with no domain is skipped. The same rule applies
  between site files, in file-name order. Reason: without it, NRPT's most specific rule
  would quietly move a built-in's subdomain to the user's strategy, and two profiles with
  the same exact domain were a fatal error before. Adding a site (UI or library) refuses
  a covered domain with `store.steampowered.com is already covered by the built-in profile
  steam`, and writes nothing.
- The folder is read when a session starts, so a restart after a change sees it.

## Public API

Library `snisplit` (crate `crates/cli`):

```rust
pub mod profile;
pub mod session;
pub mod sites;
pub mod system_dns;
pub use plan::{DEFAULT_PROBE_RUNS, DEFAULT_PROBE_TIMEOUT, DNS_PORT, PIECE_GAP,
               UPSTREAM_TIMEOUT, upstream_servers};
pub use profile_file::{Problem, ProfileError, ProfileSpec, parse_domain};
pub const VERSION: &str;
```

`profile` keeps `BUILT_IN`, `BuiltIn::spec`, `find` and `parse_file`; `read_file` became
crate-internal. `system_dns` exposes `servers()`. The other modules (`plan`, `profile_file`,
`routing`) stay private; only the items re-exported above are public.

`snisplit::session`:

```rust
pub struct RunOptions { pub verbose: bool, pub profiles: Vec<&'static str>,
    pub profile_files: Vec<PathBuf>, pub site_folder: Option<PathBuf>,
    pub routing: bool, pub upstreams: Vec<Ipv4Addr> }
pub type Output = Arc<dyn Fn(&str) + Send + Sync>;
pub fn stdout() -> Output;
pub fn stop_signals() -> io::Result<impl Future<Output = ()> + Send>;
pub async fn serve(options, stop_requested, out) -> io::Result<()>;
pub async fn serve_with_ready(options, stop_requested, out, ready: impl FnOnce() + Send) -> io::Result<()>;
pub fn routing_failure_cause(error: &io::Error) -> Option<&str>;
pub fn remove_rules(out: &Output) -> io::Result<()>;
```

`serve_with_ready` calls `ready` right after the start line, so the UI knows the relay
runs without matching text. A failure while removing stale rules or adding rules is
returned as an `io::Error` whose text is exactly what the command line printed before;
`routing_failure_cause` recognises it by type (no string matching) and gives the first
line of the PowerShell error.

`snisplit::sites`:

```rust
pub struct SiteFile { pub name: String, pub loaded: Result<ProfileSpec, String> }
pub fn folder() -> Option<PathBuf>;
pub fn list(folder: &Path) -> io::Result<Vec<SiteFile>>;
pub fn add(folder: &Path, profile_text: &str) -> io::Result<ProfileSpec>;
pub fn remove(folder: &Path, name: &str) -> io::Result<()>;
pub fn built_in_specs() -> io::Result<Vec<ProfileSpec>>;
```

`admit` (the coverage rule above) is crate-internal: only the session uses it. Every function takes the folder as a parameter; only the two `main`s call `folder()`, so
no test reads or writes the real `%APPDATA%`.

Library `snisplit_gui`: `state` (`State::update(Event) -> Vec<Effect>`, `site_list`),
`report::view`, `driver::Driver` (`new(folder, wake)`, `run(effect)`, `shutdown()`).

## The window

A status line with one Start/Stop button (disabled while stopping) and, when something
failed, the message on its own wrapped line; the site list (built-ins marked built-in
with Check only, user sites with Check and Remove, a broken site file with its problem
and Remove); an "Add a site" field with a Check button (Enter works too); the probe table
with the verdict sentence; and a log pane fed by the session's output lines. Plain default
egui styling, English text.

- Check probes the site's first domain and shows the table. The field probes the typed
  domain and offers "Add" when a profile is suggested, otherwise "Not blocked, nothing to
  add" or "No method worked". Decision: `NoProfileHelps` (something failed and no profile
  fixes it, for example HTTPS open and port 80 dead) also shows "No method worked"; the
  verdict sentence above it says what failed. Check on an existing site never offers Add.
- One probe at a time; a result is taken only for the domain in flight. Probes use the
  system DNS servers, 2 runs, 5 s timeout and the relay's piece gap, as `snisplit probe`.
- Adding or removing a site while the relay runs or starts stops the session and starts
  it again; the log says `site shop added; restarting snisplit so the change applies`.
  Changes that arrive while it is stopping fold into one restart. A failed stop cancels
  the restart. The relay is not restarted when it is stopped.
- Closing the window while running cancels the close (`ViewportCommand::CancelClose`),
  stops the session through the same stop path as Stop (the session removes its rules),
  and closes once the session has ended. A close during a pending restart cancels the
  restart. As a safety net, if eframe returns for any other reason, dropping the driver
  signals stop and waits up to 60 s for the session to end before the process exits.
- Without administrator rights, Start fails with: "Adding the DNS rules failed. snisplit
  needs administrator rights for that: close this window and start snisplit-gui again
  with Run as administrator. Windows said: ..." Everything else (site list, Check, Add,
  Remove, log, close) works unelevated. There is no up-front elevation check: that would
  need new `windows-sys` features, and the failing rule add already says exactly this.
- The session's start line still ends with "Ctrl+C to stop", which shows in the GUI log;
  left as is so the command line output is unchanged.

## Elevation

`crates/gui/build.rs` prints, only when `CARGO_CFG_TARGET_OS` is `windows` and
`CARGO_CFG_TARGET_ENV` is `msvc`:

```
cargo:rustc-link-arg-bins=/MANIFEST:EMBED
cargo:rustc-link-arg-bins=/MANIFESTUAC:level='requireAdministrator' uiAccess='false'
```

Checked with rustc 1.98.1 and the MSVC linker: the release `snisplit-gui.exe` contains
`<requestedExecutionLevel level='requireAdministrator' uiAccess='false' />`;
`snisplit.exe` and the gui crate's test binaries contain no `requireAdministrator`.
`rustc-link-arg-bins` also applies to a bin target's own test harness: with `test = true`
on the `[[bin]]`, `cargo test` failed to run it with "The requested operation requires
elevation. (os error 740)". So the bin has `test = false` and holds no tests; all tests
live in the gui library and `crates/gui/tests/`. Starting the exe unelevated without the
compatibility setting fails with the same elevation error, as intended.
`#![cfg_attr(windows, windows_subsystem = "windows")]` keeps a console from opening.

## Dependencies

- New direct dependency: `eframe` 0.37.0, only under `[target.'cfg(windows)'.dependencies]`
  of `crates/gui`, `default-features = false`, features `glow` and `default_fonts` and
  nothing else. No other new direct dependency (the gui crate also uses the workspace
  crates `snisplit`, `snisplit-core`, `snisplit-relay` and `tokio`, all already in the
  lockfile), and no new `windows-sys` or `tokio` feature.
- `crates/gui` has its own `rust-version = "1.95"`: eframe 0.37 declares 1.95, while the
  workspace says 1.85 for the other crates.
- Measured with `cargo tree -p snisplit-gui --target x86_64-pc-windows-msvc`: 98 packages
  for normal dependencies, the gui and the three workspace crates included, which is 88
  more than the 10 of `snisplit`; 104 with build-time dependencies (`autocfg`,
  `cfg_aliases`, `gl_generator`, `khronos_api`, `version_check`, `xml-rs`). D15 expected
  several hundred; the default-features-off build stays under one hundred.
- On Windows the GUI also links `windows-sys` 0.52.0 (through `glutin` and `winit`) next to
  the 0.61.2 the relay uses.
- `Cargo.lock` grew from 16 to 233 package entries, additions only (every name and version
  in the old file is still there); the lockfile lists packages for every target.
- `cargo tree -p snisplit`, `cargo tree -p snisplit -e features` and `cargo tree -p
  snisplit --target x86_64-unknown-linux-gnu` are byte-for-byte the same as before the
  milestone.
- On Linux and macOS the gui crate compiles without eframe (`cargo tree -p snisplit-gui
  --target x86_64-unknown-linux-gnu`: 12 packages), so CI needs no system packages. Only
  the Linux target was checked locally (clippy); macOS goes through the same
  `cfg(not(windows))` path.
- Release sizes: `snisplit-gui.exe` 7.5 MB, `snisplit.exe` 1.3 MB.

## Verified overnight, and how

Not elevated throughout (checked with `WindowsPrincipal.IsInRole(Administrator)`: False).
Before: `Get-Process snisplit*` empty, 0 NRPT rules. No antivirus block, termination or
missing binary was observed at any point; `snisplit.exe` and `snisplit-gui.exe` were on
disk at the end.

Tests: 512 pass on Windows (472 before): 9 site-folder tests (a tenth, the folder from
`XDG_CONFIG_HOME` or `HOME`, runs only off Windows and was only compile-checked here,
through the Linux-target clippy), 2 session tests with a site folder, 21 state-machine
tests, 3 report tests, 2 driver unit tests and 3 driver tests with a temporary folder. The
ready signal and the routing-failure cause are asserted inside existing session tests;
those tests, like before, bind loopback 127.0.0.4 ports 53 and 443. No new test touches
the internet, binds a port, changes DNS rules or writes outside the temp directory.
Gates: `cargo fmt --all --check`, `RUSTFLAGS="-D warnings" cargo clippy --workspace
--all-targets`, the same with `--target x86_64-unknown-linux-gnu`, and `cargo test
--workspace` passed before each commit, except the window commit (ca23adf), which had fmt,
both clippy runs and the gui crate's tests; the full workspace tests passed right after
it, before the next commit. macOS was not built or checked locally.

Real window, release build, started with `__COMPAT_LAYER=RunAsInvoker` (which lowers the
manifest's request to the caller's level) and `APPDATA` pointed at a temporary folder
holding one valid site file (`example-org`) and one broken one, so the real `%APPDATA%`
was not touched. Driven by synthetic mouse clicks and keystrokes, checked by screenshots:

- The window titled `snisplit` showed Start and "Stopped", steam and medium marked
  built-in with their domains, the broken file with its parse error and Remove (Check
  disabled), example-org marked "your site" with Check and Remove, the Add a site field
  and an empty log. (The first screenshots showed a blank white client area while the
  display was asleep after an hour without input; after one synthetic mouse move the
  same build rendered normally. The GL renderer was the NVIDIA driver, OpenGL 3.3.)
- Check on medium: spinner and "Checking medium.com", then the table for 162.159.152.4
  matching milestone 6 (whole hello reset, `tls-split 1` and `1 domain-mid` handshake,
  `host-start` and `domain-mid` reset, `Host:` reset, `hoSt:` 301).
- Typing in the field: the first attempt went through the keyboard's Vietnamese input
  method and arrived as non-ASCII text, which the field refused with "bad domain ...: only
  a-z, 0-9, '-' and '.' are allowed" and no probe. Typed again as Unicode key events,
  `example.org` + Enter: every attempt works, "Verdict: not blocked here; no profile
  needed" and "Not blocked, nothing to add".
- Remove on example-org: the file was deleted, the row disappeared, the notice and the log
  said "site example-org removed".
- Start without administrator rights: status "Failed" and the administrator message with
  "Windows said: powershell: Add-DnsClientNrptRule : Failed to load the NRPT
  information..."; the log showed "starting snisplit", "skipped a site: broken.profile:2:
  domains needs a value", "snisplit DNS rules left: 0" and the failure. NRPT rules: 0
  before and after.
- Close while starting (Start pressed, the close message sent at once): 150 ms later the
  window was still up, showing "Stopping..." with Stop disabled and the log lines "closing:
  stopping snisplit and removing its DNS rules" and "stopping"; the process exited by
  itself 758 ms after the close message, exit code 0. This is the cancel-close path; it
  ran the session's stop and rule removal (no rules existed unelevated).
- Close while failed or stopped: the process exited at once (185 ms), exit code 0.
- After the last close: no `snisplit*` process, 0 NRPT rules in total.

A bug found by the close-while-starting run and fixed: the first build waited for the
session at exit with a tokio timer created outside the runtime, which panicked after the
session had already ended and its rules had been removed (exit code 101). The wait now
runs inside the runtime; two driver tests fail on the old code and pass on the new.

Never exercised in a real window: Start that succeeds, Running, Stop, the Add button (no
blocked domain outside the built-ins was allowed to be probed tonight), a restart after a
site change, closing while running with rules present, the 60 s shutdown safety net, and
logoff or shutdown with the window open (not handled; a rule left behind is removed by
the next start or by `snisplit cleanup`).

## Awaiting the owner

Run from an elevated PowerShell in the repo, with no snisplit running:

```
cargo build --release -p snisplit-gui
powershell -ExecutionPolicy Bypass -File tools\acceptance\m9.ps1
```

The script refuses to run unelevated or while a `snisplit*` process exists, starts the
release GUI and pauses after each step, logging rule counts and results to
`$env:TEMP\m9-gui.log`. It parses cleanly (PowerShell parser, no errors) but has never
been run. `pwsh -File tools\acceptance\m9.ps1` works too: the script calls `curl.exe`
through a helper named `Fetch`, so Windows PowerShell's `curl` alias does not interfere.

1. The window shows the site list (confirms rendering on the owner's display).
2. Start: the status reaches Running, rules are added.
3. `curl.exe` to https://store.steampowered.com/ answers through 127.0.0.2.
4. Add a site through the field with a blocked domain the owner types (not a Steam or
   medium.com name): the probe table, Add, the restart line in the log, and that domain
   resolving to a relay address.
5. Stop: 0 rules.
6. Start again: rules are logged while it runs.
7. Close the window while it runs: the process exits by itself and 0 rules are left.

Also open: whether Kaspersky reacts to an elevated GUI that starts PowerShell (no alert
was possible tonight since nothing ran elevated), and whether a font with Vietnamese
letters is wanted (the default egui fonts show them as boxes; domains are ASCII anyway).
