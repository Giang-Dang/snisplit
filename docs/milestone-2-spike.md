# Milestone 2 spike: results and go/no-go

Answers to PLAN.md section 4.1, measured on the owner's PC on 2026-11-14.

## Recommendation: go, conditional on the steamwebhelper DNS result

Blocking: Steam's embedded browser resolves names over DNS over HTTPS, bypassing the
system resolver (see "Steam client DNS check"). Go holds only if a rerun with an
NRPT rule present shows it falling back to the system resolver, or another routing
(for example the hosts file, also unverified) reaches it.

The relay part works on this ISP for HTTPS and HTTP, for the Steam store,
community, API, login and content hosts, with zapret stopped:

- HTTPS: cut the ClientHello at byte 1 (two writes). Single write is reset every time;
  cut at 1 passes every time, for small and large (1529-byte) client hellos.
- HTTP: respell the header name `Host:` as `hoSt:`, tested on the store and one
  content host. HTTP splitting is untested on this PC: the antivirus merged the
  pieces before the wire.
- Not tested, not needed: a low-TTL first piece and TLS record fragmentation
  (PLAN.md 4.1 Q2).
- DNS: an NRPT rule sends only the listed suffix to a resolver on 127.0.0.2:53; other
  names are unaffected, also with a stale rule after the resolver is gone.

Open before milestone 2 is closed:

- steamwebhelper (Steam's embedded Chromium, which draws the Store and Community
  pages) does not use the system resolver. See "Steam client DNS check" below. Next
  test: the same check with a dummy NRPT rule present, since Chromium is believed to
  fall back to the system resolver when NRPT rules exist (unverified).

## Steam client DNS check (2026-11-14 16:35-16:38)

`tools/spike/steam-dns-check.ps1`, elevated: DNS client log on, wait for a new
`steam.exe`, observe 120 s while the owner opened the Store, then read the 3006
events by process name and turn the log off again.

- `steam.exe` resolves through the system resolver: 17 lookups, including
  `api.steampowered.com`, `steamcommunity.com`, the `cmp*-hkg1/sgp1.steamserver.net`
  connection managers and `steamconnecttest.com`.
- `steamwebhelper.exe` made 13 system lookups, all `wpad` or `disabled.invalid`, and
  none of a Steam name. At the same time it held about 30 established connections to
  Akamai and Fastly addresses on 443 (`Get-NetTCPConnection`), and one to
  162.159.61.3:443, which is `chrome.cloudflare-dns.com`. It resolves names with DNS
  over HTTPS: Chromium's automatic upgrade, because this PC's system DNS is 1.1.1.1
  (Cloudflare). Its command line sets no DNS option.
- So the bypass depends on the DNS provider: a PC whose DNS is the ISP's own resolver
  would not get the automatic upgrade. Friends' setups vary.
- Consequence: with only NRPT routing, the client's Store pages would keep going
  direct and stay blocked, unless the NRPT fallback holds for steamwebhelper.
- Pass rule for the rerun with the dummy rule (`-WithDummyNrptRule`): steamwebhelper
  makes system lookups of Steam web names while the rule exists.

Carried into milestone 3 as an acceptance check: the port of Steam's plain HTTP
downloads (Q3).

## Elevated run with zapret stopped (2026-11-14 16:23-16:25)

`tools/spike/elevated-window.ps1`, run once elevated with the owner's approval:
stopped `zapret-steam` and waited until no `winws` process was left, ran the probes
with a pktmon capture (ports 80 and 443, NICs only), restarted the service in a
`finally` block (log: `zapret restored: Running, winws=True`), then ran the NRPT test
and read the DNS client log. zapret was stopped for about one minute.

### Steam HTTPS, `tls-split-probe.ps1 -Rounds 2`

| Host | single | cut at 1 | cut mid-host | cut at 1 and mid-host |
|---|---|---|---|---|
| store.steampowered.com | FAIL 2/2 | PASS 2/2 (200) | FAIL 2/2 | PASS 2/2 |
| steamcommunity.com | FAIL 2/2 | PASS 2/2 (200) | FAIL 2/2 | PASS 2/2 |
| api.steampowered.com | FAIL 2/2 | PASS 2/2 (404 for `/`) | FAIL 2/2 | PASS 2/2 |
| login.steampowered.com | FAIL 2/2 | PASS 2/2 (302) | FAIL 2/2 | PASS 2/2 |
| cache5-hkg1.steamcontent.com | FAIL 2/2 | PASS 2/2 (200) | FAIL 2/2 | PASS 2/2 |
| medium.com | FAIL 1/1 | PASS 1/1 | FAIL 1/1 | PASS 1/1 |

Large hello through the loopback relay to store.steampowered.com (`big-hello.py`,
1529-byte record): single write 0 bytes from the server 2/2; cut at 1 TLS 1.3 and
`HTTP/1.1 200 OK` 2/2.

### Steam HTTP, `http-variants.ps1 -Rounds 1`

| Variant | store.steampowered.com | cache5-hkg1.steamcontent.com |
|---|---|---|
| control, hostcase, hostdot, hostnospace, domcase, methodspace, unixeol, hostpad 1-8 KiB | FAIL | FAIL |
| methodeol, all splits | untested: changed before the wire (see below) | same |
| `hoSt:` | **PASS, 302** | **PASS, 200** |
| hosttab | ISP passes, server 400 | ISP passes, server 400 |

### Wire evidence (pktmon, `tools/spike/pcapng-segments.py`)

- HTTPS: every write left as its own TCP send. Cut-at-1 connections start with
  `1+448`, `1+141+626`, `1+454`; single-write connections with one 449, 742 or
  768-byte segment. The large hello through the relay: single write one 1529-byte
  send, cut at 1 `1+1528`. The 1529 and 1528-byte sends are large-send offload
  packets (IP total length 0 in the capture), which the NIC cuts to the path MTU.
- Every failed HTTPS connection received a RST from the server side; no passing
  connection did. This holds for the small and the large hello.
- HTTP: the split variants left as one 100-byte segment, so the pieces were merged
  before the wire, even with Nagle off and 30 ms between writes. methodeol left
  without its leading CRLF (100 bytes, not the 102 written). Blocked HTTP
  connections also send a RST from this PC, matching the WSAECONNABORTED the probe
  sees. The antivirus (`avp`) filters port 80 on this PC and is the likely cause; it
  was not worked around, so those rows stay untested here. The `hoSt:` request
  reached the wire byte for byte as written.

### NRPT

| Step | Result |
|---|---|
| `Add-DnsClientNrptRule -Namespace .nrpt-probe.example -NameServers 127.0.0.2` | 1 rule |
| `Resolve-DnsName a.nrpt-probe.example` | 127.0.0.2 (answered by the test listener) |
| `Resolve-DnsName example.org` | normal public addresses |
| headless Chrome, no resolver flags, `http://a.nrpt-probe.example/` | its query reached the listener at 16:24:20: Chrome uses the system resolver and honours NRPT |
| stale rule, listener stopped: `b.nrpt-probe.example` | no answer, after 2 ms |
| stale rule: `example.org` | normal public addresses |
| rule removed in `finally` | 0 rules left |

A crash with the rule in place breaks only the listed names, and fails fast.

### DNS client log

`Microsoft-Windows-DNS-Client/Operational` events 3006 during the run, Steam and
medium names only: the user's Chrome looked up `steamcommunity.com` and
`cache4-hkg1.steamcontent.com` through the system resolver at 16:24:16, as the NRPT
test rule was added, and at 16:25:17, around its removal. Both moments are NRPT
changes, so this does not show whether Chrome uses the system resolver without a
rule. The other lookups came from the short-lived probe processes.

## Test domain: medium.com

The owner reported `https://medium.com/` blocked. It is blocked the same way as Steam,
and zapret's hostlist does not include it, so it can be tested with `zapret-steam`
left running: both zapret profiles are limited to `list-steam.txt`, which has no
`medium` entry (checked with `sc qc zapret-steam` and `Select-String`).

The block is on the name, not the address (curl, zapret running):

| Request | Result |
|---|---|
| `https://medium.com/` | TCP connects, reset after the ClientHello (curl 35) |
| `http://medium.com/` | reset after the request (curl 56) |
| medium IP 162.159.153.4, SNI `www.cloudflare.com` | 200 |
| Cloudflare IP 104.16.123.96, SNI `medium.com` | reset |
| medium IP, HTTP `Host: example.org` | 200 |
| medium IP, HTTP `Host: medium.com` or `MEDIUM.com` | reset |
| `www.medium.com`, `help.medium.com` | reset |

## Q1: two writes past the ISP (HTTPS)

`tools/spike/tls-split-probe.ps1 -HostName medium.com -Rounds 3` (no admin). It runs a
real TLS handshake with .NET `SslStream` over a stream that cuts the first write
(the ClientHello) into pieces, Nagle off, 30 ms between pieces. Pass means the
handshake completes and an HTTP status line comes back.

| Mode | Pieces (bytes) | Result, 3 rounds |
|---|---|---|
| single write | 437 or 692 | FAIL 3/3, connection reset |
| cut at 1 | 1+691 | PASS 3/3 (HTTP 403 from Cloudflare to a bare HEAD: the handshake completed) |
| cut mid-host only | 136+556 | FAIL 3/3 |
| cut at 1 and mid-host | 1+135+556 | PASS 3/3 |

Dry run against `www.cloudflare.com` (not blocked): all four modes pass.

Findings:

- Separate user-mode writes do get HTTPS past this ISP. The cut that matters is at
  byte 1. A cut only inside the host name does not help. Hypothesis, not tested: the
  ISP inspects only a segment that starts with a full TLS record header.
- Not yet proven at wire level: that each write left as its own TCP segment. Single
  write fails and cut-at-1 passes on the same path, so the wire did differ; a pktmon
  capture in the elevated run will record the segment sizes.
- Steam hosts: same result, see the elevated run above.

### Through a loopback relay (the relay design, no DNS change, no admin)

`tools/spike/loopback-relay.ps1 -Upstream medium.com -Mode single|split1` listens on
127.0.0.2:443, reads the client's first TLS record, connects to the real server, sends
the record as one write or cut at byte 1, then copies both ways.

Clients:

- Headless Chrome with its own profile and
  `--host-resolver-rules="MAP medium.com 127.0.0.2"`. It sent a 517-byte ClientHello.
- `tools/spike/big-hello.py` (Python with OpenSSL 3.5.6, post-quantum key share). It
  sent a 1517-byte ClientHello record, which is larger than one segment, the same
  shape as the Chrome and Steam client hellos captured on this PC.

| Client | single write | cut at 1 |
|---|---|---|
| headless Chrome, 4 connections | 0 bytes from server on all 4 | server data on all 4 (410 KB on the page load) |
| Python, 1517-byte hello, 3 rounds | handshake timeout 3/3, 0 bytes from server | TLS 1.3 and HTTP status 3/3 |
| Python direct, no relay (control) | reset 2/2 | - |

## Q2: a user-mode HTTP method

`tools/spike/http-variants.ps1 -HostName medium.com -Rounds 2` (no admin). Candidates
are the tpws (user-mode) methods from the blockcheck script in the zapret bundle
(`blockcheck.sh`, tpws section) and their definitions in `zapret-antidpi.lua`. Pass
means the server sent any status line, so the ISP let the request through.

| Variant | example.org (control host) | medium.com, 2 rounds |
|---|---|---|
| control `Host: medium.com` | 200 | FAIL 2/2 |
| `host:` (hostcase) | 200 | FAIL |
| `hoSt:` (hostspell) | 200 | **PASS 2/2, 301 from the real server** |
| `Host: medium.com.` (hostdot) | 200 | FAIL |
| `Host: medium.com<TAB>` (hosttab) | 400 | passes the ISP, server answers 400: unusable |
| `Host:medium.com` (hostnospace) | 200 | FAIL |
| `Host: MeDiUm.cOm` (domcase) | 200 | FAIL |
| `HEAD  /` (methodspace) | 200 | FAIL |
| LF line ends (unixeol) | 200 | FAIL |
| leading CRLF (methodeol) | 200 | FAIL |
| cuts at method+2, midsld, both | 200 | FAIL |
| `host:` plus cuts | 200 | FAIL |
| padding header before Host, 1 to 8 KiB (hostpad) | 200 | FAIL |

Findings:

- One user-mode method passes: respelling the header name as `hoSt:`. Header names
  are case-insensitive (RFC 9110 section 5.1), so a conforming server must accept it;
  medium.com's server did. The relay sees plain HTTP and can rewrite the header name
  in place without changing the length.
- The split rows here failed, but the elevated run showed the pieces were merged
  before the wire, so splitting is untested for HTTP on this PC. The earlier zapret
  measurement (in-order split fails, only disorder worked) points the same way.
- Steam store and content servers accept `hoSt:` (elevated run above).
- The split rows are not a valid test of splitting on this PC: the pieces were merged
  before the wire (see wire evidence above).

## Q3: do Steam downloads use port 80

Source: the Steam client's own `logs/content_log.txt` (no admin), lines of the form
`HTTP/2 (SteamCache,N) - cacheN-hkg1.steamcontent.com (IP:443 ...)`.

| Day | HTTP/2 connections (all port 443) | plain HTTP connections |
|---|---|---|
| 2026-11-06 | 15 | 6 |
| 2026-11-10 | 13 | 24 |
| 2026-11-14 | 30 | 0 |

Findings:

- Downloads use both. The plain HTTP lines log the address as `0.0.0.0:0`, so their
  port is not recorded; port 80 is the likely one and is still to be confirmed.
- The content hosts are `*.steamcontent.com`, which is on zapret's Steam list.
  `cache5-hkg1.steamcontent.com` answers 200 on 80 and 443 with zapret running.
- HTTP support therefore matters for downloads. The elevated run tests the `hoSt:`
  method against a content server with zapret stopped.

## Q4 (partial): loopback DNS on 127.0.0.2:53

A UDP socket bound to 127.0.0.2:53 received the query sent by
`Resolve-DnsName probe.example -Server 127.0.0.2` (31 bytes from 127.0.0.1, a DNS
header with one question). Internet Connection Sharing, which holds 0.0.0.0:53, does
not take queries sent to 127.0.0.2. This closes the open point in question 5.

`Get-DnsClientCache` returns nothing without elevation on this PC, so the elevated
run used the DNS client event log instead (above). A client that resolves on its own
(DNS over HTTPS, cached addresses) would bypass the relay; the Steam client is still
to be checked.

## Q6: crates proposal (conditional on a go)

Recommendation: std threads and a hand-written DNS codec; no async runtime.

- Load is small: one user's Steam client and browser, tens of concurrent
  connections. Two threads per relayed connection (one per direction) with blocking
  sockets stays in the low hundreds of threads.
- The DNS side needs little: read the question name and type, answer A with the
  relay address, answer AAAA and HTTPS (type 65) with an empty answer, forward
  everything else as raw bytes, and build one A query per upstream lookup. That is a
  few hundred lines with tests against real captured packets, the same pattern as the
  TLS parser.
- An async runtime would add a large dependency tree to a tool that friends run with
  admin rights, for a gain that only shows at thousands of connections.
- One gap std does not cover: Ctrl+C and service stop on Linux and macOS (signals).
  Options are a small FFI shim as on Windows, or the `ctrlc` crate. Decide in
  milestone 10.
