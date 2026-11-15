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
- `crates/cli/src/instance.rs`: the one-instance lock file, `%ProgramData%\snisplit\instance.lock`.
- `crates/gui/src/driver.rs`: runs the effects off the UI thread; `Keeper` outlives the
  window.
- `crates/gui/src/window.rs` (Windows only): draws the view and emits events.
- `crates/gui/src/widgets.rs` (Windows only): round button, ring, switch, triangle, marks.

## Public API changes

`snisplit::sites`:

```rust
pub struct Covered { pub domain: String, pub owner: String, label: String }
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

Changes 1-4 (below) add `snisplit::instance` (`Instance` holds the open lock file and the open lock folder):

```rust
pub struct Instance { _open: File, _folder: File }
pub enum Claim { Ours(Instance), Elsewhere, Unlocked(io::Error) }
pub fn folder() -> Option<PathBuf>;
pub fn lock_path(folder: &Path) -> PathBuf;
pub fn claim(folder: Option<&Path>) -> Claim;
```

`snisplit::session`:

```rust
pub fn count_rules(out: &Output) -> io::Result<usize>;
pub fn cleanup(lock_folder: Option<&Path>, warn: impl FnOnce(&str), out: &Output) -> io::Result<()>;
```

`snisplit_gui::driver`: `Driver::new` is replaced by a `Keeper` (the runtime, the
session handle and the lock) that hands out the `Driver`:

```rust
pub struct Folders { pub sites: Option<PathBuf>, pub home: Option<PathBuf>, pub lock: Option<PathBuf> }
pub struct Keeper { runtime: Option<Runtime>, tracked: Mutex<Tracked> }
impl Keeper {
    pub fn new() -> io::Result<Keeper>;
    pub fn driver(&self, folders: Folders, wake: Wake) -> (Driver<'_>, Receiver<Event>);
    pub fn finish(self) -> Ending;
}
pub enum Ending { Clean, RulesMayRemain, TimedOut }
```

`snisplit_gui::state`: `Relay::AlreadyOpen`; events `Launched`, `InstanceClaimed`,
`ClaimAgainPressed`, `LeftoversCounted`; effects `ClaimInstance`, `CountLeftovers`;
`RowPressed`, `SwitchPressed`, `TestPressed`, `Open::Site` and `Target::Site` carry a
`SiteId`:

```rust
pub enum Origin { BuiltIn, File }
pub struct SiteId { pub name: String, pub origin: Origin }
pub enum Claimed { Ours, Elsewhere, Unlocked(String) }
```

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
| Off, with leftovers found at launch | Start | double ring | Off, with leftovers | snisplit did not get to turn off properly last time. Some of its settings are still in place, so the sites below may not open. + "Remove leftover settings", "See what happened" | pressing Start also removes them |
| Already open (another snisplit holds the lock) | Try again | double ring | Already open | snisplit is already running, or is still turning off. Wait a few seconds, then press Try again. (switches disabled) | version 0.2.0 |

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
"snisplit restarts for about 2 seconds." when it would restart, with Keep / Remove (Keep
first). The question goes away when its row is closed, another row or the add panel is
opened, the site list reloads, the page changes, or Escape is pressed. Activity records
`remove <name> asked` and `remove <name> confirmed`.

The add panel ("Add a site"):

| Step | Mark | Text | Buttons |
|---|---|---|---|
| Typing | | field, hint "example.net or a pasted link" | Test, Close |
| Mistyped | | Type only the address, like example.net. A pasted link such as https://example.net/page is accepted and shortened. | Test, Close |
| Mistyped, the host has letters outside ASCII | | Addresses with letters other than a to z, such as accented letters or other scripts, are not supported yet. Type an address like example.net. | Test, Close |
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
otherwise to `%APPDATA%\snisplit`, replacing a file of that name saved earlier. The note
beside the button: "Saves snisplit-log.txt to send to whoever helps you, and shows where it
went.", then "Saved as `<full path of the file written>`" or "Could not save the file:
`<error>`". The full path is shown because `%USERPROFILE%\Desktop` is not always the
Desktop the user sees (OneDrive folder backup moves it; an administrator run may use
another account's profile). The path appears only in the note, never in Activity, since
Activity is what gets saved and sent. No clipboard.

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
  it is still stopping closes the window at once and logs that rules may remain; the
  process then waits for the removal after the window is gone (change 1 below). A close
  while leftovers are being removed waits for that result the same way.
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
- The address field: surrounding spaces, a scheme (`https://`) or a bare leading `//`,
  user info, port, path (which starts at `/` or `\`), query, fragment and one trailing dot
  are removed, the rest is lower-cased and must be a
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
    (275 px for the full header when on, 68 px per row, 51 px for the add row, 38 px for the heading, measured on the Off screenshot: at 680 px four sites keep the full header and a fifth switches to the small one), not from what is
    open, so opening a row does not flip it. The Activity page always uses it.
12. In the small form the add row moves to the footer ("+ Add a site"); the panel opens
    at the end of the list and is scrolled into view.
13. The typing step of the add panel has a Close button (the mockup shows none).
14. Display titles and the Steam test address (above).
15. Footer texts per state (above); "on for under 1 min" in the first minute.
16. "Save to a file" shows the full path it wrote (changed in the review fixes; it first
    named the place in words).
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
folder holding one valid site file (`example-org`) and one broken one; 400 x 680 client
area at 96 DPI. Screenshots stayed in a temporary folder outside the repo; two that had
captured other programs' windows instead of snisplit were deleted unseen beyond that.

Release build, 08:47 to 08:50, driven by synthetic clicks:

- Off: the hollow ring with Start, "Off", "The sites switched on below will not open until
  you press Start.", Steam and Medium "comes with snisplit", the broken file dimmed with no
  switch and "This file has a mistake and is skipped: bro..." cut with an ellipsis,
  example.org "added by you", the On switches, "Add a site", footer "Activity" and
  "version 0.2.0".
- The Medium row opened in place (medium.com, Test).
- The click on Test produced no test. Instead both temporary site files were removed
  through the window (Activity: `site broken removed`, `site example-org removed`), the
  second while the driver sent nothing for 12 s. Each removal needs a row opened, Remove
  and the confirming Remove. This is attributed, not explained: the trace below shows
  input the driver did not send, but this run was not traced. One possible path: egui
  treats Space or Enter on a focused clickable widget as a click (egui 0.37,
  `context.rs`) and Tab moves the focus, so keystrokes reaching the window after the
  driver brought it to the front could open a row and press Remove twice; the trace did
  not record keys. Remove on the release build is therefore unverified.
- Activity (reached by a click): the small header (56 px ring with Start, "Off" and the
  sentence), "Back to sites", the two lines, "Save to a file" with its note, footer
  "2 lines". This screenshot showed a layout bug, fixed afterwards (below).
- Close while Off: the process exited.

Debug build with a temporary event trace to stderr (each window event and each pointer
and focus event; not committed, removed afterwards), 08:50 to 08:53, same temporary
folders with the two site files written again:

- One click on the Medium name gave exactly one `RowPressed("medium")`; one click on Test
  gave one `TestPressed`, and the probe finished in about 1.6 s with "Tested medium.com
  just now: blocked without snisplit, and snisplit can open it.", "Test again" and "What
  snisplit tried" on screen. The probe saw 162.159.153.4: whole hello, host-start and
  domain-mid reset, `tls-split 1` and `1 domain-mid` handshake, plain `Host:` reset,
  `hoSt:` 301, the same pattern as milestone 6.
- A pointer resting on a site name while the window was refocused produced no event.
- Replaying the first run's steps: the driver's first click arrived 18 px right and 29 px
  lower than where it had put the pointer, and a second click that the driver never sent
  arrived, pressed and released in separate frames (the driver's clicks arrive as one
  press and release in the same frame), plus focus-lost events. The trace recorded only
  pointer and focus input, so keystrokes would not show. This is input the driver did
  not send, so synthetic input was stopped and the window closed with a window message.
  No snisplit process and 0 NRPT rules with comment `snisplit` at 08:53:29.
- At 09:00:36 the system idle time (`GetLastInputInfo`, read only) was 672 ms with no
  input sent since 08:53, so no further pointer input was used.

Release build after the layout fix (20b551a), 09:01, no input at all: the window was
started with a seeded folder, its content captured with `PrintWindow` (no focus change,
no screen copy), then closed with a window message:

- Medium switched off through `off.list` (`medium`): Medium dimmed with an Off switch,
  Steam and example.com On, the Off sentence; as the mockup's first window.
- Every site switched off (`steam`, `medium`): the dimmed ring with Start disabled and
  "Turn on at least one site below, then press Start."
- Ten sites: the small header (56 px ring, "Off" and the sentence), tight rows without
  the second line, a long name cut with an ellipsis (that file's name exceeds the
  32-character profile name limit, so it showed as a broken file with no switch), and
  "+ Add a site" in the footer.

Fixed after the screenshots and not seen again in a real window: the Activity save row ran
under the footer (the log area did not count its frame margin; the row now sits in its own
bottom panel), and a pressed site name showed a text selection (names are no longer
selectable).

Not seen in a real window: the add flow (testing, every outcome, the covered refusal, the
mistyped hint, Cancel), Test on example.org, a switch clicked and the `off.list` it
writes, Remove with its question, Start pressed unelevated ("Could not start" with
"Windows refused the change"), Save to a file, the Activity page after the fix, and every
state that needs a successful Start (On, Applying, Turning off with rules, closing while
on, leftovers after a real failed removal), which needs the owner's elevated run anyway.

## Review fixes

An independent review of 7cbf8c2..0c8d793 found these; each is fixed with a test.
Tests: 592 pass on Windows (581 before): cli 2 site tests and 1 session test, gui 4 state
tests, 2 address tests, 2 driver unit tests.

1. `sites::add` rewrote `off.list` before creating the site file, so a refused add whose
   name collided with a switched-off site (`news.example.com` and `news-example.com` give
   the same name) switched that site back on. `off.list` is now written only after the
   file is fully written; if that write fails, the new file is removed again.
2. A rule removal that leaves rules in place (`snisplit DNS rules left: N`, N > 0) is now
   an error tagged `RemovingRules`: the window shows "Off, with leftovers", and
   `snisplit cleanup` and `snisplit run` exit with code 1 in that case.
3. If the session task or the leftover removal task panics or is cancelled, the driver
   still reports its end: the session as a `RemovingRules` failure ("the session crashed,
   so DNS rules may remain"), the removal as refused. Before, the window stayed in
   "Turning off..." for good.
4. Switching the last site off during the starting half of a restart clears the applying
   state, so the header says "Turning off..." rather than "Applying your change...".
5. The remove question is dropped when its row is closed, the list reloads, the page
   changes, or Escape is pressed; Activity traces `remove <name> asked` and
   `remove <name> confirmed`; Keep comes before Remove; each row's widget ids are salted
   with the site name (and whether it is a broken file, since a broken `steam.profile`
   lists under the same name as the built-in).
6. "Save to a file" shows the full path it wrote, since `%USERPROFILE%\Desktop` may not be
   the visible Desktop; the hint no longer promises the Desktop.
7. `off.list` ignores a leading UTF-8 byte order mark (as Notepad may write).
8. The address field ends the host at `\` (so `https://example.com\@example.net` is
   example.com, as a browser reads it), accepts a leading `//`, and gives its own hint
   when the host has letters outside ASCII.

Not seen in a real window: Escape, the Keep / Remove order on screen, the row id salt,
and how a long path wraps in the save note.

## Owner's elevated run of tools/acceptance/m9b.ps1, 2026-11-15 10:00-10:04, binary built from 0c8d793 (before the review fixes)

- Start: 3 rules; the store over HTTPS answered 200 and over HTTP 302, through the relay.
- Medium switched off while on: 2 rules, medium.com resolving directly; switched back
  on: 3 rules.
- A site added while on: 4 rules, and the added site resolved to the relay (HTTPS
  status 403).
- Removing it while on: 3 rules and 1 site file.
- Stop: 0 rules.
- Closing with X while on: the process exited with code 0, and 0 rules were left.
- The owner saw "Applying your change..." and reported nothing from the antivirus.

The review fixes above and changes 1-4 below were made after this run and have not
been seen in a real window.

## Changes 1-4 after the owner's run

Logoff, shutdown and any abrupt kill (Task Manager, a crash, power loss) are handled by
recovery at the next launch (change 2), not at the moment they happen. A handler for the
Windows end-session messages (`WM_QUERYENDSESSION` / `WM_ENDSESSION`) that would remove
the rules while the session ends was considered and deferred; it is not built.

1. **No frozen window on the second close.** Before, a second close while stopping
   emitted `Effect::Exit`; eframe drops the App before it destroys the window, and the
   App owned the `Driver`, whose `Drop` waited up to 60 s for the session on the UI
   thread, so the window showed "Not responding". Now `run()` in `window.rs` owns a
   `driver::Keeper`: the tokio runtime, the stop handle of the running session, the task
   that reports the end of the latest session or leftover removal, and the
   one-instance lock (change 3). The App holds a `Driver<'keeper>` that only borrows the
   keeper, and nothing in the App blocks when it is dropped. After `eframe::run_native`
   returns (the window is gone), `run()` calls `Keeper::finish`: it sends the stop (in
   case the window ended while the session ran), waits up to 60 s for the latest session
   or leftover removal to end, shuts the runtime down with a 1 s limit, releases the
   lock, and only then returns. Exit code 0 when the latest session ended cleanly, or
   failed for a reason other than removing its rules, or the latest leftover removal
   succeeded, or nothing was started; exit code 1 when its rules may remain (a failed or
   crashed removal), when the wait gave up, or when the window could not be created.
   Exit paths:
   - First close while off, failed or with leftovers: `Exit` at once; nothing to wait
     for after the window, except the result of an earlier session or removal, which is
     already known. So a close from "Off, with leftovers" after a failed stop exits with
     code 1; leftovers found at launch and not removed exit with code 0, since this run
     made no rules.
   - First close while starting, on or applying: stop, "Turning off...", the window
     waits; when the session ends cleanly it exits, and the wait after the window is
     immediate (code 0).
   - Failed removal at the first close: the window stays open showing "Off, with
     leftovers" (as before); the next close exits with code 1, or with code 0 if
     "Remove leftover settings" succeeded in between.
   - Second close while still stopping (or while removing leftovers): the window closes
     at once; the process waits up to 60 s after the window is gone for the removal.
   - The removal fails or does not finish in 60 s after the window is gone: no window to
     report in; exit code 1, and the next launch shows the leftovers (change 2).
2. **Leftovers shown at launch.** At launch the window claims the lock (change 3) and,
   if it got it or could not take a lock, counts the snisplit rules on a blocking thread
   (without a lock, a count above 0 is only logged; see the review fixes below)
   (`session::count_rules`, `@(Get-DnsClientNrptRule | Where-Object { $_.Comment -eq
   'snisplit' }).Count`, through the same host abstraction removal uses; nothing is
   removed and the DNS cache is not cleared). Activity gets `snisplit DNS rules in place:
   N`. Above 0, the window enters "Off, with leftovers" with the sentence in the table
   above and the existing button; Start still removes stale rules first. A failed check
   logs `could not check for leftover DNS rules: <cause>` and stays Off. The first paint
   is not delayed (the count runs off the UI thread); a result that arrives after Start
   was pressed is ignored, also when Start was followed by Stop and the window is Off
   again (that count may have seen the live session's rules).
3. **One instance at a time.** `snisplit::instance::claim(folder)` opens
   `instance.lock` in the machine-wide folder `%ProgramData%\snisplit` (read from the
   `ProgramData` environment variable; the review fixes below moved it there from the
   per-user `%APPDATA%\snisplit`), creating the folder if needed, with read access only
   and no sharing on Windows (`share_mode(0)`), and the open handle is the lock: a file
   left by a killed process does not block. Only a sharing violation (OS error 32)
   counts as held elsewhere; any other error, or no `ProgramData`, means no lock,
   logged, and snisplit goes on.
   Nothing is ever deleted. On other targets the file is opened without locking (the
   API compiles; the semantics are Windows only). The window keeps the lock in the
   keeper until the post-window wait is over, so a window that is gone but still
   turning off still holds it. Held elsewhere: "Already open" (table above); Try again
   claims again; Start, leftover removal and the switches are refused. `snisplit run`
   with routing takes the lock and, if it is held, exits with code 1 and `snisplit:
   error: another snisplit is already running, or is still turning off: wait a few
   seconds, then try again`. `snisplit cleanup` holds the lock while it removes the
   rules (review fix 3 below); if it is held elsewhere it prints `snisplit: warning:
   another snisplit is running and will lose its DNS rules` and goes ahead.
4. **A broken site file named like a built-in.** A row is identified by its name and
   where it comes from (`SiteId { name, origin: BuiltIn | File }`). Opening a row, the
   remove question, Remove, tests and switches address the id. A broken `steam.profile`
   is its own row: opening it opens only it, Remove asks "Remove steam?" and deletes the
   file, the built-in Steam row stays with its switch and has no Remove, and deleting
   the file while on neither drops the built-in row nor restarts the session. A valid
   hand-written file named like a built-in is also its own row; both rows share the
   `off.list` entry of that name, so switching one switches both.

Tests: 620 pass on Windows (592 before). cli: 5 lock tests (a second claim refused
while the first is held and accepted after it is dropped, a stale lock file, a missing
folder, a folder that cannot be made, no per-user folder), the count script text and
its parse, the count through the fake host with no removal. gui: 7 keeper tests
(dropping the driver while a slow fake session stops returns in under 250 ms and the
wait after sees a clean end; a crash while stopping after the window is gone means
rules may remain, and the wait sends the stop itself; a failure for another reason is
clean; a session that ignores the stop is given up after the limit; a successful
leftover removal after a failed stop is clean; a refused removal is not; a stuck
blocking task does not hold the exit), the report tests assert the returned outcome,
11 state tests (launch, leftovers found, none, failed check, a result after Start and
after Start-Stop, no lock, Already open and Try again, closing while Already open, and
three for the broken `steam.profile`), 3 view tests (the leftovers-at-launch and
Already open headers written out by hand, the broken `steam.profile` row), 3 driver
tests with temporary folders (the lock stays with the keeper after the window side is
dropped until `finish`, no folder means unlocked, deleting a broken `steam.profile`
removes that file and the built-in stays listed). No test starts PowerShell for DNS
rules, counts or removes real rules, or touches `%APPDATA%\snisplit`.

Not seen in a real window: everything in changes 1-4 (the second close leaving the
window at once, the exit codes, the leftovers sentence at launch, Already open and Try
again, the broken file row), and the review fixes above. `tools\acceptance\m9b.ps1`
now has steps for a kill and the leftovers at the next launch, and for a second window.

## Review fixes to changes 1-4

An independent review of 39879e8..c3b4196 found these; each is fixed, with tests where
the code can be reached without a window.

1. **A panic on the UI thread left the rules in place.** winit re-raises a UI panic out
   of `eframe::run_native`, so `Keeper::finish` never ran and the runtime's drop
   cancelled the session before it removed its rules. The keeper's runtime is now an
   `Option` that one wind-down takes: `finish` runs it, and `Drop` runs the same
   bounded stop-and-wait (60 s, then a 1 s runtime shutdown) when `finish` has not, also
   while unwinding. It cannot run twice; the exit code logic is unchanged.
2. **The lock was per user, the rules are machine-wide.** A second account (or "run as"
   another administrator) took its own lock, counted the first user's live rules and
   was offered "Remove leftover settings". The lock is now
   `%ProgramData%\snisplit\instance.lock`, opened with `FILE_READ_DATA` only and
   `share_mode(0)`: read access is the least that the exclusive share mode checks
   against (an open with no data access skips the share check, so it would lock
   nothing), and the default `%ProgramData%` permissions give other accounts read but
   not write access to a file one account created, so asking for write access would
   turn the second account's claim into "no lock". std refuses `create(true)` without
   write, so the options say write and create (open always, never truncate) and
   `access_mode` replaces the requested access. The file is never written, truncated or
   deleted. The folder comes from the `ProgramData` variable in `main.rs` and
   `window.rs` only; `Folders::lock` carries it into the driver, so tests inject a
   temporary folder. When the lock cannot be taken for a reason other than a holder
   (no `ProgramData`, a folder that cannot be made, access denied), snisplit goes on
   unlocked as before, and the window then does not enter "Off, with leftovers" from
   the launch count: it logs `N snisplit DNS rules are in place; with no lock they may
   belong to another running snisplit, so they are not offered for removal (Start
   still removes them)`. A late `Elsewhere` after this window took the lock is ignored.
   Hardening, since any local account can pre-create the folder and snisplit runs
   elevated: the folder itself is opened as a handle (list and read-attributes access,
   `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`, sharing read and write
   but not delete) and its attributes are taken from that handle; it must be a real
   folder without a reparse point (junctions and symbolic links are reparse points).
   The handle is kept for the life of the claim, so while it is open the folder cannot
   be renamed or deleted, and so cannot be swapped for a link between the check and the
   lock-file open. A second snisplit opens the folder the same way and gets as far as
   the lock file, so it still reports held elsewhere. An existing lock file is read
   with `symlink_metadata` and must be a regular file without a reparse point, and it
   is opened with `FILE_FLAG_OPEN_REPARSE_POINT`; both act on the last path component
   only, which is why the folder is pinned by its handle. A refusal means no lock, with
   the reason in Activity or on stderr. No DACL is changed.
3. **`snisplit cleanup` dropped its claim before removing the rules.** It now holds it
   until the removal has finished (`session::cleanup`). Held elsewhere: the warning,
   then the removal, as before. No lock: `snisplit: warning: not checking for another
   running snisplit: <reason>` (the line `run` prints), then the removal.
4. **Missing tests** for `run` and `cleanup` with the lock held (below).
5. **Hide before closing.** Where the window acts on `Effect::Exit` it sends
   `ViewportCommand::Visible(false)`, then `Close`, then asks for a repaint, so the
   window cannot sit on screen unresponsive while the wait after it runs (winit only
   posts the destroy message). eframe 0.37 still runs the logic pass of a hidden root
   viewport and paints invisible windows directly on Windows, so the `Close` is handled.
6. **`tools/acceptance/m9b.ps1`.** Everything after the first window start runs in
   `try { } finally { }`: an abort (Ctrl+C, an error) removes any rules pointing at the
   relay and logs that it did, also between the deliberate kill and the leftover
   removal; a completed run leaves the finally block idle, so "left running for you to
   look at" stays as it was. The kill uses the Process object's `Kill()` guarded by
   `HasExited`, never a process id. When the surviving window is the second one, step
   12 says to press Try again first. Parsed with the language parser under Windows
   PowerShell 5.1 and pwsh 7 (0 errors); not run.

Known risks, not fixed:

- A PowerShell call stuck beyond the 60 s limit keeps running after the process has
  released the lock and exited its wait; if it is a removal, it could remove the rules
  of a new instance started in the meantime.
- Another local account can hold or pre-create the lock so that snisplit says "Already
  open" (a local denial of service, the same class as that account binding the
  loopback ports snisplit needs). The way out is `snisplit cleanup`, which warns and
  proceeds. The lock is advisory between cooperating snisplit processes, not a security
  boundary. Before the folder was held as a handle, an account controlling
  `%ProgramData%\snisplit` could swap the folder for a junction after the check, and
  the elevated snisplit then created or held `instance.lock` in a folder of that
  account's choosing. With the handle held from the check until the claim ends, the
  folder can no longer be renamed or deleted, so that swap no longer redirects the file
  open. That account can still make snisplit run unlocked (refuse or block the folder)
  or say "Already open" (hold the lock file). Not ruled out: turning the folder into a
  junction in place, without renaming it, while it is still empty (setting a reparse
  point may need only write-attributes access, which a share mode does not gate); std
  offers no way to open the file relative to the folder handle, and this case is not
  tested.
- The cli outside Windows has no `ProgramData` and so runs unlocked; routing is Windows
  only anyway.

Tests: 637 pass on Windows (620 before). cli: 7 lock tests (a lock file that cannot be
written is still claimed and still excludes a second claim, the lock path under a given
`ProgramData` written out by hand, an unset or empty `ProgramData`, a lock path that is a
folder, a lock folder that is a junction and one that is a symbolic link with nothing
created in the target, a lock file that is a symbolic link with its target untouched;
the link tests return early if the link cannot be made, and on this machine they ran), 3
cleanup tests through the fake host (held elsewhere: the warning and one removal; ours:
the lock is still held during the removal and free after it; no lock folder: the
removal still runs and its refusal is returned), 1 binary test (a routed `run` while the
lock is held exits with code 1 and the one-line message; the child gets a temporary
`ProgramData` and `APPDATA` and a missing profile file, so even a broken lock check
stops before any DNS change). gui: 2 keeper tests (a keeper dropped without `finish`,
and one dropped by a panic unwinding past it, both send the stop to a slow fake session
and wait for its end), 3 state tests (rules found without a lock are logged, not offered,
and Start still runs; `Elsewhere` while Off at launch shows Already open; a late
`Elsewhere` after the lock was taken changes nothing), 1 view test (no leftovers button
without a lock).

Not seen in a real window: the hide before the close, the drop path after a real UI
panic, and the machine-wide lock with two accounts.

The owner ran two on-demand antivirus scans on 2026-11-15, of the release folder and then of the whole repository; neither reported anything. These were file scans of the builds from before changes 1-4 and say nothing about behaviour-based detection or about the CI-built release zip.

## Awaiting the owner

1. The release build of 0.2.0 is in `target\release` (built after the review fixes to
   changes 1-4).
2. The rest of the unelevated smoke run, at a time when nobody else uses the PC: Test
   on example.org, the add flow on example.net or example.org (and medium.com for the
   covered refusal), a switch and its `off.list`, Remove, Start unelevated showing
   "Windows refused the change" with 0 rules, Activity and Save to a file, close; and why
   the two site files were removed in the first run, if it happens again.
3. `tools\acceptance\m9c.ps1` (unattended, log in `%TEMP%\m9c.log`), then
   `tools\acceptance\m9b.ps1` (about 14 clicks in the window, log in
   `%TEMP%\m9b-gui.log`), both from an elevated PowerShell; see the last section.
4. Whether Kaspersky showed anything, and whether the window looks right on the owner's
   display (fonts, sizes, the dark colours).
5. Still open from before: logoff or shutdown with the window open (now expected to be
   shown as leftovers at the next launch); the browser fallback
   from HTTP/3 (docs/milestone-8.md); the draft release (docs/milestone-7.md).

## Owner's elevated run of tools/acceptance/m9b.ps1, 2026-11-15 11:28-11:32, binary built at cb1cdc3

This run used the release build of 11:06, which has changes 1-4 and their review fixes
but not the lock folder handle (921a2c7) or the cleanup warning (80beab3).

- Start, Medium switched off and on while on, a site added and removed while on, and
  Stop gave the same rule counts as the earlier run; the added site resolved to the
  relay (HTTPS status 403).
- Killed while on: 3 rules were left. The window started again showed the leftovers
  sentence and "Remove leftover settings" (the owner answered yes); pressing it gave 0
  rules.
- A second window while the first was open said "Already open" with Try again (the owner
  answered yes) and exited with code 0 when closed.
- Closing with X once while on: exit code 0 and 0 rules.
- Closing with X twice in a row while on: exit code 0 and 0 rules, 8 seconds from the
  script's prompt to the process ending, which includes the time the owner took to
  click. The owner left the script's question unanswered during the run and answered
  afterwards that the window vanished at once after the second X.
- The owner reported nothing from the antivirus. The run included the count at launch,
  the process outliving its window, and the lock file under `%ProgramData%\snisplit`.

Not seen in a real window after this run: the lock folder handle, a real UI panic, the
lock with two accounts, Escape, and the Keep / Remove order.

## Antivirus detection of the 11:40 build, 2026-11-15

After the 11:40 release build (HEAD 80beab3), the owner's Kaspersky scan reported
`snisplit-gui.exe` in the release folder as `VHO:Trojan-PSW.Win64.BroPass.gen`,
"detected by Kaspersky Security Network", and removed the file. `snisplit.exe` from the
same build was still in place afterwards (SHA-256
619743cd6b853bda2daefdfe34ee45e3f503041ceefacdd9ed6d0b4407c7847c); the hash of the
removed file was not taken.

What is known: the only build that was file-scanned clean is the one of 09:09 (0c8d793).
The builds of 10:32 and 11:06 were never file-scanned; the owner's 11:28 run of the 11:06
build showed no alert. `Cargo.lock` differs from the 09:09 build only in the version
number, so the dependency set is the same. Which change the detection reacts to is not
known. No build, release or acceptance run is made until the owner decides how to
proceed; the earlier sentence that two scans reported nothing describes the older builds
only.

## Looking for the commit behind the detection, 2026-11-15

With the owner's agreement, five commits were built in a separate worktree for
diagnosis only: 0c8d793, 39879e8, 54120ff, 714c683 and 80beab3, and 80beab3 was rebuilt
at the original path. Result: the detection did not reproduce.

- The antivirus command-line scan (report only) said "ok" for all six files.
- The owner was asked to scan the release folder and the five samples the same way as
  the scan that had flagged the 11:40 file; those scans (1326 objects and 5 objects)
  reported no threats. Which scan and component flagged the 11:40 file is not recorded.
- Linking the same commit twice gives files with different SHA-256 values, so none of
  the rebuilt files is byte-identical to the file that was flagged and removed.

So no commit can be named as the cause. What is established is that one particular
build of `snisplit-gui.exe` was flagged once by the cloud verdict, and that rebuilt files
of the same source, and of earlier source, were not flagged when scanned once, minutes
after they were built. The 11:40 file was flagged some time after its build, and no later
rescan of the rebuilt files has been done yet. A clean local build says nothing about the
CI-built zip, because every link produces a different file.

## Acceptance scripts m9c and m9b (reworked), 2026-11-15

Both run from an elevated PowerShell (the UAC prompt for that PowerShell is the one
thing the owner does before either), share `tools/acceptance/lib.ps1`, refuse with
nothing changed when unelevated, when a snisplit process runs, when a binary is missing,
or when `off.list` names medium, and remove stale rules pointing at 127.0.0.2-127.0.0.20
or carrying Comment 'snisplit' first. Every step ends in a `PASS` or `FAIL` line; the
last line is `RESULT: PASS` or `RESULT: FAIL`, and the exit code is 0 only when every
check passed. A `finally` block ends every process the script started (by its Process
object, never one it did not start), restores `off.list` byte for byte (or deletes it
if it did not exist), removes any rule left, and logs what is left. Screenshots are
taken with `PrintWindow` (flag 2, render full content) of the window only, without
input, into `<OutDir>\m9c-shots` or `<OutDir>\m9b-shots` (default `%TEMP%`); the output
of the `snisplit` commands goes to the `output` folder beside them.

- **`m9c.ps1`: unattended, no clicks and no questions** (log `%TEMP%\m9c.log`). It
  drives `snisplit run` and only looks at the window: run on, the base rule count, the
  store through 127.0.0.x with HTTPS 200 and HTTP 302; a second `run` exits with code 1
  and "already running" on stderr within 15 s; the window while `run` is on (screenshot,
  expected "Already open"), closed by the script, exit code 0, rules unchanged; `run`
  stopped by Ctrl+C with exit code 0, "rules left: 0" and 0 rules; `medium` appended to
  `off.list`: one rule fewer and medium.com resolving directly, then restored; `run`
  killed while on: rules stay, the window shows the leftovers (two screenshots), closing
  it leaves them, `snisplit cleanup` exits 0 and leaves 0.
- **`m9b.ps1`: the window, with the owner clicking only in the window** (log
  `%TEMP%\m9b-gui.log`). No Enter, no y/n, no address typed in the terminal: each step
  waits up to 180 s for its result (rule count, `off.list`, a new site file, the file
  gone) and logs `TIMEOUT` if it does not come. About 14 clicks and one typed address,
  all in the window: Start; Medium off; Medium on; Add a site, type it, Test, Add and turn
  on; the site's name, Remove, Remove; Stop; Start; [the script kills the window, starts
  it again, screenshots the leftovers]; Remove leftover settings; [the script starts a
  second window, screenshots it, closes it]; Start; [the script closes the window once];
  [it starts it again]; Start; [it closes it twice, 300 ms apart]. The closes record the
  seconds to exit, the exit code, the rules left and how many polls found the window
  not responding. The added site's domain is read from its new `.profile` file for the
  resolution check and is never written to the log or the console, and no screenshot is
  taken while its row exists. The one question left is whether the antivirus showed
  anything.

The clean stop of `run` from a script: the helper `send-ctrl-c.ps1` frees its own
console, attaches to the console of `run`, installs a handler that swallows the event
for itself, and calls `GenerateConsoleCtrlEvent`. A Ctrl+C sent that way is ignored when
the process inherited the "ignore Ctrl+C" flag, which a parent sets with
`SetConsoleCtrlHandler(NULL, TRUE)` or `CREATE_NEW_PROCESS_GROUP` and which every child
inherits: started from a shell that carries the flag, `ping -t` and
`snisplit run --no-routing` ignored Ctrl+C, and both stopped once the flag was cleared
in the launching process. `lib.ps1` therefore clears it in the script's own process
before it starts `run`. If `run` has not printed `stopping` 15 s after the Ctrl+C, the
script sends Ctrl+Break, which the flag does not block and which `run` also handles
(the PowerShell children that change the rules are started with `CREATE_NO_WINDOW`, so
they have consoles of their own and should not receive it; this follows from the
creation flags and is not tested); if `run` is still alive 75 s later, it is killed and `snisplit cleanup`
runs, and the log says the clean stop was not exercised. Verified unelevated with
`snisplit run --no-routing` in a temporary profile under Windows PowerShell 5.1 and
PowerShell 7: Ctrl+C, exit code 0 in about 3 s. The rule removal after the Ctrl+C is
seen only in the elevated run.

Still needs the owner: the elevated PowerShell (UAC), the clicks in m9b, the antivirus
question, and a look at the screenshots by the lead. No script covers a real UI panic,
two accounts, or logoff and shutdown with the window open.
