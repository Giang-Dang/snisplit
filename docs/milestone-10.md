# Milestone 10 (Linux part) - plan and record

Started 2026-11-20 at the owner's request: make snisplit work on the popular Linux
distributions and on SteamOS, checked in Docker. macOS, the other half of milestone 10
(PLAN.md D4), is not part of this request and stays deferred.

## Scope

Phase 1: the command line (`snisplit run`, `cleanup`, `probe`, `show-profile`) on Linux
x86_64, one static binary. Phase 2 (the window on Linux) has its own design and waits for
the owner (decision 7).

## Spike results (2026-11-20, Docker Desktop, kernel 6.6.87.2-microsoft-standard-WSL2)

Routing mechanism: one `dummy` link per profile, given an address, with the profile's
loopback DNS server and its suffixes as routing domains in systemd-resolved:

```
ip link add snisplit0 type dummy
ip addr add 169.254.53.1/32 dev snisplit0
ip link set snisplit0 up
resolvectl dns snisplit0 127.0.0.2
resolvectl domain snisplit0 ~steampowered.com
```

Measured:

| Check | Result |
|---|---|
| Dummy link in the container kernel | Created |
| Scope with a link-scope address (169.254.53.53/32 scope link) | `Current Scopes: none`, listed names not routed |
| Scope with a global-scope address (169.254.53.53/32) | `Current Scopes: DNS`; `store.steampowered.com` and `cdn.steampowered.com` answered 127.0.0.2 by a test server on 127.0.0.2:53; `example.org` resolved as before |
| Loopback server address on a link | Accepted (systemd 255 on Ubuntu 24.04, systemd of SteamOS 3.8.14) |
| Removal | `ip link del snisplit0` removes the link and every resolved setting on it; listed names resolve as before at once (after `resolvectl flush-caches`) |
| Killed process | Leaves the link and its settings; nothing else. `cleanup` deletes links named `snisplit*` |
| NetworkManager 1.52.1 (SteamOS) | Shows the link as `connected (externally)`; the link's DNS and domains survived `nmcli general reload`, a NetworkManager restart and `nmcli con up` of another connection |
| A resolv.conf-only client (raw UDP query to the first `nameserver` in /etc/resolv.conf) on SteamOS | Listed name answered 127.0.0.2 through 127.0.0.53; other names as before |

Docker traps found: Docker bind-mounts its own `/etc/resolv.conf` (nameserver
192.168.65.7), which bypasses resolved; every test unmounts it first. Files copied with
`docker cp` under a tmpfs `/tmp` are hidden; the harness copies elsewhere.

### SteamOS 3.8.14 (Valve's recovery image, `steamdeck-oobe-repair-20260707.10-3.8.14`)

Read from the image's root partition, then booted with its own systemd in a container
(the Deck's partset mounts in `/etc/fstab` and `etc.mount` masked; `machine-id` left blank
so the first-boot presets apply as on a Deck):

- systemd-resolved: enabled by preset (`90-systemd.preset`), started by NetworkManager;
  `resolv.conf mode: foreign`.
- NetworkManager: `dns=systemd-resolved`, `rc-manager=symlink`; `/etc/resolv.conf` is a
  regular file into which NetworkManager writes `nameserver 127.0.0.53`.
- `/etc/nsswitch.conf`: `hosts: mymachines mdns_minimal [NOTFOUND=return] resolve
  [!UNAVAIL=return] files myhostname dns`.
- Kernel 6.16.12-valve24.4 ships `dummy.ko`; `ip`, `resolvectl`, `sudo`, `pkexec` exist.
- `deck` has no password (`deck::` in /etc/shadow): `sudo` needs `passwd` first.
- Root is read-only on a Deck; `/home` is persistent. The binary lives under the user's
  home, nothing is installed into `/usr`.

## Distribution tiers

Sources: the spike above, Debian release notes for bookworm (systemd-resolved is a separate
package, not the default), archinstall's network handler (the NetworkManager choice does
not enable resolved), community reports for the rest. A distribution moves to "verified"
only when the container matrix below passes on it.

| Tier | Distributions |
|---|---|
| Works as installed | Ubuntu 24.04 and later, Fedora Workstation, SteamOS 3.x |
| Works after enabling systemd-resolved (snisplit prints the steps) | Debian 12 and 13, Arch Linux (NetworkManager install), and the distributions in the matrix where the steps were run |
| Unverified | openSUSE (netconfig; a community report says the switch to resolved failed), Linux Mint, Pop!_OS, Manjaro, EndeavourOS, Bazzite |

## Decisions

1. Resolver routing on Linux is systemd-resolved per-link routing domains on one dummy link
   per profile (`snisplit0`, `snisplit1`, ...), each with address `169.254.53.<n+1>/32`,
   because a global-scope address is what activates the link's DNS scope (spike) and a
   link-local /32 on a dummy link reaches nothing. The link gets `default-route no`,
   `llmnr no`, `mdns no`, `dnsovertls no`, `dnssec no` so global settings cannot apply to
   the loopback server.
1b. Each link also gets the route `local <server>/32 dev snisplitN table local`.
   Measured on Ubuntu 22.04 (systemd 249): resolved sends a link's queries from the link
   (source 169.254.53.1, socket bound to `snisplit0`), so a query to 127.0.0.2 left
   through the dummy link and was lost ("All attempts to contact name servers or
   networks failed"). With the local route, a lookup bound to the link finds 127.0.0.2
   as local and the kernel delivers it over loopback; TCP to 127.0.0.2 from other
   programs still works, and the route goes away with the link. systemd 252 and later
   did not need it; the matrix passes with it on every version.
2. The tools `ip` and `resolvectl` are run as child processes, as PowerShell is on Windows;
   no new crate (D11). They are looked up only in `/usr/sbin`, `/usr/bin`, `/sbin`, `/bin`,
   never through `PATH`, because the process runs as root.
3. `run` refuses to start unless systemd-resolved answers and `/etc/resolv.conf` sends
   queries to its stub (a `nameserver 127.0.0.53` line, or a link to
   `/run/systemd/resolve/stub-resolv.conf`). NSS alone is not enough: clients that read
   resolv.conf directly would bypass snisplit.
4. Upstream DNS servers for `probe` and the relay come from
   `/run/systemd/resolve/resolv.conf` (resolved's real servers), else `/etc/resolv.conf`,
   loopback and IPv6 entries dropped, so the relay never asks its own stub.
5. The one-instance lock on Linux is `flock` through `File::try_lock` in `/run/snisplit`;
   the `snisplit` crate's `rust-version` goes from 1.85 to 1.89 for it.
6. Under `sudo` or `pkexec` (`SUDO_UID` or `PKEXEC_UID` set), the sites folder is the
   invoking user's (`<home from /etc/passwd>/.config/snisplit/sites`), not root's.
7. The window on Linux is phase 2: enabling eframe's Linux back-ends (D15) and how a window
   gets root (pkexec of the whole window, or a small elevated helper) need the owner.
8. Release form: one static x86_64 musl binary, `snisplit-<version>-linux-x86_64.tar.gz`,
   run from any folder (on SteamOS: under `/home/deck`).
9. Platform code is gated on `target_os = "linux"`; other non-Windows systems keep the
   current "unsupported" answers.

Owner answers, 2026-11-20:

10. Systems without systemd-resolved: snisplit refuses to start and prints the steps to
    enable it; no resolv.conf backend.
11. The window on Linux is phase 2, after the command line; its design goes to the owner.
12. SteamOS: foreground `sudo snisplit run` only; Gaming Mode is listed as unverified for
    the owner to try on a Deck. No systemd unit.
13. Git: local commits on `m10-linux`, no push.

The owner then delegated every remaining decision to the lead (2026-11-20). Decided
during implementation:

14. Stale-link removal and `cleanup` touch only dummy links whose name starts with
    `snisplit`; a non-dummy link of that name is left alone and makes `run` fail at
    the add step.
15. `cleanup` runs `resolvectl flush-caches` only when it deleted a link, so it works on
    a machine without resolved.
16. Child processes run with an empty environment plus `LC_ALL=C`, stdin closed; any
    stderr output counts as a failure.
17. A `SUDO_UID` or `PKEXEC_UID` that is unparsable or missing from /etc/passwd falls
    back to the current user's folder (root's, under sudo).
18. The release workflow gets a `linux` job after the Windows one: tests, the musl build,
    `snisplit-<version>-linux-x86_64.tar.gz` with its `.sha256`, uploaded to the same
    draft release.

## Phase 1 tasks

| Task | Done when |
|---|---|
| T1 `routing` on Linux (decisions 1-3) | Unit tests for every command line built and every output parsed; the Windows code and tests unchanged |
| T2 `system_dns` on Linux (decision 4) | Parser unit tests on hand-written resolv.conf texts |
| T3 `instance` lock on Linux (decision 5) | A second claim is `Elsewhere` while the first is held, `Ours` after it drops |
| T4 sites folder under sudo (decision 6) | Unit tests through the existing lookup function |
| T5 Container matrix `tools/acceptance/linux` | Per distribution: build-once binary, preconditions, listed name to the relay's address through NSS and through resolv.conf, an HTTPS fetch of a listed name through the relay, unlisted names untouched, Ctrl+C and SIGTERM leave no link, `kill -9` leaves links that `cleanup` and the next `run` remove |
| T6 README Linux section, PROGRESS.md, this record | The tiers table matches what the matrix ran |

## Result (2026-11-20)

Phase 1 is built. One static binary (`x86_64-unknown-linux-musl`, 2.1 MB, sha256
`68a47511b365a4631242f7b6e9552522bfdedcb33c9d2b1800d4909d4284559a` for the build checked)
ran unchanged in every container of `tools/acceptance/linux/matrix.sh`:

| Distribution | systemd | Setup before the checks | Checks |
|---|---|---|---|
| Ubuntu 24.04.5 | 255 | stub symlink, as on a desktop install | 41 pass, 0 fail |
| Ubuntu 22.04.5 | 249 | stub symlink, as on a desktop install | 41 pass, 0 fail |
| Debian 12 | 252 | refusal checked, then `apt-get install systemd-resolved` | 44 pass, 0 fail |
| Debian 13 | 257 | refusal checked, then `apt-get install systemd-resolved` | 44 pass, 0 fail |
| Fedora 45 | 262 | stub symlink, as on a desktop install | 41 pass, 0 fail |
| Arch Linux | 262 | refusal checked, then enable resolved and the stub symlink | 44 pass, 0 fail |
| openSUSE Tumbleweed | 262 | refusal checked, resolved package in the image, then enable and the stub symlink | 45 pass, 0 fail |
| SteamOS 3.8.14 (Valve image) | 257 | booted as shipped; NetworkManager connection standing in for Wi-Fi | 41 pass, 0 fail |

What each run checks: preconditions (resolv.conf reaches the stub, names resolve to a real
address); a non-root `run` is refused with a sudo hint and adds nothing; `run` is ready
with one link per profile; listed names, a subdomain and the second profile resolve to the
relay addresses through NSS (`getent`) and through a resolv.conf-only client; an unlisted
name resolves to a real address; `curl` over HTTPS (200) and HTTP (302) reaches
`store.steampowered.com` through 127.0.0.2 and the relay logs it; a second `run` is refused
and leaves the first's links; Ctrl+C exits 0 with no link left; `kill -9` leaves the links,
`cleanup` removes them; a `run` after `kill -9` removes the stale links and starts; SIGTERM
leaves no link; with `SUDO_UID` set, a profile in that user's `~/.config/snisplit/sites`
is relayed.

Found by the matrix and fixed: the systemd 249 query binding (decision 1b). Harness faults
found and fixed on the way: missing D-Bus in minimal images, SteamOS checks starting
before NetworkManager, and address checks that passed on an empty answer (they now need a
real address).

Gates: `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace` on Linux (680
passed, baseline 640) and Windows (699 passed, baseline 666 measured at d3a01b3), `cargo
check --target x86_64-apple-darwin`, actionlint on both workflows.

### Not verified

- The ISP: containers share Docker Desktop's kernel and network path, so whether the split
  ClientHello gets through the provider's DPI on Linux is not shown; only that the relay
  carries the traffic.
- Real kernels and desktops: every container ran on kernel 6.6.87.2-microsoft-standard-WSL2.
  NetworkManager was present only in the SteamOS run; netplan, wicked, netconfig and
  connman were not.
- `sudo` itself (the check sets `SUDO_UID` directly), pkexec, and a Steam Deck: Gaming Mode,
  the Steam client, Flatpak and Steam Runtime containers, browsers with their own secure DNS.
- Linux Mint, Pop!_OS, Manjaro, EndeavourOS, Bazzite.

## Blast radius

| Symbol changed | Callers | What could break |
|---|---|---|
| `routing::{begin_start, PendingStart::{add, abandon}, remove_all, count_all}` | `session.rs` `System` host (used by `run`, `cleanup`, the GUI driver) | Windows behaviour if the dispatch changes; the Windows tests in `routing/tests.rs` must pass unchanged |
| `system_dns::servers` | `session.rs`, `probe.rs`, `gui/src/driver.rs` | Windows adapter lookup; Linux upstream choice |
| `instance::{folder, claim}` | `main.rs`, `session.rs`, `gui/src/driver.rs`, `gui/src/window.rs` | Windows lock semantics; Linux gets a real lock |
| `sites::folder` | `main.rs`, GUI | Which folder `run` reads under sudo |

`crates/gui` is not edited in phase 1.

## Risks

| Risk | Handling |
|---|---|
| NetworkManager or another manager reconfigures the dummy links | Measured on SteamOS's NetworkManager: no effect. Other managers (netplan, wicked, connman) unmeasured |
| Docker Desktop shares one kernel; DPI cannot be seen from a container | Container results are claimed as "routing, relay, cleanup", never as "gets through the ISP" |
| Clients with their own DNS (browser secure DNS, Steam Runtime containers) bypass resolved | README note; not measurable in containers |
| SteamOS Gaming Mode: a foreground `sudo snisplit run` from Desktop Mode | Owner question; not measurable in containers |
| A port 53 listener on 0.0.0.0 (dnsmasq, libvirt) blocks 127.0.0.2:53 | The existing bind error stops `run` before any link exists |

## Dropped

- A second backend that points `/etc/resolv.conf` at snisplit for systems without resolved:
  every lookup breaks if snisplit dies (PLAN.md "Crash leaves DNS broken"). Owner decision.
- NetworkManager's dnsmasq plugin: not installed by default anywhere in the tiers.
- `/etc/hosts`: cannot express suffixes.
- macOS: not requested.
