# Milestone 2 spike: results so far

Answers to PLAN.md section 4.1, measured on the owner's PC on 2026-11-14. Not yet a
go/no-go: the Steam runs (zapret stopped), question 3 and question 4 are still open.

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
  byte 1: the first piece then lacks a full TLS record header. A cut only inside the
  host name does not help.
- Not yet proven at wire level: that each write left as its own TCP segment. Single
  write fails and cut-at-1 passes on the same path, so the wire did differ; a pktmon
  capture in the elevated run will record the segment sizes.
- Still to do: the same table for the Steam hosts with `zapret-steam` stopped.

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
| `Host: MeDiUm.CoM` (domcase) | 200 | FAIL |
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
- Splitting the request does not help HTTP, matching the earlier zapret measurement
  (in-order split fails, only disorder worked).
- Still to do: the HTTP Steam URL with `zapret-steam` stopped, and a check that
  Steam's content servers accept `hoSt:`.
- Blocked HTTP connections end with WSAECONNABORTED rather than a reset seen by curl
  as 56. Whether the ISP or local software (the antivirus web scanner) ends them is
  open; the pktmon capture will show whether a RST arrives from the network.

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

Open, needs admin: listing the OS DNS cache (`Get-DnsClientCache` returns nothing
without elevation on this PC), which is how the run will check that the Steam client
and Chrome resolve the blocked names through the system resolver. A client that
resolves on its own (DNS over HTTPS, cached addresses) would bypass the relay.

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
