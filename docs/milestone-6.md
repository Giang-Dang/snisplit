# Milestone 6: `snisplit probe <domain>`

Done when (PLAN.md section 4, row 6): `probe` reproduces the no-bypass and split rows
of PLAN.md section 1 for a domain and prints a suggested profile.

Status: done on the night of 2026-11-14 to 15, without administrator rights. The
three acceptance domains give the results section 1 and milestone 4 predict (below).

## What it does

`snisplit probe <domain> [--upstream <ip[:port]>]... [--timeout <seconds>] [--runs <n>]`

- Resolves the domain through the upstream DNS servers directly (`UdpResolver`, the
  same resolver `run` uses; default: the system's DNS servers, as `run`), so NRPT rules
  of a running `snisplit run` do not matter. Loopback, unspecified and broadcast
  answers are refused with an error: probe never connects to the relay itself.
- Connects to the first usable IPv4 address, one fresh connection per attempt and
  run, Nagle off, and sends the first flight with `snisplit_relay::write_pieces` and
  the relay's 30 ms gap (`plan::PIECE_GAP`, passed in by the cli).
- Attempts, in table order: TLS on port 443 with the whole hello in one write, then
  `tls-split 1`, `tls-split host-start`, `tls-split domain-mid`,
  `tls-split 1 domain-mid`; HTTP on port 80 with `Host:` as written, then `hoSt:`.
  The TLS cuts come from `first_flight::decide` with a one-domain profile listing the
  probed name, so they are exactly the relay's cuts for the suggested profile. The
  respelled request goes through `http_stream::HostRespeller`.
- Rounds are interleaved: run 1 of every attempt, then run 2. An attempt "works"
  only if every run answered; "mixed" if some did; "fails" if none did.
- Reads until the first TLS record header or the HTTP status line, then closes. The
  handshake is never completed.
- No listening sockets, no DNS rule changes, no elevation.

Outcomes per run: `handshake 0x0303` / `alert 0x0303` (a TLS record header of type 22
or 21, major version 3, minor 0-4, length 1 to 2^14+2048), the HTTP status line
(`HTTP/1.x ...`, control bytes shown as `?`, at most 200 characters), `unexpected
<first bytes in hex>`, `reset`, `closed` (end of stream with no data), `timed out`
(connected and sent, then silence), `connect failed: <reason>` (a connect timeout is
also this, so `timed out` always means the server was reached), `failed: <reason>`.

Suggestion:

- whole hello and plain HTTP both work: "not blocked here; no profile needed";
- otherwise, when a split or `hoSt:` gets through what failed as is: a profile file
  in the format `show-profile` prints. The split is the first that worked every run
  in the order `1`, `1 domain-mid` (the built-in medium and steam splits), then
  `domain-mid`, `host-start`. `http-method host-spell` when `hoSt:` worked every run
  (or when neither HTTP attempt did), `pass` only when plain HTTP worked and `hoSt:`
  did not. When no TLS attempt answered at all but `hoSt:` worked, the profile has
  `tls-ports none`. The name is the domain with dots turned into `-`, cut to 32
  characters;
- nothing worked every run: "nothing got through: the site may be down, blocked by
  address, or blocked by a method snisplit does not have";
- something failed and no profile helps (for example HTTPS open and port 80 dead, or
  `hoSt:` works but every split breaks HTTPS, which a profile could not avoid because
  it always splits TLS): the verdict says so.

Exit codes as before: 0 when the probe ran, whatever the verdict; 1 for a runtime
error (no DNS answer, no usable address); 2 for a usage error.

## Public API

`snisplit_core::probe` (pure, no I/O):

```rust
pub struct HelloRandom { pub random: [u8; 32], pub session_id: [u8; 32], pub key_share: [u8; 32] }
pub fn client_hello(host: &str, secrets: &HelloRandom) -> Option<Vec<u8>>;
pub enum Attempt { TlsWhole, TlsSplit(Vec<SplitPosition>), HttpPlain, HttpRespelled }
pub fn attempts() -> Vec<Attempt>;
pub struct FirstFlight { pub bytes: Vec<u8>, pub cuts: Vec<usize> }
pub fn first_flight(attempt: &Attempt, domain: &str, secrets: &HelloRandom) -> Option<FirstFlight>;
pub enum Outcome { Answered(Reply), Unexpected(Vec<u8>), Reset, Closed, TimedOut,
                   ConnectFailed(io::ErrorKind), Failed(io::ErrorKind) }
pub fn reply_outcome(protocol: Protocol, received: &[u8], ended: bool) -> Option<Outcome>;
pub struct AttemptRecord { pub attempt: Attempt, pub outcomes: Vec<Outcome> }
impl AttemptRecord { pub fn agreement(&self) -> Agreement }
pub struct Assessment { pub verdict: Verdict, pub https: Reach, pub http: Reach,
                        pub tls_split: Option<Vec<SplitPosition>>, pub profile: Option<String> }
impl Assessment { pub fn summary(&self) -> String }
pub fn assess(domain: &str, records: &[AttemptRecord]) -> Assessment;
```

`snisplit_relay::probe` (network):

```rust
pub struct ProbeSettings { pub runs: usize, pub timeout: Duration, pub piece_gap: Duration }
pub struct ProbeTarget { pub domain: String, pub address: Ipv4Addr, pub tls_port: u16, pub http_port: u16 }
pub struct ProbeReport { pub domain: String, pub address: Ipv4Addr,
                         pub records: Vec<AttemptRecord>, pub assessment: Assessment }
pub async fn probe<R: Resolve>(resolver: &R, domain: &str, settings: &ProbeSettings) -> io::Result<ProbeReport>;
pub async fn probe_target(target: &ProbeTarget, settings: &ProbeSettings) -> io::Result<ProbeReport>;
```

The cli (`crates/cli/src/probe.rs`) only picks the DNS servers, prints a starting
line, runs `probe` and prints the table, verdict and profile. A future
`crates/gui` calls `snisplit_relay::probe::probe` and reads the report.

## The ClientHello

Hand-written in `crates/core/src/probe/hello.rs`, no crate: record type 22, record
version 0x0301, ClientHello with legacy version 0x0303, 32 random bytes, a 32-byte
session id, 9 cipher suites (TLS 1.3 AES-128-GCM, AES-256-GCM, CHACHA20; ECDHE
ECDSA/RSA with AES-GCM and CHACHA20), null compression, then the extensions
server_name, supported_groups (x25519, secp256r1, secp384r1), signature_algorithms
(8 schemes), ALPN (h2, http/1.1), supported_versions (1.3, 1.2) and key_share
(x25519, 32 random bytes). The host always starts at byte 109 and the hello is
212 bytes plus the host. Random bytes come from `RandomState` hashers fed the clock;
they protect nothing because the handshake is abandoned. A test compares the hello
for `ab.cd` with a byte vector written by hand from RFC 8446 section 4.1.2 and RFC
6066 section 3; real servers answered it with a ServerHello record (type 22) below.

## Changes to the contract, and why

1. **A fifth TLS attempt, `tls-split 1 domain-mid`.** Section 1's split row is "byte
   1 + mid-domain", and it is the built-in steam profile's split, so the probe sends
   it too. It is second in the suggestion order, after `1`.
2. **`--runs <n>` (1-10, default 2)** and a repeatable `--upstream`, as `run` has.
3. **`--upstream` takes `ip[:port]`, IPv4 or IPv6, and may be loopback** (useful for
   a local resolver and for the cli tests); loopback answers are still refused. `run`
   keeps its `--upstream <ipv4>`.
4. **Two more outcomes:** `Unexpected` (bytes that are neither a plausible TLS record
   header nor `HTTP/1.`) and `Failed` (an I/O error other than reset, abort or broken
   pipe). A reset during the piece writes counts as `reset`.
5. **`tls-ports none` profiles** when no TLS attempt answered in any run and `hoSt:`
   worked; **no profile** when `hoSt:` helps but every split breaks HTTPS that works
   as is, because a profile always splits TLS on its ports.
6. **The domain must pass the profile-file domain rules** (two labels or more, a-z,
   0-9, `-`, `.`), so the printed profile always parses. `parse_domain` in
   `profile_file.rs` became `pub` for this, and `plan::PIECE_GAP` became `pub` so the
   probe uses the relay's gap.
7. **Four verdicts, not three:** `NotBlocked`, `ProfileSuggested`, `NothingWorked`,
   and `NoProfileHelps` for "something failed, and no snisplit profile fixes it".
8. **`http-method host-spell` also when neither HTTP attempt worked**, because
   respelling is never worse than passing through.

## Limits

- Some blockers answer HTTP with their own page. Any `HTTP/1.` reply counts as
  answered, so the status line is printed: compare it with what the site should send.
- The verdict is for the address the resolver returned. Through 8.8.8.8,
  steamcommunity.com resolved to an address inside the ISP's network where the name is
  not filtered (below); through the system DNS, which `run` also uses, it is blocked.
- `domain-mid` cuts the first label of the probed name, as the suggested profile
  would: for `store.steampowered.com` that is `store`, not `steampowered` as in the
  built-in steam profile.
- Possible contradiction with section 1, row 4 ("one split right after the start of
  the SNI extension: works"). The exact position of that row is not recorded in the
  repo (no zapret option for it in PLAN.md, docs or tools), so it could not be
  reproduced. The nearest attempt, `tls-split host-start` (one cut just before the
  host name, inside the SNI extension), was reset 2/2 on store.steampowered.com,
  medium.com and steamcommunity.com, and so was `tls-split domain-mid`; only cuts that
  include byte 1 answered. The milestone 2 spike agrees with the probe ("cut mid-host
  only: FAIL"). Either row 4 used an earlier position than `host-start`, or it no
  longer holds; not investigated, and no attempt was added for it.
- IPv4 only, TCP only (QUIC: docs/milestone-8.md), ports 443 and 80 only (not Steam's
  27015-27030).
- Not run here: probe while `snisplit run` is active (needs elevation for the NRPT
  rules). The design does not depend on it: probe never uses the OS resolver.

## Acceptance (no elevation), 2026-11-14 to 15

Before: `Get-Process snisplit -ErrorAction SilentlyContinue` returned nothing (count
0). Binary: debug build of commit 76ad6bf, `.\target\debug\snisplit.exe`, run from
PowerShell. Each probe took about 2 s.

### `snisplit probe store.steampowered.com`

```
probing store.steampowered.com through DNS 1.1.1.1:53, 1.0.0.1:53: 7 attempts, 2 runs each, timeout 5 s
address 104.119.106.101

attempt                        result  run 1                           run 2
https, whole hello             fails   reset                           reset
https, tls-split 1             works   handshake 0x0303                handshake 0x0303
https, tls-split host-start    fails   reset                           reset
https, tls-split domain-mid    fails   reset                           reset
https, tls-split 1 domain-mid  works   handshake 0x0303                handshake 0x0303
http, Host:                    fails   reset                           reset
http, hoSt:                    works   HTTP/1.1 302 Moved Temporarily  HTTP/1.1 302 Moved Temporarily

verdict: HTTPS fails as is, tls-split 1 answered; HTTP fails as is, hoSt: answered; the profile below should get it through

suggested profile (save it to a file, then: snisplit run --profile-file <file>):

name store-steampowered-com
domains store.steampowered.com
tls-ports 443
tls-split 1
http-ports 80
http-method host-spell
exit code: 0
```

The whole hello is reset (section 1 "no bypass"), the splits that include byte 1
answer (section 1 row 2, "byte 1 + mid-domain"), plain HTTP is reset and `hoSt:`
passes (milestone 4), as predicted. One single cut that does not include byte 1
(`host-start`, `domain-mid`) is reset, as in the milestone 2 spike ("cut mid-host
only: FAIL"); see Limits for how this sits with section 1 row 4.

### `snisplit probe medium.com`

```
probing medium.com through DNS 1.1.1.1:53, 1.0.0.1:53: 7 attempts, 2 runs each, timeout 5 s
address 162.159.152.4

attempt                        result  run 1                           run 2
https, whole hello             fails   reset                           reset
https, tls-split 1             works   handshake 0x0303                handshake 0x0303
https, tls-split host-start    fails   reset                           reset
https, tls-split domain-mid    fails   reset                           reset
https, tls-split 1 domain-mid  works   handshake 0x0303                handshake 0x0303
http, Host:                    fails   reset                           reset
http, hoSt:                    works   HTTP/1.1 301 Moved Permanently  HTTP/1.1 301 Moved Permanently

verdict: HTTPS fails as is, tls-split 1 answered; HTTP fails as is, hoSt: answered; the profile below should get it through

suggested profile (save it to a file, then: snisplit run --profile-file <file>):

name medium-com
domains medium.com
tls-ports 443
tls-split 1
http-ports 80
http-method host-spell
exit code: 0
```

The suggestion has the built-in medium profile's split and method.

### `snisplit probe example.org`

```
probing example.org through DNS 1.1.1.1:53, 1.0.0.1:53: 7 attempts, 2 runs each, timeout 5 s
address 172.66.157.237

attempt                        result  run 1             run 2
https, whole hello             works   handshake 0x0303  handshake 0x0303
https, tls-split 1             works   handshake 0x0303  handshake 0x0303
https, tls-split host-start    works   handshake 0x0303  handshake 0x0303
https, tls-split domain-mid    works   handshake 0x0303  handshake 0x0303
https, tls-split 1 domain-mid  works   handshake 0x0303  handshake 0x0303
http, Host:                    works   HTTP/1.1 200 OK   HTTP/1.1 200 OK
http, hoSt:                    works   HTTP/1.1 200 OK   HTTP/1.1 200 OK

verdict: not blocked here; no profile needed
exit code: 0
```

Every TLS answer is a handshake record, not an alert, so the hand-built hello is
accepted by a real server.

### Control: curl directly

```
PS> foreach ($u in 'https://store.steampowered.com/','http://store.steampowered.com/','https://example.org/') { $code = curl.exe -s -o NUL -w "%{http_code} exit=%{exitcode} %{errormsg}" --max-time 15 $u; "curl $u -> $code" }
curl https://store.steampowered.com/ -> 000 exit=35 Recv failure: Connection was reset
curl http://store.steampowered.com/ -> 000 exit=56 Recv failure: Connection was reset
curl https://example.org/ -> 200 exit=0
```

### A Steam profile domain, and the resolver matters

`snisplit probe steamcommunity.com --upstream 8.8.8.8 --runs 3`:

```
probing steamcommunity.com through DNS 8.8.8.8:53: 7 attempts, 3 runs each, timeout 5 s
address 118.68.80.196

attempt                        result  run 1                           run 2                           run 3
https, whole hello             works   handshake 0x0303                handshake 0x0303                handshake 0x0303
https, tls-split 1             works   handshake 0x0303                handshake 0x0303                handshake 0x0303
https, tls-split host-start    works   handshake 0x0303                handshake 0x0303                handshake 0x0303
https, tls-split domain-mid    works   handshake 0x0303                handshake 0x0303                handshake 0x0303
https, tls-split 1 domain-mid  works   handshake 0x0303                handshake 0x0303                handshake 0x0303
http, Host:                    works   HTTP/1.1 302 Moved Temporarily  HTTP/1.1 302 Moved Temporarily  HTTP/1.1 302 Moved Temporarily
http, hoSt:                    works   HTTP/1.1 302 Moved Temporarily  HTTP/1.1 302 Moved Temporarily  HTTP/1.1 302 Moved Temporarily

verdict: not blocked here; no profile needed
exit code: 0
```

`snisplit probe steamcommunity.com` (system DNS):

```
probing steamcommunity.com through DNS 1.1.1.1:53, 1.0.0.1:53: 7 attempts, 2 runs each, timeout 5 s
address 104.119.104.211

attempt                        result  run 1                           run 2
https, whole hello             fails   reset                           reset
https, tls-split 1             works   handshake 0x0303                handshake 0x0303
https, tls-split host-start    fails   reset                           reset
https, tls-split domain-mid    fails   reset                           reset
https, tls-split 1 domain-mid  works   handshake 0x0303                handshake 0x0303
http, Host:                    fails   reset                           reset
http, hoSt:                    works   HTTP/1.1 302 Moved Temporarily  HTTP/1.1 302 Moved Temporarily

verdict: HTTPS fails as is, tls-split 1 answered; HTTP fails as is, hoSt: answered; the profile below should get it through

suggested profile (save it to a file, then: snisplit run --profile-file <file>):

name steamcommunity-com
domains steamcommunity.com
tls-ports 443
tls-split 1
http-ports 80
http-method host-spell
exit code: 0
```

curl agrees: the default path is reset, the address 8.8.8.8 returned is not.

```
curl  -> 000 exit=35 ip= Recv failure: Connection was reset
curl --resolve steamcommunity.com:443:118.68.80.196 -> 200 exit=0 ip=118.68.80.196
```

So on this ISP the same name is filtered on the way to one address and not to
another. Who runs 118.68.80.196 was not checked. It does not change the probe,
which reports the address it tested and by default uses the same DNS servers as
`run`.

## Tests and gates

68 new tests (404 before, 472 after): hello bytes against a hand-written RFC layout
and the inspector's host range; reply classification against hand-written headers;
cuts at hand-derived offsets (1, 109, 112 for `example.org`); verdicts and literal
profile texts; local TCP listeners on port 0 that reset when the first read holds
the whole host, answer only after three separate reads (the read sizes 1, 111, 111
prove the cuts reach the wire), reset between pieces, close, stay silent or send
foreign bytes; a closed port; loopback-only DNS answers; argument parsing; the
printed profile parsed by the profile-file parser for a 33-character Steam name, an
HTTP-only profile and a `pass` profile; the binary's exit codes with a silent and a
loopback-answering local DNS server. No test touches the internet or binds 53, 80 or
443.

Gates on each of the three commits (c1d73c7, b0dd78f, 76ad6bf): `cargo fmt --all
--check`, `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`, the same
with `--target x86_64-unknown-linux-gnu`, `cargo test --workspace` (441, 452 and 472
passed, 0 failed).
