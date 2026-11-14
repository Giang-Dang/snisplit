# snisplit - plan

A small, cross-platform tool that gets blocked domains through SNI-based DPI by
splitting the TLS ClientHello (and HTTP request) across TCP segments. It works for
every app on the machine, only touches domains listed in profiles, and is written
in Rust as an original implementation (no zapret code or binaries).

Status (current state and next step: docs/PROGRESS.md):

- Milestones 0 and 1 done 2026-11-14 (sections 5.1, 5.2).
- 2026-11-14: the owner chose a driverless design (section 2, decision D1). It
  replaces the WinDivert packet engine. Whether it works on this ISP is unproven: the
  new milestone 2 is a measurement spike with a written go/no-go (section 5).
  Spike done (docs/milestone-2-spike.md); the owner said go on 2026-11-14. Milestone 3
  (relay + DNS, HTTPS) done on 2026-11-14, every acceptance item passed
  (docs/milestone-3.md). Each built-in profile has its own loopback address (steam
  127.0.0.2, medium 127.0.0.3); Steam's TLS ports are 443 and 27015-27030. A built-in medium.com profile was added at the owner's request (TLS cut at
  byte 1, the measured cut; HTTP waits for the `hoSt:` rewrite in milestone 4).
- Milestone 4 (docs/milestone-4.md) measured that the ISP inspects every HTTP request
  on a keep-alive connection, so the relay respells every request head to `hoSt:`.
  Done 2026-11-14: a 1.66 GB Steam game download went through the relay with zapret
  stopped, entirely over plain HTTP on port 80.
- Milestone order after the spike is proposed, not confirmed.

## 1. What was measured

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

Conclusions:

- Fake packets contribute nothing on this ISP. No forged packets, bad sequence
  numbers, MD5 options or TTL tricks are needed for HTTPS.
- HTTPS needs one split inside or next to the SNI, in order. This is what two
  separate socket writes produce, so the driverless design (section 3) matches a
  measured working case, provided each write leaves as its own segment (untested).
- HTTP needs out-of-order segments; an in-order mid-host split failed. The driverless
  design cannot reorder segments directly and has no proven HTTP method yet.

Limits of the evidence: curl only; one ISP, one day, IPv4 only; QUIC (UDP 443) not
tested. Milestone 1 later captured Chrome and Steam client ClientHellos on this PC:
they span two packets (1384-1452 + 300-540 bytes).

## 2. Decisions

| # | Topic | Decision |
|---|---|---|
| D1 | Interception (2026-11-14, owner) | **User-mode DNS + relay, no kernel driver.** snisplit answers DNS for listed domains with a loopback address, accepts the app's connection, opens its own connection to the real server and sends the first bytes in pieces. Chosen over WinDivert because the owner does not want a third-party kernel driver (Kaspersky flags it as riskware; a driver of our own cannot be signed without a paid EV certificate tied to an organisation). |
| - | Interception (superseded by D1) | Native packet engine per OS (WinDivert on Windows, NFQUEUE on Linux). Its code is kept, parked (section 4). |
| D2 | Engine | Own implementation |
| D3 | Coverage | Every app that resolves names through the OS resolver, transparently. Apps that connect to hard-coded IPs or use their own secure-DNS provider bypass snisplit. |
| D4 | Platforms (2026-11-14, owner) | Windows first, then the same DNS + relay design on Linux and macOS. It replaces the NFQUEUE backend and the macOS packet-interception spike. Only resolver routing (NRPT on Windows, its equivalents elsewhere) is per OS. |
| D5 | Language | Rust |
| D6 | Config | Profiles: each has a domain list and a strategy per protocol |
| D7 | Targeting | Listed domains only; all other names resolve and connect as before |
| D8 | Audience | The author and friends on similar ISPs |
| D9 | Distribution | Public GitHub repo with per-OS release binaries; no driver in the release |
| D10 | Name | `snisplit` (repo and binary) |
| D11 | Crates (2026-11-14) | `tokio` with features `rt-multi-thread`, `net`, `time`, `signal`, `io-util`, `sync` (no `macros`; `sync` added for milestone 3, measured: no additional crate) for the relay and DNS loop, outside `core`. DNS codec hand-written. Any other crate still needs an explicit decision. Reasons and measured dependency counts: docs/milestone-2-spike.md, Q6 |
| D12 | Windows API (2026-11-14, owner) | `windows-sys` 0.61, a direct dependency of `crates/cli` under `cfg(windows)` only, on the same line tokio uses (measured: no new package in `Cargo.lock`). Used for the adapter DNS server lookup (the service host that also used it was removed with the service, 2026-11-14). Chosen over hand-written declarations; docs/milestone-5.md decision 1 |

Not in scope: fake-packet strategies, an all-traffic mode, automatic domain
discovery, general proxy/VPN features, terminating TLS (snisplit never holds a
certificate or sees decrypted data).

## 3. Architecture (driverless)

```
app --DNS query--> snisplit DNS (loopback) --listed?--> answer: loopback address
                                           \--other----> forward to upstream resolver
app --TCP :443/:80--> snisplit relay (loopback)
                        reads ClientHello / HTTP request, finds host, picks cuts
                        resolves the real address upstream (never through itself)
                        connects to the real server, writes piece 1, piece 2, ...
                        then relays bytes both ways unchanged
```

A diagram version of this and of the superseded design is in a private explainer
page owned by the repo owner.

### Components

- **Resolver routing.** Only listed domains go to snisplit's DNS. Candidate on
  Windows: Name Resolution Policy Table (NRPT) rules per domain suffix, so a crash
  breaks only the listed names. Fallback: system DNS pointed at snisplit, where a
  crash breaks all lookups. Rules persist across crashes and reboots, so snisplit
  removes stale rules on start and has a cleanup command.
- **DNS server.** Answers A queries for listed names with the relay's loopback
  address and a short TTL; answers AAAA and HTTPS (type 65) queries for listed names
  with an empty answer so apps neither go direct over IPv6 nor learn ECH or
  alternative endpoints; forwards everything else. Flushes the OS DNS cache on start
  and stop.
- **Relay.** Listens on the loopback address for 443 and 80. Per connection: buffer
  the first bytes until `tls::inspect_client_hello` / `http::inspect_request`
  decides, match with `DomainList`, compute cut offsets with `Strategy`, then write
  the pieces to the upstream socket with Nagle disabled and relay both directions.
  Upstream addresses come from a direct upstream DNS query, not from the OS resolver.
- **cli.** `snisplit run` (foreground, started when needed), `cleanup`, `show-profile`; later `probe`. A Windows service was built in milestone 5 and removed (docs/milestone-5.md).

### Existing work and where it goes

| Code | Status under D1 |
|---|---|
| `core::tls`, `core::http`, `core::domains`, `core::strategy`, `core::inspect`, `core::reader` | Reused as is: the relay needs exactly "find the host, match it, choose the cut". |
| Real ClientHello fixtures, mutation tests, fixture privacy guard, CI | Reused. |
| `core::segment`, `core::checksum`, `core::packet`, `core::flow`, `core::engine`, `core::pump` | Parked: tested and kept, unused by the relay. Revisit only if a packet-level backend returns (for example for HTTP disorder on Linux). |
| `crates/backend-windows` (on `main`: raw-dylib WinDivert FFI, `sniff` and `capture` examples) | Parked. The `capture` example is still the tool for recording new fixtures, but it needs a WinDivert driver to run. |
| Local branch `m2/windows` (commits 2c590a5, eb7aec0; never pushed) | Parked: runtime-loaded WinDivert, `Diverter` implementing `pump::PacketIo`, Ctrl+C hook. Two findings to keep if it is ever merged: the receive buffer must be 65575 bytes (`WINDIVERT_MTU_MAX`), not 65535, and `shutdown()` returns a Result the caller must handle. |
| `crates/cli` on `main` | Rewired to the relay in milestone 3: `run`, `cleanup`, built-in steam and medium profiles, Windows NRPT routing. |
| docs/milestone-2.md | Superseded (WinDivert plan); kept for the record. |

## 4. Milestones

Each milestone ends with something that can be checked. Tests that need the ISP run
on this PC; until 2026-11-14 that meant stopping the `zapret-steam` service after asking. zapret has since been removed.

| # | Milestone | Done when |
|---|---|---|
| 0 | Scaffold, licence, CI, WinDivert spike | Done (5.1) |
| 1 | Core: parsers, matcher, segment planner, engine | Done (5.2) |
| 2 | **Feasibility spike (go/no-go)** | A written result in `docs/` for every question in section 4.1, with the commands run and their output; and a go or no-go recommendation |
| 3 | Relay + DNS, HTTPS only, foreground (if go) | With zapret stopped, `snisplit run` makes the 4 HTTPS Steam URLs return 200/302 from curl and from Chrome, through normal name resolution |
| 4 | HTTP method, profile files, Steam profile | The HTTP Steam URL works (method chosen in the spike); a non-listed name resolves exactly as without snisplit |
| 5 | Foreground use on this PC, crash-safe DNS cleanup, zapret removed (rescoped 2026-11-14: the Windows service was built, then dropped because Kaspersky terminates its install) | Start-up under 1.5 s; Ctrl+C, a closed window and a restart leave no rule, or the next `run` removes it; a killed process breaks only listed names; Steam logs in and downloads a game with zapret removed |
| 6 | `probe` command | Reproduces the no-bypass and split rows of section 1 for a domain and prints a suggested profile |
| 7 | Releases: CI-built archives, README | A friend installs from the GitHub release and reaches Steam without help |
| 8 | QUIC: measure first, then decide | A written result on whether this ISP blocks QUIC by SNI. With DNS pointing listed names at loopback, QUIC to them fails and apps fall back to TCP, which may already be enough |
| 9 | UI (redefined 2026-11-14): start and stop the relay, add and remove sites | The owner adds a site, sees whether it works, and starts and stops snisplit without a terminal. Open owner decisions: the UI technology (new crates or `windows-sys` features), one elevated app or a split with a privilege boundary, Linux and macOS under D4. Needs a per-user profile folder that `run` loads, and `probe` (milestone 6) to say whether a site will work |
| 10 | Linux and macOS (D4) | The relay and DNS run on both; resolver routing per OS; the milestone 3-5 checks pass on a Linux machine and a Mac, as foreground runs |

Ordering notes:

- If the spike finds no user-mode HTTP method that passes this ISP, the choice goes
  back to the owner before milestone 4: HTTPS-only, or a packet-level piece for HTTP.
- Milestone 5's "downloads a game" depends on HTTP if Steam downloads use port 80 on
  this PC; the spike checks which ports a real download uses.
- Milestone 5 is where this PC switched from zapret to snisplit: the owner removed
  the `zapret-steam` and `windivert` services on 2026-11-14 and starts `snisplit run`
  when needed.
- The repo is public on GitHub from the start; milestone 7 is the first release meant
  for others to install.

### 4.1 Questions the spike must answer

Each answer needs a command and its output, not a recollection.

1. **Two writes, two segments, past the ISP.** A minimal relay on loopback, used by
   `curl --connect-to` (no DNS change, no admin rights). Control: one write must fail
   with zapret stopped. Test: two writes at the measured cut points must succeed.
   Same through Chrome, if `--host-resolver-rules` gives an equivalent test.
2. **HTTP.** Candidate user-mode methods, tried against the HTTP Steam URL: header
   rewrites the relay can make because it sees plaintext, a low-TTL first piece, and
   prior art in zapret's `tpws` documentation and ByeDPI. TLS record fragmentation
   is also checked as a fallback for HTTPS.
3. **Does a Steam game download on this PC use port 80?**
4. **NRPT.** Can a rule point a suffix at a loopback DNS server on port 53? Does Chrome
   honour it? How does a stale rule behave after a crash?
5. **Ports.** What already listens on loopback 53, 80 and 443 (the installed WARP
   client may hold loopback DNS addresses)? Does a dedicated address such as
   127.0.0.2 avoid conflicts?
   - Answer (measured 2026-11-14 on the owner's PC, without admin rights):
     - UDP 0.0.0.0:53 is held by the Windows service SharedAccess (Internet
       Connection Sharing), so a resolver cannot bind the wildcard address on port 53.
     - Binding a specific loopback address still works: UDP 127.0.0.2:53, UDP
       127.0.0.1:53, TCP 127.0.0.2:443 and TCP 127.0.0.2:80 all bound successfully
       (tested with .NET UdpClient and TcpListener).
     - Nothing listens on TCP 80 or 443 on any address.
     - There are no NRPT rules configured.
     - A dedicated address such as 127.0.0.2 avoids the conflicts on this PC. A
       socket on 127.0.0.2:53 receives the queries sent there; ICS does not take
       them (docs/milestone-2-spike.md).

6. **Crates.** A DNS codec and a relay raise the no-new-crates rule: threads with
   std only, or an async runtime. Decided: D11.

Results for questions 1-6 and the go decision: docs/milestone-2-spike.md.

## 5. Results

### 5.1 Milestone 0 result (2026-11-14)

- Workspace with `crates/core`, `crates/backend-windows`, `crates/cli`; MIT OR
  Apache-2.0; CI runs fmt, clippy (`-D warnings`) and tests on Windows, Linux, macOS.
- WinDivert binding: hand-written FFI linked with `raw-dylib`.
- Spike: the `sniff` example, in sniff mode with `NO_INSTALL`, printed the original
  SNIs of Steam connections next to the live zapret service.
- The driver it attached to was the zapret bundle's third-party-signed copy.

### 5.2 Milestone 1 result (2026-11-14)

Plan, decisions and contract: docs/milestone-1.md. `crates/core` holds:

- `tls::inspect_client_hello` and `http::inspect_request`: return the host's byte
  range, "need more data" or "no host"; never panic.
- `domains::DomainList`: suffix match at label boundaries, returns where the matched
  listed domain sits in the host.
- `strategy`: split positions `HostStart`, `DomainMid` (middle of the first label of
  the matched domain, zapret's `midsld`) and fixed offsets.
- `segment::split`, `flow`, `engine`: the packet-level planner (parked under D1).

Evidence:

- 134 tests with expected values independent of the code under test: published
  vectors (RFC 1071, a published IPv4 header, Wireshark sample `http.cap` frame 4 with
  its checksums checked against the file), hand-derived offsets, round-trips, and 12
  real Steam ClientHellos captured on this PC (curl, Chrome, Steam client).
- A test audit planted deliberate bugs; 18 tests that missed them were rewritten or
  removed, each rewrite shown to fail on its planted bug.
- Mutation tests over the real captures find no panic.
- A test fails if any fixture carries a session-resumption ticket.

Not covered: Firefox (not installed); a real capture with the host straddling two
packets (synthetic tests only).

### 5.3 After milestone 1 (2026-11-14, on `main`, not pushed)

- docs/milestone-2.md: the WinDivert plan, now superseded.
- `core::pump`: a packet loop that re-sends every diverted packet, 11 tests (parked).
- `crates/cli`: `snisplit run [--verbose]`, `--version`, `help`, the built-in Steam
  profile, exit codes; the platform call returns "unsupported" until milestone 3.

## 6. Open questions and risks

| Item | Why it matters | Resolved in |
|---|---|---|
| Two writes may not leave as two segments | Measured on HTTPS: each write left as its own send (pktmon). Port 80 writes were merged by the antivirus on this PC. Other PCs and security software may differ. | Milestone 2 (HTTPS confirmed); milestone 7 notes |
| User-mode HTTP method | `hoSt:` passes this ISP, and must be applied to every request on a keep-alive connection (measured in milestone 4). Splitting HTTP is untested here. | Milestone 4 |
| Crash leaves DNS broken | System-wide DNS on loopback breaks every lookup if snisplit dies. NRPT per suffix would limit it to listed names. | Milestones 2 and 5 |
| Browsers with their own secure DNS | They bypass the OS resolver and so snisplit. Measured: Steam's embedded browser auto-upgrades to DoH, and drops it while any NRPT rule exists. Explicit "secure DNS" settings are not covered; needs a README note. | Milestones 3 and 7 |
| NRPT fallback is machine-wide | While snisplit's rules exist, every Chromium-based app loses automatic DoH for all names, not only listed ones. Users should be told. | Milestone 7 (README) |
| snisplit started while Steam or Chrome is already open | Chrome open before the start went through the relay (milestone 3 acceptance). A Steam client left open worked without a restart in the owner's foreground trial (one run, 2026-11-14, docs/milestone-5.md). | Closed for this PC |
| DoH bypass on Linux and macOS | No NRPT there; Chromium's auto-upgrade needs another answer. | Milestone 10 |
| Port conflicts on loopback 53/80/443 | On this PC ICS holds 0.0.0.0:53; 127.0.0.2 and 127.0.0.3 are free. A bind failure stops `run` before any rule exists (milestone 3). | Closed for this PC |
| Listed names that exist only over IPv6 | Milestone 4: a listed name with no A record is forwarded unchanged, so it resolves as without snisplit (checked in the elevated run). | Milestone 4 |
| Start-up time | 13-21 s in milestone 3, 3.5 s in milestone 4, 0.7-1.3 s routed in milestone 5's first runs, then 703, 1789 and 2000 ms with the foreground build (cause of the spread unknown; target 1.5 s). | Milestone 5, owner decides |
| IPv6 and HTTPS records | Done in milestone 3: listed names get empty AAAA and type-65 answers. | Closed |
| Relay throughput | Every Steam byte, including game downloads, is copied through snisplit. | Milestone 5 |
| Antivirus reaction | Kaspersky on the owner's PC (2026-11-14): 19 routed foreground runs passed; `snisplit service install` was terminated as `PDM:Trojan.Win32.Generic` twice, in the second it created the service, and the file was deleted; one stream C build was deleted on disk as `VHO:Backdoor.Win64.Agent.gen`. Milestone 5 is blocked on this PC (docs/milestone-5.md). | Owner decision; milestone 7 |
| New crates (DNS codec, async runtime) | Decided: D11 (tokio, minimal features; DNS codec hand-written). | Closed |
| Resolver routing on Linux and macOS | Per-domain DNS routing exists (systemd-resolved domains, macOS `/etc/resolver/<domain>` files) but is untested for this use. | Milestone 10 |
| Test machines for Linux and macOS | None identified yet. | Milestone 10 |
| ISP changes its DPI | Split may stop working. `probe` shows what still works. | Ongoing |
| Moot under D1 | WinDivert binding choice, bundled driver provenance, WinDivert licence, `nfq` crate, Linux NFQUEUE test machine, macOS packet interception. Kept here so they are not reopened by accident. | Closed |

## 7. References

- Former setup, removed 2026-11-14 (rollback: run as administrator after `snisplit cleanup`), in the local zapret-win-bundle checkout: `zapret-winws/steam_service_install.cmd`
- Steam domain list to seed `profiles/steam.toml`: `zapret-winws/files/list-steam.txt`
- zapret, including the user-mode `tpws` proxy documentation: https://github.com/bol-van/zapret
- Windows NRPT: `Add-DnsClientNrptRule` in the DnsClient PowerShell module
- WinDivert (parked design): https://reqrypt.org/windivert.html
