# snisplit - plan

A small, cross-platform tool that gets blocked domains through SNI-based DPI by
splitting the TLS ClientHello (and HTTP request) across TCP segments. It works for
every app on the machine, only touches domains listed in profiles, and is written
in Rust as an original implementation (no zapret code or binaries).

Status: approach agreed 2026-11-14; milestone order proposed, not yet confirmed
(QUIC and the tray were requested for v1 and are placed after the first release
here). Milestone 0 done 2026-11-14 (see section 4.1); CI green on Windows, Linux and macOS.

## 1. Why this is feasible: what was measured

Today this PC reaches Steam through zapret (`winws.exe` + WinDivert) running as the
`zapret-steam` service with a `fake,multidisorder` strategy. On 2026-11-14 each part
of that strategy was tested in isolation on this ISP, with the service stopped. Each
case ran twice against 4 HTTPS Steam URLs and 1 HTTP URL, with identical results.

| Case | HTTPS | HTTP |
|---|---|---|
| No bypass | fail | fail |
| Split only, in order, at byte 1 + mid-domain | works | fail |
| Split only, out of order (disorder), same positions | works | works |
| One split right after the start of the SNI extension | works | n/a (TLS-only position) |
| Fake packets only, no split | fail | fail |
| Full zapret strategy (fakes + disorder) | works | works |

Conclusions that shape the design:

- Fake packets contribute nothing on this ISP. The tool does not need forged
  packets, bad sequence numbers, MD5 options or TTL tricks.
- HTTPS needs one split inside or next to the SNI. This is the core feature.
- HTTP needs out-of-order segments. The in-order case that failed used zapret's
  `midsld` position, which (per zapret's documentation, not re-confirmed locally)
  resolves inside the `Host` value for HTTP, so an in-order mid-host split was
  most likely already tested and does not work.

Limits of the evidence:

- Tested with curl only. Its ClientHello fits one packet. Chrome and Firefox send a
  post-quantum ClientHello that spans two packets; that case is untested.
- One ISP, one day, IPv4 only. QUIC (UDP 443) was not tested.

## 2. Decisions

| Topic | Decision |
|---|---|
| Engine | Own implementation |
| Coverage | Every app, transparently (no per-app proxy setup) |
| Interception | Native packet engine per OS behind one shared core |
| Platforms | Windows first; Linux next; macOS last, after a feasibility spike |
| Language | Rust |
| Config | Profiles: each has a domain list and a strategy per protocol |
| Targeting | Listed domains only; all other traffic passes untouched |
| Audience | The author and friends on similar ISPs |
| Distribution | Public GitHub repo with per-OS release binaries |
| Name | `snisplit` (repo and binary) |

Not in scope: fake-packet strategies, an all-traffic mode, automatic domain
discovery, proxy/VPN features, DNS tampering fixes.

## 3. Architecture

A Cargo workspace. The core has no OS dependencies, so nearly all logic is testable
without a driver or admin rights.

The core is layered so a non-packet backend can reuse it: the parsers and strategies
work on the byte stream and return split offsets; the packet rewriter that turns
offsets into IP packets is a separate layer used only by packet backends. A macOS
backend built on a local transparent proxy would use the offsets directly.

```
snisplit/
  crates/
    core/        parsers, matcher, strategies, segment planner (pure, no I/O)
    backend-windows/   WinDivert
    backend-linux/     NFQUEUE + nftables
    backend-macos/     later
    cli/         the `snisplit` binary
    tray/        later
  profiles/      steam.toml and other shipped profiles
```

### core

- **TLS ClientHello parser**: returns the byte range of the SNI host name. Must
  never panic on malformed input.
- **HTTP request parser**: returns the byte range of the `Host` value.
- **Host matcher**: suffix match of a host name against profile domain lists.
- **Flow table**: small per-connection state, keyed by the 5-tuple, used only until
  the first request is handled. Needed for a ClientHello spanning two packets.
- **Segment planner**: a pure function. Input: one captured IP packet and a strategy.
  Output: the list of IP packets to send instead, in order, with sequence numbers,
  lengths, IP IDs and checksums fixed. IPv4 and IPv6.
- **Strategies (v1)**: `split` (in order) and `disorder` (reversed order), each with
  named positions: `sni-mid`, `sni-start`, `host-mid`, and a fixed byte offset.
- **Profiles**: TOML. Example:

```toml
name = "steam"
domains = ["steampowered.com", "steamcommunity.com", "steamstatic.com"]

[tls]
strategy = "split"
positions = ["sni-mid"]

[http]
strategy = "disorder"
positions = ["host-mid"]
```

### backends

One trait: receive a matching outbound packet, then either pass it unchanged or
replace it with the planner's packets.

- **Windows**: WinDivert, using the official signed release, not a re-signed copy.
  The driver filter is a milestone 2 decision with two candidates:
  - Narrow: only outbound TCP 80/443 packets whose payload starts like a TLS
    handshake or an HTTP method. Cheapest, but the second packet of a two-packet
    ClientHello is never seen, so only a first-packet SNI can be handled.
  - Wide: every outbound TCP 80/443 packet that carries payload. Handles a
    two-packet ClientHello through the flow table, at the cost of passing uploads
    through user space (downloads are unaffected; their outbound side is bare ACKs).
  Start wide for correctness; narrow only if measurements show it is safe.
- **Linux**: NFQUEUE fed by an nftables rule limited to the first packets of each
  connection; extra segments go out through a raw socket with a firewall mark so
  they are not queued again.
- **macOS**: undecided. See milestone 10.

### cli

`snisplit run`, `snisplit service install|uninstall|start|stop|status`,
`snisplit probe <domain>`, `snisplit profile list|check`.

## 4. Milestones

Each milestone ends with something that can be checked. Windows work is validated on
this PC with the `zapret-steam` service stopped for the duration of the test.

| # | Milestone | Done when |
|---|---|---|
| 0 | Scaffold: workspace, licence, CI (fmt, clippy, test), WinDivert binding spike | CI is green; a 50-line program opens WinDivert and prints captured SNIs |
| 1 | Core: parsers, matcher, segment planner | Unit tests pass on captured real ClientHellos (curl, Chrome, Firefox, Steam client); fuzzing the parsers finds no panic |
| 2 | Windows engine, HTTPS split, foreground only | The 4 HTTPS Steam URLs return 200/302 with zapret stopped, from curl and from Chrome |
| 3 | HTTP disorder, profile files, Steam profile | The HTTP Steam URL works; a non-listed site is byte-for-byte untouched in a capture |
| 4 | Windows service, logging, clean uninstall | Survives reboot; Steam client logs in and downloads a game with zapret removed |
| 5 | `probe` command | Running it on a Steam domain reproduces the no-bypass, split and disorder rows of section 1 and prints a suggested profile |
| 6 | Linux backend, systemd unit | Same checks as milestones 2-4 pass on a Linux machine |
| 7 | Releases: CI-built archives, README, antivirus note | A friend installs from the GitHub release and reaches Steam without help |
| 8 | QUIC: measure first, then decide | A written result on whether this ISP blocks QUIC by SNI; implementation only if it does |
| 9 | Tray icon: on/off, status, active profiles | Works on Windows; talks to the service, holds no engine logic |
| 10 | macOS feasibility spike, then backend | A written recommendation with a working prototype of the chosen approach |

Ordering notes:

- Milestone 4 is where this PC switches from zapret to snisplit. Removing the zapret
  service is a separate, explicit step.
- The repo is public on GitHub from the start; milestone 7 is the first release
  meant for others to install.
- QUIC is after the release on purpose: today's setup ignores UDP and Steam works.
  If the ISP does block QUIC by SNI, the cheap fix is to drop those QUIC packets so
  apps fall back to TCP; the alternative needs forged packets, which is out of scope.

### 4.1 Milestone 0 result (2026-11-14)

- Workspace: `crates/core` (pure; a first SNI extractor and IPv4/IPv6 TCP parser
  with tests that include truncated and random input), `crates/backend-windows`
  (WinDivert bindings; compiles to an empty crate off Windows), `crates/cli`
  (placeholder binary). Linux, macOS and tray crates are added when their
  milestones start.
- WinDivert binding: hand-written FFI for the few functions used, linked with
  `raw-dylib`. Building needs no WinDivert SDK, import library or download, so CI
  builds on every OS. Revisit the wrapper crates only if the API surface grows a lot.
- Spike: `cargo run -p snisplit-backend-windows --example sniff` (elevated, with
  `WinDivert.dll` on PATH). It opens WinDivert in sniff mode with `NO_INSTALL` at
  the highest priority, so it never changes traffic and only attaches to an
  already-loaded driver. Run next to the live zapret service, with curl traffic,
  it printed `store.steampowered.com`, `steamcommunity.com` and `example.com` as
  the original SNIs, not zapret's fakes.
- Not proven: the driver it attached to was the zapret bundle's third-party-signed
  copy, not the official WinDivert build. Browser two-packet ClientHellos print as
  "no SNI in first packet"; that is milestone 1-2 work.
- CI: fmt, clippy (`-D warnings`) and tests on Windows, Linux and macOS.

## 5. Open questions and risks

| Item | Why it matters | Resolved in |
|---|---|---|
| Which WinDivert binding to use | Resolved: hand-written FFI with `raw-dylib` (section 4.1). | Done |
| ClientHello spanning two packets | Browsers with post-quantum key shares do this, and the SNI can land in either packet. Untested with split-only on this ISP. | Milestones 1-2 |
| Is the bundled zapret driver the upstream WinDivert build? | It is signed by a third party, not the WinDivert author. A hash comparison against the official 2.2.2 release was blocked by the antivirus on download. Irrelevant once snisplit ships the official build, but development needs the official zip, which needs the user to allow it past the antivirus. | Milestone 2 |
| `nfq` crate: can it return a modified payload, and is it maintained? | Last release 2022. A fork and a libnetfilter_queue binding exist. | Milestone 6 |
| Linux test machine | WSL2 on this PC may work for NFQUEUE; unverified. Otherwise a VM. | Milestone 6 |
| macOS packet interception | No WinDivert/NFQUEUE equivalent. The only documented route short of Apple's signed Network Extension is a pf redirect into a local transparent proxy, which changes the backend shape. | Milestone 10 |
| Antivirus flags WinDivert | Kaspersky reported the driver as riskware on this PC. Friends will see the same. Needs a README section and the official driver build. | Milestone 7 |
| Licence | snisplit is MIT OR Apache-2.0 (licence files added; still changeable while there are no outside contributors). WinDivert's official LICENSE: "dual-licensed under your choice of the GNU Lesser General Public License (LGPL) Version 3 or the GNU General Public License (GPL) Version 2". snisplit loads it as a separate, unmodified DLL; releases must include its licence. | Milestone 7 |
| ISP changes its DPI | Split may stop working. `probe` shows what still works; fake-packet strategies would be the next step and are deliberately left out for now. | Ongoing |

## 6. References

- Current working setup, in the local zapret-win-bundle checkout: `zapret-winws/steam_service_install.cmd`
- Steam domain list to seed `profiles/steam.toml`: `zapret-winws/files/list-steam.txt`
- Reachability check to port: `zapret-winws/steam_verify.ps1`
- WinDivert: https://reqrypt.org/windivert.html
- Rust bindings: https://lib.rs/crates/windivert-sys , https://docs.rs/nfq
- zapret notes on BSD and macOS limits: https://github.com/bol-van/zapret (docs/bsd.en.md)
