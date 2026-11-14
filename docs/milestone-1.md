# Milestone 1 plan: core parsers, matcher, segment planner

Done when (PLAN.md section 4): unit tests pass on captured real ClientHellos (curl,
Chrome, Firefox, Steam client), and mutating the parsers' input finds no panic.

Everything in this milestone lives in `crates/core` except the capture tool. No new
crates without the tech lead's approval. Profiles and TOML stay in milestone 3.

## Rules for every contributor

- No code comments of any kind (line, block, doc). Names, types and small functions
  carry the meaning.
- ASCII only in source.
- Unit tests live in `#[cfg(test)] mod tests`. Inline when short; once a test module
  passes about 150 lines it moves to `<module>/tests.rs` (declared as `mod tests;`).
  Tests of the public API only go in `crates/<crate>/tests/`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` with
  `RUSTFLAGS=-D warnings`, and `cargo test --workspace` pass before every commit.
- Never push, never run anything elevated, never touch the zapret service or bundle.

## Decisions

1. **Shared result type.** `inspect::Inspection { Host(Range<usize>), NeedMoreData, NoHost }`.
   `NeedMoreData` means "could still contain a host if more stream bytes arrive";
   `NoHost` means "definitely not, stop looking".
2. **Split positions.** Internally: `HostStart`, `DomainMid`, `Offset(n)`. `DomainMid`
   is the middle of the first label of the *matched listed domain* (for host
   `a.store.steampowered.com` and listed `steampowered.com`, the middle of
   `steampowered`). This is zapret's `midsld` for second-level entries, which is
   what was measured. The plan's `sni-mid`/`host-mid` map to `DomainMid`,
   `sni-start` maps to `HostStart`; profile names are settled in milestone 3.
3. **Matcher returns where the match is.** `DomainList::matched_domain(host)` returns
   the byte range of the matched listed domain inside `host`, so `DomainMid` can be
   computed. Matching is case-insensitive, at label boundaries only
   (`notsteampowered.com` does not match `steampowered.com`), and ignores one
   trailing dot.
4. **HTTP.** Every outbound packet that starts with an HTTP request line is
   inspected, not just the first on a connection (Steam downloads use keep-alive).
   Only the `Host` header counts (not `X-Forwarded-Host`), matched
   case-insensitively; the returned range excludes `:port`.
5. **TLS.** Only the first ClientHello of a connection. A ClientHello may span
   several TCP packets; the host name can be wholly in packet 1, wholly in packet 2,
   or straddle them.
6. **Flow state** (phase 2) is keyed by the TCP 4-tuple, records the sequence number
   of the first payload byte, copies payload into a bounded per-flow buffer at
   `seq - first_seq`, and is bounded in entries and bytes. Time is passed in, never
   read. Entries live until an idle timeout, not "until the first request is
   handled", so a retransmitted packet produces the same split again.
7. **Planner splits one packet at a time.** Offsets are relative to that packet's
   payload; the flow layer translates stream offsets. Offsets that fall in an
   earlier, already-sent packet are dropped.
8. **Planner output.** Sequence numbers with wrapping arithmetic; PSH and FIN only on
   the segment that ends the original payload; other flags copied; distinct IPv4
   IDs; IP and TCP options preserved; IPv4 header checksum, IPv6 payload length and
   TCP checksum recomputed. `Reversed` order emits the same segments last-first.
9. **Checksum tests use independent vectors**, not values the code computed and not
   outbound captures (offload may leave checksums unfilled).
10. **Fuzzing on stable.** Mutation tests over real captures and builders (truncate,
    flip bytes, corrupt length fields) run in `cargo test`. Coverage-guided
    `cargo-fuzz` is dropped for now (needs nightly; awkward on Windows).
11. **Captures** store TCP payload bytes only (no IP headers, no addresses), plus
    per-packet lengths, and only for allowlisted hosts. Committing them to the public
    repo needs the user's approval.

## Contract

```rust
// inspect.rs (tech lead, done)
pub enum Inspection { Host(Range<usize>), NeedMoreData, NoHost }

// tls.rs (stream B)
pub fn inspect_client_hello(stream_start: &[u8]) -> Inspection;

// http.rs (stream B)
pub fn inspect_request(request_start: &[u8]) -> Inspection;

// domains.rs (stream B)
pub struct DomainList;
impl DomainList {
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(domains: I) -> Self;
    pub fn matched_domain(&self, host: &[u8]) -> Option<Range<usize>>;
}

// segment.rs (stream C)
pub enum Order { InSequence, Reversed }
pub fn split(packet: &[u8], payload_offsets: &[usize], order: Order) -> Option<Vec<Vec<u8>>>;
```

`split` returns `None` if the packet is not TCP over IPv4/IPv6, no offset falls
strictly inside the payload, or splitting would change what the receiver sees: SYN,
URG or RST set, an IPv4 fragment, or IPv6 extension headers. Offsets outside
`(0, payload_len)` are ignored. Callers must treat `None` as "pass unchanged".

Known limit: split segments take IPv4 IDs `original + index`, which the OS may reuse
for its next packets. Harmless with DF set (RFC 6864), which is the norm for TCP.

## Streams and file ownership

| Stream | Owner files | Depends on |
|---|---|---|
| A capture | `crates/backend-windows/examples/capture.rs` | nothing |
| B parsers | `core/src/tls.rs`, `http.rs`, `domains.rs`, `reader.rs` | contract |
| C planner | `core/src/segment.rs`, `checksum.rs`, `packet.rs` | contract |
| D flow + strategy + engine (phase 2) | `core/src/flow.rs`, `strategy.rs`, `engine.rs` | B, C |
| E mutation tests (phase 2) | `core/tests/`, fixtures | B, captures |

`core/src/lib.rs` is edited only by the tech lead (B, C) or by D (phase 2).

## Tests that must exist

| Risk | Test |
|---|---|
| Host name in packet 1, in packet 2, straddling | B: `inspect_client_hello` on each prefix of a two-record-sized hello; D: flow with each split |
| Label boundary | B: `notsteampowered.com` vs `steampowered.com` |
| `X-Forwarded-Host`, `Host` with port, header case | B |
| Wrong checksums | C: RFC 1071 and published IPv4 header vectors; TCP checksum of a split packet verified by an independent slow reference in the test |
| Sequence wraparound | C: packet with `seq` near `u32::MAX` |
| PSH/FIN placement, IPv4 IDs, options kept | C |
| Retransmission gives the same output | D |
| Flow table bounds | D |
| Parsers never panic on mutated real input | E |

## Dropped

- Coverage-guided fuzzing (decision 10).
- Profiles/TOML (milestone 3).
- Holding packet 1 of a two-packet ClientHello until the host is known (decision 7):
  the split happens in the packet that contains the chosen offset.
