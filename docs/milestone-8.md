# Milestone 8: QUIC measurement and decision

PLAN.md row 8: "A written result on whether this ISP blocks QUIC by SNI." Measured on
this PC on 2026-11-14 between 16:40 and 16:55 UTC (the pre-checks and the first TCP baseline ran just before 16:40 and were not timestamped), one ISP, IPv4 only, without
administrator rights. Probe times below are UTC (the probe prints UTC; the spike
relay prints local time).

## Recommendation: no QUIC work needed

- Measured: this ISP does not block QUIC by SNI for the names tested. medium.com and
  three listed Steam names (`shared.fastly.steamstatic.com`,
  `shared.cloudflare.steamstatic.com`, `steamcdn-a.akamaihd.net`) complete a QUIC
  handshake and answer an HTTP/3 request in every round, while the same names over
  TCP TLS are reset in the same minutes, also when TCP is pinned to the address QUIC
  used.
- Consequence: QUIC is not a leak snisplit has to close. When an app does reach a
  listed name over QUIC, it gets through without snisplit.
- Read from the code, not run routed: under snisplit a listed name resolves to a
  loopback address, with no IPv6 address and no HTTPS record, so no DNS-advertised
  HTTP/3. Nothing listens on UDP 443 there; a QUIC packet meets a closed port (shown
  unelevated below). The expected result is a fallback to TCP, which the relay
  handles; that fallback was not measured with a browser.
- A QUIC relay would add a second protocol for no measured gain. Blocking QUIC
  locally is already what happens, with no code.

## Before the measurement

Commands as run in PowerShell (shortened where marked `...`), output trimmed:

```
> Get-Process snisplit -ErrorAction SilentlyContinue; "proc-exit:$?"; Get-DnsClientNrptRule;
  "nrpt-count: $((Get-DnsClientNrptRule | Measure-Object).Count)"; ...
proc-exit:False
nrpt-count: 0

> Get-Service zapret*, windivert, WARPSvc -ErrorAction SilentlyContinue | Format-Table Name,Status;
  Get-Process winws, warp-svc -ErrorAction SilentlyContinue | Format-Table Name; ...;
  "udp443-endpoints: $((Get-NetUDPEndpoint -LocalPort 443 -ErrorAction SilentlyContinue | Measure-Object).Count)"
Name
----
warp-svc
udp443-endpoints: 0

> warp-cli status
Status update: Disconnected
Reason: Manual Disconnection
```

(The adapter list from the second command is cut: one Ethernet adapter and the WSL
virtual adapter were up, no tunnel adapter.) So: no snisplit process, no NRPT rule,
no zapret service or `winws` process, the WARP service process running but
disconnected, and nothing holding UDP 443. The system resolver is 1.1.1.1
(docs/milestone-2-spike.md).

## Tooling

`curl.exe --version` prints `curl 8.21.0 (Windows) libcurl/8.21.0 Schannel ...` with
features `alt-svc AsynchDNS HSTS HTTPS-proxy IDN IPv6 Kerberos Largefile libz SPNEGO
SSL SSPI threadsafe Unicode UnixSockets`: no `HTTP3` (and no `HTTP2`), so it cannot
speak QUIC. The probe is `tools/quic/quic_probe.py` on `aioquic` 1.2.0 and `dnspython`
2.7.0, in the conda environment `snisplit-quic` created from
`tools/quic/environment.yml`:

```
conda env create -f tools/quic/environment.yml
conda run -n snisplit-quic python -I tools/quic/quic_probe.py handshake ADDRESS=SNI ... --rounds 5
conda run -n snisplit-quic python -I tools/quic/quic_probe.py version-negotiation ADDRESS ...
conda run -n snisplit-quic python -I tools/quic/quic_probe.py https-records NAME ...
```

- `handshake` connects to the given IPv4 address on UDP 443 with the given SNI
  (aioquic uses the address as SNI only when none is set; checked in
  `aioquic/asyncio/client.py` line 66), ALPN `h3`, certificate checking on, then sends
  `GET /` over HTTP/3. Per attempt it prints datagrams sent and received (counted at
  the UDP socket, so a Retry or a close counts as a reply), whether the handshake
  completed, the HTTP/3 status, elapsed time, the close code and the QUIC/HTTP/3
  events seen. `close code=0x0` after a status is the probe's own normal close.
- `version-negotiation` sends a 1200-byte long-header packet with the unused version
  `0x1a2a3a4a` and no TLS data, so no SNI. A QUIC server that handles unknown versions
  answers with a Version Negotiation packet.
- `https-records` asks the system resolver for type 65 records. `Resolve-DnsName`
  has no HTTPS type here, and `nslookup -type=TYPE65` answers `unknown query type`.

aioquic's ClientHello fits in one datagram: the Akamai attempts below show `sent=1
received=1`, a TLS alert in reply to that single Initial packet.

## Q1. Does the ISP treat QUIC to an affected name differently?

### Who offers HTTP/3

```
> quic_probe.py https-records medium.com cloudflare.com www.google.com store.steampowered.com
  steamcommunity.com api.steampowered.com login.steampowered.com help.steampowered.com
  checkout.steampowered.com shared.akamai.steamstatic.com shared.fastly.steamstatic.com
  shared.cloudflare.steamstatic.com cdn.akamai.steamstatic.com cdn.fastly.steamstatic.com
  clan.akamai.steamstatic.com avatars.fastly.steamstatic.com community.fastly.steamstatic.com
  steamcdn-a.akamaihd.net steamuserimages-a.akamaihd.net cache5-hkg1.steamcontent.com steam-chat.com
medium.com HTTPS none (NoAnswer)
cloudflare.com HTTPS 1 . alpn="h2" ipv4hint="104.16.132.229,104.16.133.229" ipv6hint="2606:4700::6810:84e5,2606:4700::6810:85e5"
www.google.com HTTPS 1 . alpn="h2"
```

The other 18 lines, one per Steam name, all read `<name> HTTPS none (NoAnswer)`.

No tested name advertises `h3` in DNS. The `alt-svc` header could not decide it
either: curl here speaks only HTTP/1.1, and neither cloudflare.com nor www.google.com
sent `alt-svc` on HTTP/1.1, although both speak HTTP/3 (below). medium.com, read
through the milestone 2 split relay (`tools/spike/loopback-relay.ps1 -Upstream
medium.com -Seconds 25`, unelevated, then `curl.exe -sI --connect-to
medium.com:443:127.0.0.2:443 https://medium.com/`), answered `HTTP/1.1 403
Forbidden`, `Server: cloudflare`, no `alt-svc` (relay log: `pieces=1+516
bytes_from_server=5278`). The Steam names are reset over TCP, so their `alt-svc` is
unreadable without a bypass. The QUIC handshake itself is therefore the test of
whether a name offers HTTP/3.

### medium.com, with controls, interleaved, 5 rounds

Addresses: medium.com 162.159.152.4 and 162.159.153.4 (Cloudflare); cloudflare.com
104.16.132.229; www.google.com 142.251.150.119. Control (a) is cloudflare.com sent to
medium.com's own address 162.159.152.4. Control (b) is cloudflare.com and
www.google.com at their own addresses. Each round runs a, medium x2, a, b, b.

Verbatim lines, rounds 2 to 4 omitted (same outcome in every line):

```
> quic_probe.py handshake 162.159.152.4=cloudflare.com 162.159.152.4=medium.com 162.159.153.4=medium.com
  162.159.152.4=cloudflare.com 104.16.132.229=cloudflare.com 142.251.150.119=www.google.com --rounds 5
16:45:42 round=1 addr=162.159.152.4 sni=cloudflare.com sent=6 received=13 handshake=complete h3=301 ms=390 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx7,HeadersReceivedx1,DataReceivedx3,ConnectionTerminatedx1
16:45:42 round=1 addr=162.159.152.4 sni=medium.com sent=10 received=21 handshake=complete h3=403 ms=531 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx7,HeadersReceivedx1,DataReceivedx1,ConnectionTerminatedx1
16:45:43 round=1 addr=162.159.153.4 sni=medium.com sent=8 received=17 handshake=complete h3=403 ms=390 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx7,HeadersReceivedx1,DataReceivedx1,ConnectionTerminatedx1
16:45:43 round=1 addr=162.159.152.4 sni=cloudflare.com sent=8 received=13 handshake=complete h3=301 ms=437 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx7,HeadersReceivedx1,DataReceivedx3,ConnectionTerminatedx1
16:45:43 round=1 addr=104.16.132.229 sni=cloudflare.com sent=5 received=12 handshake=complete h3=301 ms=468 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx7,HeadersReceivedx1,DataReceivedx3,ConnectionTerminatedx1
16:45:44 round=1 addr=142.251.150.119 sni=www.google.com sent=11 received=89 handshake=complete h3=200 ms=500 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,StreamDataReceivedx3,ConnectionIdIssuedx1,HeadersReceivedx1,DataReceivedx1,ConnectionTerminatedx1
16:45:53 round=5 addr=162.159.152.4 sni=cloudflare.com sent=7 received=12 handshake=complete h3=301 ms=468 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx7,HeadersReceivedx1,DataReceivedx3,ConnectionTerminatedx1
16:45:53 round=5 addr=162.159.152.4 sni=medium.com sent=8 received=17 handshake=complete h3=403 ms=438 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx8,HeadersReceivedx1,DataReceivedx2,ConnectionTerminatedx1
16:45:54 round=5 addr=162.159.153.4 sni=medium.com sent=8 received=17 handshake=complete h3=403 ms=468 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx7,HeadersReceivedx1,DataReceivedx1,ConnectionTerminatedx1
16:45:54 round=5 addr=162.159.152.4 sni=cloudflare.com sent=8 received=12 handshake=complete h3=301 ms=515 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx6,HeadersReceivedx1,DataReceivedx2,ConnectionTerminatedx1
16:45:55 round=5 addr=104.16.132.229 sni=cloudflare.com sent=7 received=11 handshake=complete h3=301 ms=422 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx6,HeadersReceivedx1,DataReceivedx3,ConnectionTerminatedx1
16:45:55 round=5 addr=142.251.150.119 sni=www.google.com sent=10 received=88 handshake=complete h3=200 ms=516 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,StreamDataReceivedx4,ConnectionIdIssuedx1,HeadersReceivedx1,DataReceivedx2,ConnectionTerminatedx1
```

All 30 attempts completed the handshake and got an HTTP/3 status; medium.com 403 on
both addresses 10 of 10, in 359 to 562 ms. The 403 is Cloudflare's answer inside a
verified TLS session (certificate checking on), the same 403 medium.com gives over
TCP through the split relay; it is not an ISP block page.

An earlier run (16:43-16:44, 5 rounds, same cases) sent `HEAD /` instead of `GET /`.
Every handshake completed there too, but each medium.com request timed out after 8 s,
10 of 10. A one-round rerun with event logging showed why:

```
16:45:19 round=1 addr=162.159.152.4 sni=medium.com sent=10 received=17 handshake=complete h3=- ms=8015 timeout close code=0x10e reason='content-length does not match data size' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx7,ConnectionTerminatedx1
```

aioquic received medium.com's response and rejected it itself (HTTP/3 message error
0x10e): its check does not allow a HEAD response with a `content-length`. The
response had arrived; the probe was changed to `GET`.

### Steam names, 3 rounds

```
> quic_probe.py version-negotiation 162.159.152.4 104.16.132.229 142.251.150.119 23.15.142.182
  23.15.140.216 23.35.148.170 23.2.16.115 199.232.211.52 104.84.150.164 103.28.54.133
16:46:14 addr=162.159.152.4 reply 27 bytes, version negotiation=True
16:46:14 addr=104.16.132.229 reply 27 bytes, version negotiation=True
16:46:14 addr=142.251.150.119 reply 35 bytes, version negotiation=True
16:46:14 addr=23.15.142.182 no reply (timeout)
16:46:19 addr=23.15.140.216 no reply (timeout)
16:46:24 addr=23.35.148.170 no reply (timeout)
16:46:29 addr=23.2.16.115 no reply (timeout)
16:46:34 addr=199.232.211.52 reply 39 bytes, version negotiation=True
16:46:34 addr=104.84.150.164 no reply (timeout)
16:46:39 addr=103.28.54.133 no reply (timeout)
```

The Akamai addresses (23.x, 104.84.x) ignore the unknown version even where they do
speak QUIC (104.84.150.164 below), so this check only shows QUIC where it gets a reply.

Round 1 verbatim (rounds 2 and 3 give the same outcome per name). The stderr lines
`Future exception was never retrieved` / `ConnectionError` that asyncio printed after
each cache5 attempt are left out:

```
> quic_probe.py handshake 23.15.142.182=store.steampowered.com 23.15.140.216=steamcommunity.com
  23.35.148.170=api.steampowered.com 23.2.16.115=shared.akamai.steamstatic.com
  199.232.211.52=shared.fastly.steamstatic.com 104.84.150.164=shared.cloudflare.steamstatic.com
  103.28.54.133=cache5-hkg1.steamcontent.com --rounds 3 --timeout 6
16:46:46 round=1 addr=23.15.142.182 sni=store.steampowered.com sent=1 received=1 handshake=none h3=- ms=577 ConnectionError:  close code=0x150 reason='200:internal error' events=ConnectionTerminatedx1
16:46:47 round=1 addr=23.15.140.216 sni=steamcommunity.com sent=1 received=1 handshake=none h3=- ms=563 ConnectionError:  close code=0x150 reason='200:internal error' events=ConnectionTerminatedx1
16:46:47 round=1 addr=23.35.148.170 sni=api.steampowered.com sent=1 received=1 handshake=none h3=- ms=561 ConnectionError:  close code=0x150 reason='200:internal error' events=ConnectionTerminatedx1
16:46:48 round=1 addr=23.2.16.115 sni=shared.akamai.steamstatic.com sent=1 received=1 handshake=none h3=- ms=547 ConnectionError:  close code=0x150 reason='200:internal error' events=ConnectionTerminatedx1
16:46:49 round=1 addr=199.232.211.52 sni=shared.fastly.steamstatic.com sent=10 received=9 handshake=complete h3=404 ms=1375 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,StreamDataReceivedx5,ConnectionIdIssuedx3,HeadersReceivedx1,DataReceivedx1,ConnectionTerminatedx1
16:46:50 round=1 addr=104.84.150.164 sni=shared.cloudflare.steamstatic.com sent=11 received=16 handshake=complete h3=301 ms=827 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx4,HeadersReceivedx1,DataReceivedx1,ConnectionTerminatedx1
16:46:57 round=1 addr=103.28.54.133 sni=cache5-hkg1.steamcontent.com sent=6 received=0 handshake=none h3=- ms=6625 timeout close code=0x0 reason='' events=ConnectionTerminatedx1
```

One more listed name, with an unaffected SNI on the same address as control, 3
rounds; round 1 verbatim, aioquic's own `Error: 298` log line before each control line
left out, rounds 2 and 3 the same:

```
> quic_probe.py handshake 104.84.150.165=steamcdn-a.akamaihd.net 104.84.150.165=www.google.com --rounds 3 --timeout 6
16:54:14 round=1 addr=104.84.150.165 sni=steamcdn-a.akamaihd.net sent=11 received=15 handshake=complete h3=200 ms=702 close code=0x0 reason='' events=ProtocolNegotiatedx1,HandshakeCompletedx1,ConnectionIdIssuedx1,StreamDataReceivedx3,HeadersReceivedx1,ConnectionTerminatedx1
16:54:15 round=1 addr=104.84.150.165 sni=www.google.com sent=3 received=4 handshake=none h3=- ms=562 ConnectionError:  close code=0x12a reason="hostname 'www.google.com' doesn't match either of DNSPattern(pattern=b'a248.e.akamai.net'), DNSPattern(pattern=b'*.akamaihd-staging.net'), DNSPattern(pattern=b'*.akamaihd.net'), DNSPattern(pattern=b'*.akamaized-staging.net'), DNSPattern(pattern=b'*.akamaized.net')" events=ProtocolNegotiatedx1,ConnectionTerminatedx1
```

The control gets Akamai's default certificate and the probe ends it on the name
mismatch (0x12a), so that address answers QUIC for any SNI.

Control on the addresses that sent the alert, 3 rounds (`23.15.142.182` with
cloudflare.com, store.steampowered.com and www.google.com; `23.2.16.115` with
cloudflare.com and shared.akamai.steamstatic.com). Round 1, with the trailing
`events=` field and aioquic's `Error: 298` log line cut; rounds 2 and 3 the same:

```
16:47:36 round=1 addr=23.15.142.182 sni=cloudflare.com sent=1 received=2 handshake=none h3=- ms=890 ConnectionError:  close code=0x150 reason='200:internal error'
16:47:36 round=1 addr=23.15.142.182 sni=store.steampowered.com sent=1 received=1 handshake=none h3=- ms=562 ConnectionError:  close code=0x150 reason='200:internal error'
16:47:37 round=1 addr=23.15.142.182 sni=www.google.com sent=1 received=1 handshake=none h3=- ms=562 ConnectionError:  close code=0x150 reason='200:internal error'
16:47:37 round=1 addr=23.2.16.115 sni=cloudflare.com sent=5 received=4 handshake=none h3=- ms=578 ConnectionError:  close code=0x12a reason="hostname 'cloudflare.com' doesn't match either of DNSPattern(pattern=b'a248.e.akamai.net'), DNSPattern(pattern=b'*.akamaihd-staging.net'), DNSPattern(pattern=b'*.akamaihd.net'), DNSPattern(pattern=b'*.akamaized-staging.net'), DNSPattern(pattern=b'*.akamaized.net')"
16:47:38 round=1 addr=23.2.16.115 sni=shared.akamai.steamstatic.com sent=1 received=1 handshake=none h3=- ms=561 ConnectionError:  close code=0x150 reason='200:internal error'
```

Reading of the Akamai and Valve rows:

- `0x150` is a QUIC CRYPTO_ERROR carrying TLS alert 80 (internal_error), sent in
  reply to the first Initial. On 23.15.142.182 the unaffected names cloudflare.com and
  www.google.com get exactly the same reply as store.steampowered.com, so on that
  address the alert does not depend on the SNI being a Steam name.
- On 23.2.16.115 the two SNIs are treated differently: cloudflare.com gets the
  default certificate, shared.akamai.steamstatic.com gets the alert. The likely
  reading, from the 23.15.142.182 control and from steamcdn-a.akamaihd.net passing on
  another Akamai address, is that Akamai refuses HTTP/3 for property names that have
  it turned off. Not proved: that the ISP did not forge the alert. It would have to
  build a valid Initial-protected QUIC packet, and on TCP this ISP blocks with resets.
- cache5-hkg1.steamcontent.com (103.28.54.133, Valve's range) answers neither the
  handshake nor the version probe. No control fits that address. Likely no QUIC
  service there; silence by the ISP is not excluded.

### TCP on the same names, after the QUIC runs

`curl.exe -sv --max-time 15 https://<name>/ -o NUL`, 16:47:56-16:47:57, lines
filtered with `Select-String 'Trying|Recv failure|failed|^< HTTP'`:

```
== https://medium.com/
  *   Trying 162.159.153.4:443...
  * Recv failure: Connection was reset
  * schannel: failed to receive handshake, SSL/TLS connection failed
  exit: 35
== https://shared.fastly.steamstatic.com/
  *   Trying 199.232.215.52:443...
  * Recv failure: Connection was reset
  exit: 35
== https://shared.cloudflare.steamstatic.com/
  *   Trying 104.84.150.191:443...
  * Recv failure: Connection was reset
  exit: 35
== https://store.steampowered.com/
  *   Trying 23.15.142.182:443...
  * Recv failure: Connection was reset
  exit: 35
== https://cloudflare.com/
  *   Trying 104.16.132.229:443...
  < HTTP/1.1 301 Moved Permanently
  exit: 0
```

(The `schannel: failed` line, identical after each reset, is kept only once.) Pinned
to the addresses the QUIC runs used, with `--connect-to <name>:443:<address>:443`,
16:47-16:54:

| SNI | Address | TCP TLS result |
|---|---|---|
| cloudflare.com | 162.159.152.4 (medium.com's) | `< HTTP/1.1 301 Moved Permanently`, exit 0 |
| medium.com | 162.159.152.4 | `Recv failure: Connection was reset`, exit 35 |
| shared.fastly.steamstatic.com | 199.232.211.52 | `Recv failure: Connection was reset` |
| shared.cloudflare.steamstatic.com | 104.84.150.164 | `Recv failure: Connection was reset` |
| steamcdn-a.akamaihd.net | 104.84.150.165 (own DNS answer, not pinned) | `Recv failure: Connection was reset`, exit 35 |
| www.google.com | 199.232.211.52 and 104.84.150.164 | `SEC_E_WRONG_PRINCIPAL`, exit 60: the server sent its certificate |
| cloudflare.com | 199.232.211.52 and 104.84.150.164 | `Recv failure: Connection was reset` |

So TCP to the Fastly and Akamai addresses is not blocked by address (www.google.com
gets the server's certificate), and the Steam SNIs are reset there, as on
medium.com's address. One result is unexplained and does not change the answer:
cloudflare.com as SNI is also reset on the Fastly and Akamai addresses (but not on
Cloudflare's). Through the split relay to 199.232.211.52 the server did send 3318
bytes for that SNI (`pieces=1+516 bytes_from_server=3318`), which points at the ISP
rather than the server, but curl still ended with a reset; not followed further.

### Result table

| Name | QUIC address | TCP TLS, no bypass | QUIC handshake | HTTP/3 answer | Rounds |
|---|---|---|---|---|---|
| medium.com | 162.159.152.4, 162.159.153.4 | reset, also pinned to 162.159.152.4 | complete | 403 (Cloudflare) | 5 + 5 |
| control (a): cloudflare.com | 162.159.152.4 | 301, pinned | complete | 301 | 10 |
| control (b): cloudflare.com | 104.16.132.229 | 301 | complete | 301 | 5 |
| control (b): www.google.com | 142.251.150.119 | 200 (`curl -sI`) | complete | 200 | 5 |
| shared.fastly.steamstatic.com (listed) | 199.232.211.52 | reset, pinned | complete | 404 for `/` | 3 |
| shared.cloudflare.steamstatic.com (listed) | 104.84.150.164 | reset, pinned | complete | 301 | 3 |
| steamcdn-a.akamaihd.net (listed) | 104.84.150.165 | reset | complete | 200 | 3 |
| store.steampowered.com, steamcommunity.com, api.steampowered.com, shared.akamai.steamstatic.com (listed) | Akamai 23.x | reset: store tonight; community and API in milestone 2; shared.akamai not measured | server TLS alert 80 | none; likely HTTP/3 not offered (same alert for unaffected SNIs on 23.15.142.182) | 3 |
| cache5-hkg1.steamcontent.com (listed) | 103.28.54.133 | reset in milestone 2 | no reply | none; likely no QUIC on the address (no control) | 3 |

Answer: no. For every name that speaks HTTP/3 (medium.com and three listed Steam
names), the ISP lets QUIC through with the affected SNI in the clear, to the same
address where TCP with that SNI is reset. The main Steam web names (store,
community, API, Akamai static) did not complete a QUIC handshake, most likely
because the server does not offer HTTP/3 for them.

## Q2. Live TCP baseline

Before the QUIC runs (verbatim, last lines of `curl.exe -sv --max-time 15
https://medium.com/ -o NUL`):

```
* Host medium.com:443 was resolved.
* IPv6: (none)
* IPv4: 162.159.152.4, 162.159.153.4
*   Trying 162.159.152.4:443...
* schannel: disabled automatic use of client certificate
* ALPN: curl offers http/1.1
* Recv failure: Connection was reset
* schannel: failed to receive handshake, SSL/TLS connection failed
* closing connection #0
exit: 35
```

It failed the same way at 16:47:56, after the QUIC runs, so the TCP block was live
before and after every medium.com QUIC attempt and the comparison holds.

## Q3. What a browser's QUIC attempt meets while snisplit routes a name

Read from the code on 2026-11-14 around 16:48 UTC; another agent was editing
`crates/` at the time, so line numbers may move.

- `crates/relay/src/dns_server.rs`, `decide` (lines 138-154): a query whose name
  matches a profile domain is `Listed`.
- `answer_listed` (lines 156-181): if the name has an IPv4 address upstream, an A
  query gets `dns::answer_a` with the policy's answer address and a TTL of 5 s
  (`ANSWER_TTL_SECONDS`, line 20); every other query type, including AAAA (28) and
  HTTPS (65), gets `dns::answer_empty`. A listed name without an IPv4 address is
  forwarded unchanged. Per PLAN.md the answer address is the profile's loopback
  address, 127.0.0.2 for Steam and 127.0.0.3 for medium (where `policy.answer` is set
  was not read).
- `crates/core/src/dns.rs`, `answer_empty` (lines 63-66) and `response_start`
  (lines 159-165): NOERROR, the question echoed, zero answers. The browser gets no
  IPv6 address and no HTTPS record, hence no DNS-advertised `h3`, no ECH key and no
  alternative endpoint.
- The relay binds only `TcpListener`s (`crates/relay/src/relay.rs` line 49). A grep
  of all `.rs` files under `crates/` for `UdpSocket` and `UdpClient` outside tests
  finds only the DNS server socket on port 53 (`dns_server.rs` line 66) and the
  upstream DNS query sockets (`dns_server.rs` line 198, `upstream.rs` line 86).
  Nothing in snisplit listens on UDP 443.

QUIC datagrams to the loopback relay addresses, unelevated, snisplit not running
(the same UDP state as during a run, since snisplit never binds UDP 443):

```
> quic_probe.py version-negotiation 127.0.0.2 127.0.0.3 --timeout 3
16:48:44 addr=127.0.0.2 error: [WinError 10054] An existing connection was forcibly closed by the remote host
16:48:44 addr=127.0.0.3 error: [WinError 10054] An existing connection was forcibly closed by the remote host

> quic_probe.py handshake 127.0.0.2=store.steampowered.com 127.0.0.3=medium.com --rounds 3 --timeout 5
16:48:52 round=1 addr=127.0.0.2 sni=store.steampowered.com sent=6 received=0 handshake=none h3=- ms=5594 timeout close code=0x0 reason='' events=ConnectionTerminatedx1
16:48:57 round=1 addr=127.0.0.3 sni=medium.com sent=6 received=0 handshake=none h3=- ms=5608 timeout close code=0x0 reason='' events=ConnectionTerminatedx1
```

(Rounds 2 and 3 the same.)

- On a connected UDP socket the error is immediate (both lines in the same second,
  with a 3 s timeout): Windows reports the ICMP port unreachable as WSAECONNRESET
  (10054) on the next receive.
- aioquic, on an unconnected socket, does not see that error and times out. How fast
  a client gives up therefore depends on the client. From general knowledge, not
  checked here: Chromium races QUIC against TCP when it tries QUIC, so a dead QUIC
  path should not hold up the TCP connection to the relay.
- A browser tries QUIC only for a name it has an HTTP/3 hint for. Under snisplit the
  DNS hint is gone. A remembered `alt-svc: h3=":443"` from before the rule names the
  same host, which now resolves to loopback, so it meets the closed port above. A
  QUIC session opened to the real address before snisplit started would keep using
  the real address, and by Q1 that path works on this ISP.

## What was not measured

- One ISP, one evening (about 15 minutes of probes), one PC, IPv4 only.
- One QUIC client: aioquic 1.2.0, whose ClientHello fits one datagram, the easiest
  case for a DPI box to read. Chrome's larger ClientHello can span two Initial
  packets. The ISP passed the easy case, so the harder one very likely passes too,
  but Chrome's own QUIC to these names was not run.
- No browser against a routed snisplit (that needs administrator rights). Not
  measured: how long Chrome takes to fall back to TCP when a remembered `alt-svc`
  sends it to a loopback address, and whether the Steam client uses QUIC at all.
- Only `/` was requested on each name; other Steam content hosts were not tried.
- Why cloudflare.com as SNI is reset on Fastly and Akamai addresses over TCP.
- Whether the ISP blocks QUIC by SNI for names on other networks, or starts doing so
  later.
