# Milestone 3 plan: relay + DNS, HTTPS, foreground

Done when (PLAN.md section 4, made concrete):

1. Without admin, `snisplit run --no-routing --profile medium` relays a curl
   request made with `--resolve medium.com:443:127.0.0.3`, and medium.com answers.
2. Elevated, `snisplit run --profile medium` (zapret left running: medium is not on
   its list) makes `https://medium.com/` load in curl and Chrome through normal name
   resolution; `example.org` resolves exactly as before.
3. Elevated, with `zapret-steam` stopped (owner asked first), `snisplit run`:
   - the 4 HTTPS Steam URLs of PLAN.md section 1 return 200/302 from curl and Chrome;
   - the Steam client logs in and its Store page loads;
   - steamwebhelper resolves through the system resolver while the rules exist.
4. Ctrl+C removes every snisplit NRPT rule and flushes the DNS cache; afterwards the
   listed names resolve to their real addresses again. `snisplit cleanup` does the
   same after a crash.

## Rules for every contributor

Same as docs/milestone-1.md: no comments of any kind, ASCII only, gates
(`cargo fmt --all --check`, `cargo clippy --workspace --all-targets` with
`RUSTFLAGS=-D warnings`, `cargo test --workspace`) before every commit, unit tests
over about 150 lines in `<module>/tests.rs`, public-API tests in
`crates/<crate>/tests/`. Never push, never elevate, never add or remove NRPT rules,
never touch the zapret service or bundle. The owner's test rule: every assertion
compares against an expected value independent of the code under test; no tests of
test helpers; no assertions on what the setup just wrote; for every test, name a
plausible bug that makes it fail.

## Decisions

1. **Governing rule: a listed name is never worse off than without snisplit.** If
   the relay cannot handle a connection it says so in the log; it never silently
   drops a working path. Hence decisions 2-4.
2. **Ports.** A listed name resolves to the relay address for every port, so the
   relay listens on every TCP port its profiles declare. Steam: TLS on 443 and
   27015-27030 (the CM websocket endpoints observed use 443, 27018, 27019, 27020,
   27024; the range stops before 27036, which the Steam client itself listens on for
   local streaming), HTTP on 80. Medium: TLS 443, HTTP 80. UDP to
   listed names (QUIC, Steam's UDP checks) gets no listener and falls back or fails;
   recorded as a risk, measured in milestone 8.
3. **Routing by name, one loopback address per profile.** A `Listener` serves one
   profile, and Steam and Medium both need port 443, so each built-in profile gets
   its own address by its position in the built-in list: steam 127.0.0.2, medium
   127.0.0.3 (fixed, whatever is selected). Each profile has its own DNS server on
   `<address>:53` answering with that address, and its NRPT rules point there. The
   relay finds the target from the TLS SNI or the HTTP Host header, matches it
   against the listener's profile, resolves it upstream and connects to the same
   port. A connection without a host, or with an unlisted host, is closed and logged.
4. **HTTP in milestone 3 is pass-through.** Port 80 is relayed unchanged by Host; the
   `hoSt:` rewrite is milestone 4.
5. **TLS cut.** The profile's positions are applied as separate writes in order, with
   `piece_gap` between them. Default gap 30 ms: measured passing on every Steam host
   and medium.com. A 0 ms gap also passed on medium.com (3/3), not on Steam. The
   Steam profile keeps `[Offset(1), DomainMid]`, Medium `[Offset(1)]`; both measured.
6. **Upstream resolver.** The relay never resolves through the OS (its own rules
   would loop). It sends A queries over UDP to the system's configured IPv4 DNS
   servers, read once at start, loopback addresses excluded; `--upstream <ip>`
   overrides. An upstream answer that is loopback, unspecified or the relay address
   is refused.
7. **DNS server.** UDP on `<listen>:53`. For a name a profile lists: A gets the relay
   address with TTL 5, AAAA and HTTPS (type 65) get NOERROR with no answer, other
   types get NOERROR with no answer. Unlisted names are forwarded to the upstream
   unchanged and the answer relayed back. No TCP DNS in milestone 3 (answers are
   small).
8. **Resolver routing (Windows).** NRPT rules, one per listed suffix
   (`.steampowered.com` and the bare name), name server = relay address, comment
   `snisplit`. Managed by running `powershell.exe -NoProfile -NonInteractive` with
   fixed cmdlets; names come only from the built-in profiles and must match
   `[a-z0-9.-]+`. Start: remove stale snisplit rules, bind every listener and the DNS
   socket, then add the rules and flush the DNS cache. Any bind failure stops before a
   rule exists. Stop: remove the rules, flush, then close the listeners.
9. **Relay profile type in core.** `first_flight::RelayProfile`, separate from the
   parked `engine::Profile` (its reversed HTTP order means nothing in a relay).
10. **tokio** per PLAN.md D11, plus the `sync` feature (measured: no additional
    crate). No `macros`: runtimes are built with `tokio::runtime::Builder`, tests too;
    shutdown uses `JoinSet::abort_all`, `tokio::time::timeout` and
    `tokio::io::copy_bidirectional`.
11. **Testable without admin.** Listen address, ports and upstream port are injected
    (`Config`), so tests bind `127.0.0.1:0` on every OS. The cut is tested on a
    recording writer, not on a socket (loopback may merge writes).
12. **Limits.** First flight: at most 16 KiB and 10 s, then the connection is
    closed and logged. Upstream connect timeout 10 s.

## Contract

`crates/core` (no I/O):

```rust
pub mod dns {
    pub const TYPE_A: u16 = 1;
    pub const TYPE_AAAA: u16 = 28;
    pub const TYPE_HTTPS: u16 = 65;

    pub struct Question { pub id: u16, pub name: String, pub qtype: u16, pub qclass: u16 }

    pub fn parse_query(packet: &[u8]) -> Option<Question>;
    pub fn answer_a(query: &[u8], address: std::net::Ipv4Addr, ttl: u32) -> Option<Vec<u8>>;
    pub fn answer_empty(query: &[u8]) -> Option<Vec<u8>>;
    pub fn build_a_query(id: u16, name: &str) -> Option<Vec<u8>>;
    pub fn parse_a_answer(packet: &[u8], id: u16, name: &str) -> Option<Vec<std::net::Ipv4Addr>>;
}

pub mod first_flight {
    pub enum Protocol { Tls, Http }

    pub struct RelayProfile {
        pub name: String,
        pub domains: crate::domains::DomainList,
        pub tls_positions: Vec<crate::strategy::SplitPosition>,
        pub tls_ports: Vec<u16>,
        pub http_ports: Vec<u16>,
    }

    pub enum Decision {
        NeedMoreData,
        Forward { host: String, cuts: Vec<usize> },
        Refuse(Refusal),
    }

    pub enum Refusal { NoHost, NotListed, TooLong }

    pub fn decide(profile: &RelayProfile, protocol: Protocol, first_bytes: &[u8], max_len: usize) -> Decision;
}
```

- `parse_query`: a standard query (QR 0, opcode 0) with exactly one question; name
  lowercased, no trailing dot. `None` for anything else.
- `answer_a` / `answer_empty`: a response with the query's id, RD copied, QR 1, RA 1,
  RCODE 0, the question echoed, additional records (EDNS) dropped; `None` if the query
  does not parse.
- `parse_a_answer`: checks id, QR 1, RCODE 0 and that the question is `name`; follows
  CNAMEs from `name`; handles name compression; returns the A addresses in order;
  `None` on any malformed input.
- `decide`: TLS uses `tls::inspect_client_hello`, HTTP `http::inspect_request`; host
  lowercased, HTTP port suffix removed; cuts are stream offsets from
  `Strategy::stream_offsets`, strictly inside `1..first_bytes.len()`; HTTP cuts are
  empty in milestone 3. `TooLong` when more data is needed but `first_bytes.len() >=
  max_len`.

`crates/relay` (tokio):

```rust
pub trait Resolve: Send + Sync + 'static {
    fn resolve(&self, host: &str) -> impl Future<Output = io::Result<Vec<Ipv4Addr>>> + Send;
}

pub struct Listener { pub listen: SocketAddr, pub upstream_port: u16, pub protocol: Protocol, pub profile: usize }

pub struct Config {
    pub listeners: Vec<Listener>,
    pub profiles: Vec<RelayProfile>,
    pub piece_gap: Duration,
    pub first_flight_timeout: Duration,
    pub max_first_flight: usize,
    pub connect_timeout: Duration,
}

pub enum Event {
    Forwarded { host: String, port: u16, pieces: usize },
    Refused { port: u16, refusal: Refusal },
    Failed { host: String, port: u16, error: String },
}

pub struct Relay;
impl Relay {
    pub async fn bind<R: Resolve>(config: Config, resolver: R, on_event: Arc<dyn Fn(Event) + Send + Sync>) -> io::Result<Relay>;
    pub fn local_addrs(&self) -> Vec<SocketAddr>;
    pub async fn run(self, shutdown: impl Future<Output = ()> + Send);
}

pub async fn write_pieces<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8], cuts: &[usize], gap: Duration) -> io::Result<usize>;

pub mod dns_server {
    pub struct DnsServer;
    impl DnsServer {
        pub async fn bind(listen: SocketAddr, profiles: Vec<RelayProfile>, answer: Ipv4Addr, upstreams: Vec<SocketAddr>) -> io::Result<DnsServer>;
        pub fn local_addr(&self) -> SocketAddr;
        pub async fn run(self, shutdown: impl Future<Output = ()> + Send);
    }
}

pub mod upstream {
    pub struct UdpResolver;
    impl UdpResolver { pub fn new(servers: Vec<SocketAddr>, timeout: Duration) -> Self; }
    impl Resolve for UdpResolver {}
}
```

`write_pieces` writes `bytes[..cuts[0]]`, flushes, waits `gap`, and so on; returns
the number of pieces written. `Relay::bind` binds every listener before returning, so
a bind failure returns `Err` and nothing is left listening.

## Streams and file ownership

| Phase | Stream | Owner files | Depends on |
|---|---|---|---|
| 1 | A DNS codec | `crates/core/src/dns.rs` (+`dns/tests.rs`), `crates/core/tests/dns_packets.rs`, `crates/core/tests/fixtures/dns/` | contract |
| 1 | B first flight + TCP relay | `crates/core/src/first_flight.rs` (+tests), `crates/relay/**` except `dns_server.rs`, `upstream.rs` | contract |
| 1 | C Windows routing | `crates/cli/src/routing.rs` (+`routing/tests.rs`) | contract |
| 2 | D DNS server + upstream | `crates/relay/src/dns_server.rs`, `crates/relay/src/upstream.rs` (+tests) | A, B |
| 3 | cli wiring | tech lead: `crates/cli/**` except `routing.rs` | B, C, D |
| 4 | Acceptance | tech lead with the owner | all |

The contract commit adds the empty `dns` and `first_flight` modules, the `relay`
crate skeleton and the workspace `tokio` dependency, so no two streams edit the same
file.

## Blast radius

| Symbol changed | Callers | What breaks |
|---|---|---|
| `snisplit_core` module list (`lib.rs`) | none outside new code | nothing; additive |
| workspace `Cargo.toml` members and dependencies | every crate builds against it | a bad tokio entry fails every build; checked by the gates on the contract commit |
| `cli::profile` (rewritten to `RelayProfile` in phase 3) | `cli::main` only | the parked engine is no longer driven by `run`; `engine` tests unaffected |
| `cli::platform::divert` and `DIVERT_FILTER` (removed in phase 3) | `cli::main` only | `run` stops reporting "unsupported"; the cli no longer depends on `backend-windows` (the crate stays in the workspace, parked) |

## Risks

| Risk | Check |
|---|---|
| A listed name used on a port the relay does not listen on breaks | decision 2; acceptance watches the Steam client log in and the relay log for refused or unknown ports |
| Kaspersky reacts to an elevated process running PowerShell or to the local DNS server | acceptance; not worked around |
| Stale NRPT rules after a crash break listed names | start-time cleanup and `snisplit cleanup`; full crash safety is milestone 5 |
| The relay connects to itself through a loopback upstream answer | decision 6, a test with a resolver returning 127.0.0.2 |
| zapret acts on the relay's outbound packets during Steam acceptance | Steam acceptance only with zapret stopped |
| snisplit started while Steam or Chrome is already open keeps DoH | acceptance item: start snisplit with Chrome open |
| macOS has no 127.0.0.2 by default; low ports need root on Linux | tests bind 127.0.0.1:0; Linux and macOS routing is milestone 10 |

## Dropped

- TCP DNS: answers fit in UDP.
- The `hoSt:` rewrite: milestone 4.
- Per-name loopback addresses (to relay protocols without a host): no such protocol
  to a listed name was observed.
- Windows service, crash-safe cleanup: milestone 5.
- Reading DNS servers with Win32 FFI: PowerShell is already needed for NRPT.

## Acceptance results

### Done-when 1 (no admin), 2026-11-14

`snisplit run --no-routing --profile medium --verbose`, then curl with
`--resolve <host>:443:127.0.0.3`: medium.com 403 (Cloudflare's answer to curl; the
handshake passed, direct is reset), www.medium.com 403, help.medium.com 302, each
`relayed ... in 2 pieces`. The DNS server answered `medium.com` with 127.0.0.3 and
forwarded `example.org`. Port 80 relayed unchanged and stays blocked (milestone 4).
Ctrl+C through `GenerateConsoleCtrlEvent` stops with exit code 0. Without admin,
`snisplit run --profile medium` fails with a one-line routing error and leaves no
rule.

### Elevated run, `tools/acceptance/m3.ps1`, 2026-11-14 17:54-18:01

Part 1, medium.com with NRPT, zapret running: PASS.

- Ready after 14.4 s (rule installation through PowerShell). Rules
  `medium.com->127.0.0.3` and `.medium.com->127.0.0.3`.
- System resolver: medium.com and www.medium.com -> 127.0.0.3 (first answer 59 ms
  after ready); example.org -> its real addresses.
- curl https://medium.com/ 403 via 127.0.0.3; help.medium.com 302.
- Headless Chrome without resolver flags: two `relayed medium.com:443` lines.
- Ctrl+C: exit 0, `snisplit DNS rules left: 0`; independent check by name server:
  0 rules pointing at 127.0.0.2/3; medium.com resolves to 162.159.152.4,
  162.159.153.4 again.

Part 2, Steam, zapret stopped: PASS for HTTPS URLs and Chrome; Steam client not
covered.

- Control without snisplit: store.steampowered.com fails (000).
- Ready after 21.2 s, 30 rules. store.steampowered.com and
  cmp1-hkg1.steamserver.net -> 127.0.0.2.
- curl: store 200, steamcommunity 200, api GetCMListForConnect 200, login 302, all
  via 127.0.0.2. `http://store.steampowered.com/` relayed unchanged and blocked, as
  planned for milestone 3.
- The owner's Chrome, open since before snisplit started, reloaded the store: its
  lookups went through the system resolver (DNS client log: store.steampowered.com,
  steamcommunity.com, store/shared/cdn `*.steamstatic.com`) and the relay logged 39
  relayed connections, 0 refused, 0 failed. This covers the "snisplit started while
  Chrome is open" risk for Chrome.
- The Steam client was not restarted within the 300 s window, so login and the
  client's Store page are not covered yet.
- After: 0 rules left, zapret Running with winws, store 200 through zapret.

Findings:

- Start-up takes 14-21 s before rules exist; most of it is PowerShell. Acceptable
  for a foreground run; milestone 5 (service) should batch the rule installation.
- Headless Chrome `--dump-dom` prints nothing on Windows (GUI subsystem); the relay
  log is the evidence instead.

### Steam client rerun, `m3.ps1 -SteamClientOnly`, 2026-11-14 18:05-18:10

Zapret stopped, control without snisplit fails, ready after 12.7 s, the same four
URLs 200/200/200/302 via 127.0.0.2. The owner exited and restarted Steam at 18:08:00,
logged in and opened the Store: PASS.

- Login: Steam's `connection_log.txt` shows `ConnectionCompleted() (127.0.0.2:27018,
  WebSocket)` followed by `RecvMsgClientLogOnResponse() ... 'OK'` at 18:08:06. The
  CM connection went through the relay; the relay logged `cmp3-hkg1.steamserver.net:27018`,
  `cmp2-sgp1...:27019`, `cmp2-tyo3...:27020` and `cmp2-*:443`.
- Store in the client: steamwebhelper made 28 system lookups of Steam web names
  (DNS client log) and the relay logged 132 relayed connections in the run (store,
  community, api, login, the `*.steamstatic.com` CDNs, `cache*-hkg1.steamcontent.com`).
  13 of Steam's 17 established connections at the end went to 127.0.0.2 (ports 443
  and 27018).
- One failure: `failed ipv6check-http.steamserver.net:80: no address`. That name has
  no A record; snisplit answers its A query with 127.0.0.2 and its AAAA query empty,
  so Steam's IPv6 connectivity check fails (`TIMEOUT`) and the UDP check finds no
  address. Steam then uses IPv4, which it does anyway on this PC. Recorded against
  decision 1 as a known exception: listed names that exist only over IPv6.
- After: 0 rules (by comment and by name server), zapret Running with winws, store
  200 through zapret.

### Done-when status

| # | Item | Result |
|---|---|---|
| 1 | No admin, medium via curl | PASS |
| 2 | Elevated, medium through NRPT in curl and Chrome; example.org unchanged | PASS |
| 3 | Elevated, zapret stopped: 4 Steam URLs in curl and Chrome; Steam client login and Store; steamwebhelper on the system resolver | PASS |
| 4 | Ctrl+C removes every rule and flushes; names resolve normally again; `cleanup` | PASS for Ctrl+C (three runs). `cleanup` was exercised only with no rules present (it ran inside `remove_rules`); a crash with rules left is milestone 5 |

Open items carried forward: HTTP on port 80 (milestone 4, `hoSt:`), IPv6-only
listed names, start-up time (milestone 5), Ctrl+C before the handler is registered
leaves rules (milestone 5).
