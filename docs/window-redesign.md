# The window redesign (0.2.0)

The window from milestone 9 (docs/milestone-9.md) rebuilt to the owner-approved design:
a compact window with one large round button, a switch per site, rows that open in place,
and plain words. Built 2026-11-15. Version 0.2.0.

## The design rules

1. 400 px wide (fixed), 680 px tall by default, height resizable from 480 to 1600 px.
2. One large round button. Its text names the action (Start, Cancel, Stop, Try again,
   Wait); the line under it names the state. The shape carries the state without colour:
   a hollow ring is off, a ring with a turning arc is a transition, a filled disc is on,
   a double ring is a failure. All painted with egui's painter; the window repaints
   continuously only while the arc turns or a test runs, and every 15 s while on (for the
   minute counter).
3. Only egui's default fonts (Ubuntu-Light, Hack). Hierarchy by size and brightness.
4. A site row opens in place when its name or triangle is pressed. "Add a site" opens in
   place at the end of the list. One row or the add panel is open at a time.
5. Every site except a broken file has a painted switch with On or Off beside it. Only
   switched-on sites are relayed.
6. Plain words on the main screen; Activity keeps the technical lines.
7. Activity replaces the list under the small form of the header, with "Back to sites".
8. With many sites the list scrolls, long names end in an ellipsis, and the header takes
   its small form.

## Where the code is

- `crates/cli/src/sites.rs`: `off.list` (`off_list_path`, `switched_off`, `set_switch`) and
  the typed refusal `Covered` (`covered(&io::Error)`). `add` switches a re-added site on.
- `crates/cli/src/session.rs`: `Failure` and `failure(&io::Error)`; `serve` honours
  `off.list`.
- `crates/gui/src/state.rs`: the pure state machine (no egui types).
- `crates/gui/src/view.rs`: every string and button the window shows, from the state.
- `crates/gui/src/address.rs`: a typed address or pasted link to a domain.
- `crates/gui/src/report.rs`: a probe report in plain words, plus the technical lines.
- `crates/gui/src/log_file.rs`: "Save to a file".
- `crates/gui/src/driver.rs`: runs the effects off the UI thread.
- `crates/gui/src/window.rs` (Windows only): draws the view and emits events.
- `crates/gui/src/widgets.rs` (Windows only): round button, ring, switch, triangle, marks.

## Public API changes

`snisplit::sites`:

```rust
pub struct Covered { pub domain: String, pub owner: String, /* label */ }
pub fn covered(error: &io::Error) -> Option<&Covered>;
pub fn off_list_path(folder: &Path) -> PathBuf;
pub fn switched_off(folder: &Path) -> io::Result<Vec<String>>;
pub fn set_switch(folder: &Path, name: &str, on: bool) -> io::Result<()>;
```

`snisplit::session`:

```rust
pub enum Failure { Binding, ChangingRules, RemovingRules, NothingSwitchedOn, Other }
pub fn failure(error: &io::Error) -> Failure;
```

`snisplit_gui`: new modules `address`, `log_file`, `view`; `driver::Folders { sites, home }`
and `Driver::new(folders, wake)`; `state` reworked (events and effects below).

## Per-site switch (`off.list`)

- One file, `%APPDATA%\snisplit\off.list` (the parent of the `sites` folder), one
  switched-off site name per line, ASCII. Missing file: everything on.
- Names that match no site are ignored, and dropped on the next write. Blank and repeated
  lines are dropped too; bytes that are not UTF-8 are read lossily and so never match.
- Written atomically: `off.list.new` in the same folder, flushed to disk, then renamed over
  `off.list`. The names are written in the site-list order (built-ins, then site files by
  file name).
- Built-in sites can be switched off.
- `snisplit run` with neither `--profile` nor `--profile-file` honours it and prints
  `switched off: medium, shop` (matched names only). An explicit `--profile` or
  `--profile-file` ignores it (the site folder, and with it `off.list`, is only used
  without them).
- The coverage rule ("the built-in wins") is applied over all sites before the
  switched-off ones are dropped, so the window's Add and `run` agree.
- Every site switched off: `run` fails before any DNS rule is touched (no PowerShell is
  started), exit code 1, with: `every site is switched off: turn one on in snisplit-gui or
  take its name out of off.list in the snisplit folder, or choose sites with --profile or
  --profile-file`. The window disables Start instead.
- Adding a site whose name is still in `off.list` (removed while off, then added again)
  takes it out, so the new row is on.

## Failure causes

`session::failure` classifies by error type, never by message text. The session tags the
error where it happens:

| Where | `Failure` | Window headline sentence |
|---|---|---|
| Binding a relay or DNS listener | `Binding` | Another program is using something snisplit needs. Close other copies of snisplit, then try again. |
| Removing stale rules or adding rules at start (the existing `RoutingFailure`) | `ChangingRules` | Windows refused the change snisplit needs. If you did not start it with Run as administrator, close this window, right-click snisplit-gui and choose Run as administrator. |
| Removing the rules on any stop path | `RemovingRules` | Off, with leftovers (below) |
| Every site switched off | `NothingSwitchedOn` | Off, with the "Turn on at least one site" hint |
| Anything else | `Other` | snisplit stopped unexpectedly. |

The raw cause always goes to Activity (`snisplit stopped with an error: ...`), reached by
"See what happened", never into the headline. `kind()` and `to_string()` of every error are
unchanged, so the command line prints what it printed before.

One behaviour change in the session: a failed start whose rule removal also fails now
returns the removal failure (so the window shows leftovers) and prints the start error as
`snisplit could not start: ...`. Before, the removal failure was dropped.

## Every UI string by state

The header (round button, the state line under it, sentences, extra buttons; then the
small form's one sentence) and the right-hand footer text:

| State | Button | Shape | State line | Sentences | Footer |
|---|---|---|---|---|---|
| Off | Start | hollow | Off | The sites switched on below will not open until you press Start. | version 0.2.0 |
| Off, every site switched off | Start (disabled) | hollow, dim | Off | Turn on at least one site below, then press Start. | version 0.2.0 |
| Off after leftovers were removed | Start | hollow | Off | The leftover settings were removed. / The sites switched on below ... | version 0.2.0 |
| Starting | Cancel | arc | Starting... | About 2 seconds. | version 0.2.0 |
| On | Stop | filled | On | `<sites>` open through snisplit. / Keep this window open: closing it turns snisplit off. / If a site still does not open, close and reopen your browser. | on for under 1 min, on for N min |
| Applying a change, stopping half | Wait (disabled) | arc | Applying your change... | snisplit restarts for about 2 seconds. Sites may pause briefly. | several quick changes apply together |
| Applying a change, starting half | Cancel | arc | Applying your change... | (same) | (same) |
| Turning off after Stop or Cancel | Wait (disabled) | arc | Turning off... | (none) | version 0.2.0 |
| Closing | Wait (disabled) | arc | Turning off... | This window closes by itself in a moment. Closing it again now may leave sites unable to open until you start snisplit once more. | version 0.2.0 |
| Could not start (another program) | Try again | double ring | Could not start | Another program is using ... then try again. + "See what happened" | nothing was changed on this PC |
| Could not start (Windows refused) | Try again | double ring | Could not start | Windows refused the change ... Run as administrator. + "See what happened" | nothing was changed on this PC |
| Could not start (other) | Try again | double ring | Could not start | snisplit stopped unexpectedly. + "See what happened" | version 0.2.0 |
| Stopped by an unexpected error after it was on | Try again | double ring | Off | snisplit stopped unexpectedly. + "See what happened" | version 0.2.0 |
| Off, with leftovers | Start | double ring | Off, with leftovers | snisplit could not tidy up after itself. `<sites>` may not open until the leftover settings are removed. + "Remove leftover settings", "See what happened" | pressing Start also removes them |
| Leftovers, removal refused | Start | double ring | Off, with leftovers | (as above) / Windows refused to remove them. If you did not start it with Run as administrator, close this window, right-click snisplit-gui and choose Run as administrator. | pressing Start also removes them |
| Removing leftovers | Wait (disabled) | arc | Removing leftover settings... | (none) | version 0.2.0 |

`<sites>` lists the titles of the sites of the session: "Steam opens through snisplit.",
"Steam and example.com open through snisplit.", "Steam, Medium and example.com open
through snisplit.", or "7 sites open through snisplit." when there are more than three
or the names together are longer than 32 characters. The small header form shows the state
line and one sentence; when on: "`<sites>` open through snisplit. Closing this window turns
it off." The tones (background of the header): plain, blue when on or applying, brown for
failures and leftovers.

Footer, left: "Activity" on the site list, "N lines" (or "1 line") on the Activity page.
Right: the table above; in the small header form on the site list, "+ Add a site" instead.

Site rows: the title, then "comes with snisplit", "added by you", or for a file that does
not parse "This file has a mistake and is skipped: `<problem>`" (no switch). Switch label:
On, Off, or while a change applies "Turning on" / "Turning off" (switch disabled, knob in
the middle). Titles: built-ins with a capital (Steam, Medium); a site the window added
shows its domain (example.com); a hand-written file keeps its profile name.

An opened row: its domains; the last test as one sentence; buttons Test (Test again after
a test), Remove (user sites and broken files), "What snisplit tried" / "Hide what snisplit
tried" (after a test that ran) with the table. Test sentences:

- Tested `<domain>` just now: blocked without snisplit, and snisplit can open it.
- Tested `<domain>` just now: opens without snisplit.
- Tested `<domain>` just now: snisplit cannot open it from here.
- Could not test `<domain>` just now. Check your internet connection and try again.

("just now" in the first minute, then "N min ago".) While a row tests: a small turning
ring, "Testing `<domain>`... up to 10 seconds" (after 10 s: "Testing `<domain>`... this one
is taking longer than usual") and Cancel. Remove asks in the row: "Remove `<title>`?", plus
"snisplit restarts for about 2 seconds." when it would restart, with Remove / Keep.

The add panel ("Add a site"):

| Step | Mark | Text | Buttons |
|---|---|---|---|
| Typing | | field, hint "example.net or a pasted link" | Test, Close |
| Mistyped | | Type only the address, like example.net. A pasted link such as https://example.net/page is accepted and shortened. | Test, Close |
| Testing | | field locked showing the address; Testing `<domain>`... up to 10 seconds | Cancel |
| Blocked, can open | + | `<domain>` is blocked here, and snisplit can open it. | Add and turn on, Not now, What snisplit tried |
| Already opens | = | `<domain>` already opens normally. Nothing to add. / If it fails in your browser anyway, the cause is something snisplit does not handle. | Done, Test another, What snisplit tried |
| Nothing answered | x | snisplit cannot open `<domain>` from here. / Nothing snisplit tried got an answer. The site may be down, or blocked in a way snisplit cannot get around. | Close, Test another, What snisplit tried |
| Something answered, nothing helps | x | snisplit cannot open `<domain>` from here. / Part of what snisplit tried got an answer, but snisplit has no setting that opens the whole site. | Close, Test another, What snisplit tried |
| Could not test | ! | Could not test `<domain>`. / Check your internet connection and try again. | Try again, Close |
| Already covered | = | `<domain>` is already covered by `<title>`. | Close, Test another |
| Save failed otherwise | ! | Could not add `<domain>`. | Close, See what happened |

"What snisplit tried": attempts as Normal secure connection, Sent in pieces (1), Sent in
pieces (host-start), Sent in pieces (domain-mid), Sent in pieces (1 domain-mid), Normal
plain connection, Header respelled; each run as answered (`Answered`), cut off (`Reset`,
`Closed`) or no answer (`Unexpected`, `TimedOut`, `ConnectFailed`, `Failed`). Every test
also writes the technical table and the verdict to Activity.

Activity: the session's lines as before, at most 500. "Save to a file" writes
`snisplit-log.txt` (CRLF lines) to `%USERPROFILE%\Desktop` when that folder exists,
otherwise to `%APPDATA%\snisplit`. The note beside the button: "Saves snisplit-log.txt on
your Desktop to send to whoever helps you.", then "Saved snisplit-log.txt on your
Desktop.", "Saved snisplit-log.txt in the snisplit folder, %APPDATA%\snisplit." or "Could
not save the file: `<error>`". The place is named in words, never as a full path, because
the path holds the user name. No clipboard.

## Behaviour

- A switch change, an add, or the removal of a switched-on site while starting or on
  restarts the session ("Applying your change..."). Changes during the stopping half fold
  into the start that follows (the session reads the folder and `off.list` when it
  starts); a change during the starting half stops that start and restarts once more.
  The restart is written to the folder first: the driver writes synchronously and the
  restart is decided on the write's result event.
- "Applying" lasts from the change until the session is ready again (or fails, is
  stopped, or the window closes); `State::applying()`, `restart_pending()` and
  `closing()` are what the view reads.
- An explicit Stop or Cancel clears a pending restart and the applying state.
- Switching the last site off while on turns snisplit off instead of restarting into an
  error; during a restart's stopping half it cancels the restart.
- Removing a switched-off site, or a broken file, needs no restart.
- Leftovers: a stop or close whose rule removal failed shows "Off, with leftovers".
  "Remove leftover settings" runs `session::remove_rules` on a blocking thread
  ("Removing leftover settings...") and reports the result in the header and in Activity.
  Start still removes stale rules first.
- Closing: the first close while starting, on or applying stops the session and waits
  ("Turning off..."); the window exits when the session has ended. A second close while
  it is still stopping exits at once and logs that rules may remain. A close while
  leftovers are being removed waits for that result the same way.
- A failure while closing keeps the window open only when the rule removal failed
  (leftovers, so the owner sees it); the next close exits. Any other failure while
  closing exits, because the session now reports a failed removal explicitly and so
  every other failure means the rules were removed.
- Tests: one at a time; each has an id, Cancel aborts its task, and a late result with
  an old id is ignored. Closing the add panel or opening a row cancels the panel's test; a
  row's test keeps running when the row is closed and its result is shown when it is
  opened again. Steam is tested at store.steampowered.com, every other site at its first
  domain.
- The probe resolves through the adapter DNS servers itself and connects to the real
  address, so a test means the same with snisplit on or off. Read in
  `crates/relay/src/probe.rs` and `upstream.rs`: `UdpResolver` sends its query over UDP
  straight to the adapter's DNS servers (`system_dns::servers`, loopback dropped by
  `upstream_servers`), so the NRPT rules, which only steer the Windows DNS client, never
  apply; an answer that is loopback is refused (`is_usable_upstream`); each attempt is a
  `TcpStream::connect` to that address on 443 or 80, never through the relay.
- The address field: surrounding spaces, a scheme (`https://`), user info, port, path,
  query, fragment and one trailing dot are removed, the rest is lower-cased and must be a
  domain (`parse_domain`) whose last label is not all digits (so an IPv4 address is
  refused).

## Decisions made here that the brief did not specify

1. The starting half of a restart shows Cancel (it is a start; Cancel stops it) rather
   than the mockup's disabled Wait, which is kept for the stopping half.
2. The starting sentence is "About 2 seconds." The mockup's "The arc on the ring turns
   while snisplit starts." reads as the designer describing the ring, so it was left out.
3. With every site off, the disabled button keeps the label Start, as in the mockup.
4. Every failure offers "Try again"; leftovers keep "Start", as in the mockup.
5. An unexpected error after it was on says "Off" (not "Could not start").
6. "Testing ... up to 10 seconds" is not a bound: a test can take up to about 140 s (7
   attempts, 2 runs, each allowed 5 s to connect and 5 s to reply). After 10 s the text
   changes to "this one is taking longer than usual"; Cancel stays available. A hard
   10 s limit would have reported sites that drop rather than reset as "could not test".
7. Test results keep their time: "just now", then "N min ago".
8. The "cannot open" case where something answered gets its own sentence; the mockup's
   "Nothing snisplit tried got an answer" would be untrue there.
9. Outcome words: see the table above. `Unexpected` (bytes that are not a TLS or HTTP
   reply) and `ConnectFailed` (including refused) are "no answer".
10. Names fit when at most three are named and the names together have at most 32
    characters.
11. The small header form is chosen from the window height and the number of sites only
    (300 px full header, 58 px per row, 46 px add row, 44 px heading), not from what is
    open, so opening a row does not flip it. The Activity page always uses it.
12. In the small form the add row moves to the footer ("+ Add a site"); the panel opens
    at the end of the list and is scrolled into view.
13. The typing step of the add panel has a Close button (the mockup shows none).
14. Display titles and the Steam test address (above).
15. Footer texts per state (above); "on for under 1 min" in the first minute.
16. "Save to a file" names the place in words.
17. The whole set of `off.list` details above (order, lossy read, atomic rename name).

## Mockup items drawn differently in egui

- The marks (+, =, x, !), the disclosure triangles and the "+" of the add row are painted
  shapes, not text glyphs.
- The double-bordered failure ring is two thin painted rings.
- "on since 14:32" became "on for N min" from a monotonic clock, and the Activity lines
  carry no timestamps (std has no local time zone; no crate or new API was added).
- The mockup's title bar is the Windows title bar.
- The dotted "Label ....... outcome" lines became a two-column monospace grid.
- A centred row of buttons is centred by measuring the labels.
- One font weight, so the mockup's bold-free hierarchy maps directly to sizes 24, 19, 16,
  15, 14, 13 and 12 px and to brightness.

## Verified, and how

Not elevated throughout. Before starting: one `snisplit-gui` process was already running
(PID 51876, started 07:32 from Explorer, elevated: its path is not readable unelevated), 0
NRPT rules with comment `snisplit`. It was not touched.

Tests: 581 pass on Windows (521 before). New or changed:

- cli: 4 site tests (`off.list` place and missing file, writing and dropping unknown
  names, a built-in switched off before any folder exists, a re-added site comes back on),
  coverage refusal typed (in the existing test), 4 session tests (a refused removal on a
  stop, a failed start whose removal also fails, switched-off sites get no rule, every
  site off fails with no host call); `failure` asserted inside the existing missing
  upstream, failed add and bind failure tests (the last two are the existing tests that
  bind 127.0.0.4; no new test binds a port). Site, session and driver test folders are now
  nested so `off.list` never lands in the shared temp directory.
- gui: 47 state tests, 22 view tests (every string above that a state produces is
  written out by hand in a test), 4 report tests, 3 address tests, 2 log file tests, 5
  driver unit tests, 5 driver tests with temporary folders.
- No test touches the internet, changes DNS rules or writes outside the temp directory.

Gates before each commit: `cargo fmt --all --check`, `RUSTFLAGS="-D warnings" cargo clippy
--workspace --all-targets`, the same with `--target x86_64-unknown-linux-gnu`, `cargo test
--workspace`. macOS was not built locally; the window and widgets modules are Windows-only
and the library compiles for Linux.

`tools/acceptance/m9b.ps1`: parses in Windows PowerShell 5.1 and PowerShell 7; run
unelevated it refused with "Run this script from an elevated PowerShell (Run as
administrator). Nothing was changed." and exit code 1.

Real window, partly. The release build of the window first failed at 08:41:21 and
08:41:27 with `failed to remove file target\release\snisplit-gui.exe: Access is denied.
(os error 5)` while PID 51876 was still running (most likely started from that file; a
running executable cannot be deleted). Nothing was changed to get around it. At 08:45:37
that process had exited (0 rules); the same build was run again and succeeded at 08:45:53
(the exe holds `requireAdministrator`). The window was then started unelevated with
`__COMPAT_LAYER=RunAsInvoker` and `APPDATA` and `USERPROFILE` pointed at a temporary
folder holding one valid site file (`example-org`) and one broken one. GL renderer as
before; 400 x 680 client area at 96 DPI. Screenshots stayed in a temporary folder outside
the repo. Seen on screen:

- Off: the hollow ring with Start, "Off", "The sites switched on below will not open until
  you press Start.", Steam and Medium "comes with snisplit", the broken file dimmed with no
  switch and "This file has a mistake and is skipped: bro..." cut with an ellipsis,
  example.org "added by you", the On switches, "Add a site", footer "Activity" and
  "version 0.2.0".
- The Medium row opened in place (medium.com, Test). Test on medium.com: done in about
  1.6 s, then "Tested medium.com just now: blocked without snisplit, and snisplit can open
  it." with "Test again" and "What snisplit tried". The probe saw 162.159.153.4: whole
  hello and host-start and domain-mid reset, `tls-split 1` and `1 domain-mid` handshake,
  plain `Host:` reset, `hoSt:` 301, the same pattern as milestone 6.
- Activity: the small header (56 px ring with Start, "Off" and the sentence), "Back to
  sites", the lines, "Save to a file" with its note, footer "2 lines".
- Close while Off: the process exited.

The run was stopped at 08:53 because the window received mouse input that was not sent
by the test driver: during the first run the two temporary site files were removed
through the window (Activity: `site broken removed`, `site example-org removed`, each of
which needs a row opened, Remove and the confirming Remove), and a debug build with a
temporary event trace (not committed, removed afterwards) then showed a click landing
18 px and 29 px away from where the driver had put the pointer and a second click,
pressed and released in separate frames, that the driver never sent (its clicks arrive
as one press and release in the same frame). That looks like someone using the mouse
at the same time, so synthetic input was stopped and the window closed with a window
message. The same steps on the trace build, with the pointer moved away between clicks,
produced exactly one event per click. No snisplit process and 0 NRPT rules with comment
`snisplit` at 08:53:29.

Not seen in a real window: the add flow (testing, every outcome, the covered refusal,
the mistyped hint, Cancel), a switch turned off and its `off.list`, Remove with its
question, Start pressed unelevated ("Could not start" with "Windows refused the
change"), Save to a file, the many-sites small header, and every state that needs a
successful Start (On, Applying, Turning off with rules, closing while on, leftovers after
a real failed removal), which needs the owner's elevated run anyway.

## Awaiting the owner

1. The release build of 0.2.0 is in `target\release` (built 08:45:53).
2. The rest of the unelevated smoke run, at a time when nobody else uses the mouse: Test
   on example.org, the add flow on example.net or example.org (and medium.com for the
   covered refusal), a switch and its `off.list`, Start unelevated showing "Windows
   refused the change" with 0 rules, Save to a file, close.
3. `tools\acceptance\m9b.ps1` from an elevated PowerShell (log in `%TEMP%\m9b-gui.log`).
4. Whether Kaspersky showed anything, and whether the window looks right on the owner's
   display (fonts, sizes, the dark colours).
5. Still open from before: logoff or shutdown with the window open; the browser fallback
   from HTTP/3 (docs/milestone-8.md); the draft release (docs/milestone-7.md).
