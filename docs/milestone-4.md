# Milestone 4 plan: HTTP through the relay, profile files, IPv6-only names

Done when (PLAN.md section 4, made concrete):

1. Without admin, `snisplit run --no-routing --profile medium` relays
   `curl --resolve medium.com:80:127.0.0.3 http://medium.com/ http://medium.com/`
   (two requests on one connection) and both get the server's 301; the same two
   requests sent directly, without the relay, are reset.
2. Without admin, a profile file given with `--profile-file` is relayed on its own
   loopback address (127.0.0.4 for the first file).
3. Elevated, with `zapret-steam` stopped (owner asked first), `snisplit run`:
   - `http://store.steampowered.com/` returns the server's 302 and a Steam content
     server returns 200 over HTTP, through normal name resolution;
   - two requests on one HTTP connection to a Steam host both pass;
   - a listed name with no A record (`ipv6check-http.steamserver.net`) resolves to its
     real AAAA address, as without snisplit;
   - `example.org` resolves exactly as without snisplit.
4. The relay log of a Steam client download, if the owner starts one in the elevated
   run, says which ports the download used (open since milestone 2, Q3).

## Measured before planning (2026-11-14, no admin, zapret running)

`keepalive.py` (scratch script, not kept): one TCP connection to medium.com:80,
request 1 with `hoSt: medium.com`, then request 2 on the same connection. The server
answers `Connection: keep-alive`.

| Request 2 header | Round 1 | Round 2 |
|---|---|---|
| `Host: medium.com` | reset | reset |
| `hoSt: medium.com` | 301 | 301 |

The ISP inspects every request on a connection, not only the first. Rewriting the
first flight alone would leave every keep-alive request after the first blocked, and
Steam downloads send many requests per connection.

## Rules for every contributor

Same as docs/milestone-3.md: no comments of any kind, ASCII only, gates
(`cargo fmt --all --check`; `cargo clippy --workspace --all-targets` with
`RUSTFLAGS=-D warnings`, for the host and for `--target x86_64-unknown-linux-gnu`;
`cargo test --workspace`) before every commit, unit tests over about 150 lines in
`<module>/tests.rs`, public-API tests in `crates/<crate>/tests/`. No new crates (no
serde, no toml). Never push, never elevate, never add or remove NRPT rules, never
touch the zapret service or bundle, never work around the antivirus. The owner's test
rule: every assertion compares against an expected value independent of the code
under test; no tests of test helpers; no assertions on what the setup just wrote; for
every test, name a plausible bug that makes it fail.

## Decisions

1. **Every HTTP request is respelled.** On an HTTP listener whose profile says
   `host-spell`, the header name of every `Host` header line in every request head
   the client sends is replaced by exactly `hoSt` (measured above and in milestone 2;
   `host:` fails). Same length, in place. The request line, other headers, bodies and
   everything the server sends are unchanged.
2. **Framing.** A request head ends at the first empty line (LF with an optional CR).
   After the head:
   - `Content-Length` with one decimal value (repeats allowed if equal): that many
     body bytes pass unchanged, then the next head starts;
   - no `Content-Length` and no `Transfer-Encoding`: no body;
   - `Transfer-Encoding` present, `Content-Length` values that differ or do not
     parse, an `Upgrade` header, the `CONNECT` method, a head over 16 KiB, or a
     request line that is not `<method> <target> HTTP/1.x`: rewriting stops and the
     rest of the connection passes unchanged. The reason is reported when the
     connection closes.
   Stopping is never worse than no snisplit (the governing rule of milestone 3): the
   bytes still reach the server unchanged.
3. **Held bytes.** An incomplete head is held until it is complete (at most 16 KiB),
   because a `Host` line may span reads. If the client closes with a head still held,
   the held bytes are sent unchanged. Body bytes are sent as they arrive.
4. **The first flight goes through the same respeller.** HTTP first flights still have
   no cuts; `decide` keeps choosing the host and the profile check.
5. **Profile field.** `RelayProfile` gets `http_method: HttpMethod` with
   `PassThrough` and `HostSpell`. Steam and medium use `HostSpell`.
6. **Close event.** The relay emits `Event::Closed { host, port, to_server,
   to_client, respelled, stopped }` when a forwarded connection ends (`respelled`:
   number of heads rewritten; `stopped`: why rewriting stopped, if it did). The cli
   prints it only with `--verbose`. This closes the milestone 3 finding that
   `relayed` only meant the first flight was forwarded.
7. **IPv6-only listed names: treated as unlisted.** For a listed name, the DNS server
   first asks the relay's resolver for A records (the same cached resolver the relay
   uses, so the connection that follows needs no second lookup). Addresses found:
   answers as in milestone 3. No A record, or the lookup fails: the query is forwarded
   upstream unchanged, so the app gets what it would get without snisplit. No IPv6
   upstream support in this milestone.
8. **Shared resolver.** `impl<R: Resolve> Resolve for Arc<R>`, so one `UdpResolver`
   is shared by the relay and every DNS server. `DnsServer::bind` gains a resolver
   argument.
9. **Profile files, hand-parsed.** One profile per file, UTF-8, one `key value...`
   line per setting; blank lines and lines starting with `#` are ignored (these are
   user files, not source). Keys:

   | Key | Value | Default |
   |---|---|---|
   | `name` | `[a-z0-9-]+`, 1-32 chars, not a built-in name | required |
   | `domains` | one or more names, space-separated; the line may repeat | required |
   | `tls-ports` | ports and ranges, `443 27015-27030` | `443` |
   | `tls-split` | positions: a byte offset `1`, `host-start`, `domain-mid` | `1` |
   | `http-ports` | ports and ranges | `80` |
   | `http-method` | `host-spell` or `pass` | `host-spell` |

   Any other key, a repeated non-repeatable key, or a bad value is an error naming
   the file and line. Domain names: lowercased, then each must be labels of
   `[a-z0-9-]` (1-63 chars, no leading or trailing hyphen) joined by dots, at least two
   labels, at most 253 chars. This is checked at parse time; `routing.rs`'s own check
   stays as the last gate before PowerShell. Ports 1-65535, not 53, and no port in both
   lists. `tls-ports none` or `http-ports none` disables a protocol; both disabled is
   an error.
10. **Built-ins in the same format.** `crates/cli/profiles/steam.profile` and
    `medium.profile`, embedded with `include_str!` and parsed by the same parser at
    start. `snisplit show-profile <name>` prints a built-in's text, as a template.
11. **Selection and addresses.** `--profile <built-in>` and `--profile-file <path>`
    are both repeatable. If neither is given, every built-in runs (as now). Built-ins
    keep their addresses by their position in the built-in list (steam 127.0.0.2,
    medium 127.0.0.3); files get 127.0.0.4, 127.0.0.5, ... in command-line order.
    The same domain in two selected profiles is an error; two files with the same
    `name` are an error.

## Contract

`crates/core`:

```rust
pub mod first_flight {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum HttpMethod { PassThrough, HostSpell }

    pub struct RelayProfile {
        pub name: String,
        pub domains: DomainList,
        pub tls_positions: Vec<SplitPosition>,
        pub tls_ports: Vec<u16>,
        pub http_ports: Vec<u16>,
        pub http_method: HttpMethod,
    }
}

pub mod http_stream {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum StopReason { TransferEncoding, ContentLength, Upgrade, Connect, HeadTooLong, NotHttp }

    pub const MAX_HEAD: usize = 16 * 1024;

    pub struct HostRespeller;
    impl HostRespeller {
        pub fn new() -> Self;
        pub fn feed(&mut self, input: &[u8], output: &mut Vec<u8>);
        pub fn finish(&mut self, output: &mut Vec<u8>);
        pub fn respelled(&self) -> usize;
        pub fn stopped(&self) -> Option<StopReason>;
    }
}
```

- `feed` appends to `output` every byte that may be sent now and holds the rest
  (decision 3). Concatenating all outputs and the `finish` output gives the input with
  only the `Host` names changed, whatever the read boundaries are.
- `finish` appends any held bytes unchanged.

`crates/relay`:

```rust
pub enum Event {
    Forwarded { host: String, port: u16, pieces: usize },
    Refused { port: u16, refusal: Refusal },
    Failed { host: String, port: u16, error: String },
    Closed { host: String, port: u16, to_server: u64, to_client: u64, respelled: usize, stopped: Option<StopReason> },
}

impl<R: Resolve> Resolve for Arc<R> {}

impl DnsServer {
    pub async fn bind<R: Resolve>(listen: SocketAddr, profiles: Vec<RelayProfile>, answer: Ipv4Addr,
        upstreams: Vec<SocketAddr>, resolver: R) -> io::Result<DnsServer>;
}
```

The contract commit adds these types, an `http_stream` module whose `feed` passes
bytes through unchanged, `DnsServer::bind` taking and ignoring the resolver, and
`http_method: HostSpell` in the built-ins, so the streams below edit disjoint files.

## Streams and file ownership

| Stream | Model, effort | Owner files | Done-check |
|---|---|---|---|
| A HTTP respelling | opus, high | `crates/core/src/http_stream.rs` (+`http_stream/tests.rs`), `crates/core/tests/http_stream.rs`, `crates/relay/src/connection.rs` (+tests), `crates/relay/src/event.rs`, `crates/relay/tests/` for HTTP | gates; done-when 1 with a local test server; keep-alive, split-read and body tests |
| B DNS fallback | sonnet, medium | `crates/relay/src/dns_server.rs` (+tests), `crates/relay/src/resolve.rs` (+tests) | gates; tests for listed with A, listed without A, lookup failure |
| C Profile files | sonnet, high | `crates/cli/src/profile_file.rs` (+tests), `crates/cli/profiles/`, `crates/cli/src/profile.rs`, `plan.rs`, `args.rs` (+their tests), `crates/cli/src/main.rs` except `describe_event` | gates; hostile-name tests; built-in parse equals the milestone 3 values |
| D cli event text, acceptance | tech lead | `describe_event` in `main.rs` after A and C land; `tools/acceptance/m4.ps1` | done-when 1-4 |

## Blast radius

| Symbol changed | Callers | What breaks |
|---|---|---|
| `RelayProfile` (new field `http_method`) | `cli::profile::{steam, medium}`; tests in `core/src/first_flight/tests.rs`, `relay/src/connection/tests.rs`, `relay/src/relay/tests.rs`, `relay/src/dns_server/tests.rs`, `relay/tests/` | every struct literal fails to compile until the field is added; the contract commit adds it everywhere |
| `Event` (new variant `Closed`) | `cli::main::{describe_event, event_printer}`; relay tests matching events | non-exhaustive matches fail to compile; tests that collect all events and compare whole lists must expect `Closed` |
| `DnsServer::bind` (new resolver argument) | `cli::main::serve`; `relay/src/dns_server/tests.rs`; `relay/tests/` | compile errors; fixed in the contract commit |
| `cli::profile::BuiltIn`, `plan::Selected`, `RunOptions.profiles` (from `&'static` to owned profiles) | `args.rs`, `plan.rs`, `main.rs` and their tests | stream C owns all of them |
| `connection::serve` copy loop (HTTP no longer `copy_bidirectional`) | every relayed HTTP connection | half-close or byte loss on HTTP; tests: client half-close, server-first close, large body |

## Risks

| Risk | Check |
|---|---|
| A framing mistake corrupts an HTTP stream (bytes lost, reordered or a body byte rewritten) | property test: random read boundaries give the same output as one read; `Host:` inside a body stays unchanged; pipelined requests |
| A profile file injects into the elevated PowerShell script | parse-time name check plus the existing `routing.rs` check; tests with quotes, `;`, `$(...)`, backticks, spaces, newlines, non-ASCII, `..` |
| Kaspersky (`avp`) proxies port 80 and changes what the relay writes | acceptance compares status codes and the relay's `respelled` count; not worked around |
| DNS answers for listed names get slower (an upstream lookup first) | the lookup is cached 30 s and shared with the relay; measure with `Resolve-DnsName` timing in acceptance |
| A server that rejects `hoSt` | servers must accept any header-name case (RFC 9110 5.1); Steam store, a content server and medium accepted it in milestone 2 |
| Steam client downloads over HTTP need more than respelling | done-when 4 observes them; milestone 5 owns a full download |

## Dropped

- HTTP splitting: untestable on this PC (the antivirus merges port-80 writes), and
  respelling passes.
- Chunked request bodies: rewriting stops and the rest passes unchanged (decision 2);
  GET downloads have no body.
- IPv6 upstream and an IPv6 relay address: decision 7 keeps IPv6-only names working
  without them.
- A config directory or default profile-file location: `--profile-file` is enough
  until the service (milestone 5) needs one.
- Start-up time, Ctrl+C during start-up, batched rule install: milestone 5.
