# Milestone 2 plan: Windows engine, foreground

Done when (PLAN.md section 4): with the `zapret-steam` service stopped, `snisplit run`
in an elevated console makes the 4 HTTPS Steam URLs return 200/302 from curl and from
Chrome. The URLs are the ones in section 1:
`https://store.steampowered.com/`, `https://steamcommunity.com/`,
`https://api.steampowered.com/ISteamDirectory/GetCMListForConnect/v1/?cellid=0&maxcount=3`,
`https://login.steampowered.com/`.

## Rules for every contributor

Same as docs/milestone-1.md (no comments, ASCII only, fmt/clippy/test gates, test
layout, never push, never elevate, never touch the zapret service or bundle), plus the
owner's test rule: every assertion compares against an expected value independent of
the code under test; no tests of test helpers; no assertions on what the setup just
wrote; for every test, name a plausible bug that makes it fail.

## Decisions

1. **Packet loop lives in core, generic over I/O.** `pump::PacketIo` abstracts receive
   and send; `pump::run` drives `Engine::process` and is unit-tested with a fake. The
   Windows backend only implements the trait.
2. **Every diverted packet is re-sent.** Pass means "send the original back unchanged
   with its original address"; Replace means "send each replacement with the original
   address". A send failure is counted and logged, never fatal: dropping one packet is
   recoverable by TCP, stopping the loop is not.
3. **Filter (wide, milestone 1 decision):**
   `outbound and !loopback and !impostor and tcp.PayloadLength > 0 and
   (tcp.DstPort == 443 or tcp.DstPort == 80)`. `!impostor` keeps re-sent packets from
   being diverted again.
4. **Built-in Steam profile until milestone 3.** Domains from PLAN.md references
   (`list-steam.txt`). TLS: `[Offset(1), DomainMid]` in sequence, the measured working
   case B. HTTP: same positions, reversed, the measured case C (verified in milestone 3).
5. **No new crates.** Ctrl+C handling is hand-written FFI to `SetConsoleCtrlHandler`
   in `backend-windows`. Argument parsing stays `std::env::args` until milestone 3.
6. **Official WinDivert only.** Development uses the official 2.2.2 release placed by
   the owner in `vendor/windivert/` (gitignored); nothing is copied from the zapret
   bundle. The loaded driver is verified (`sc qc WinDivert` path, signature) during the
   acceptance run.
7. **Foreground only.** Ctrl+C shuts the handle down and the loop exits cleanly,
   printing counts. Service mode is milestone 4.

## Contract

```rust
pub trait PacketIo {
    type Address;
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(usize, Self::Address)>>;
    fn send(&mut self, packet: &[u8], address: &Self::Address) -> io::Result<()>;
}
```

`recv` returns `Ok(None)` once the source has been shut down; `run` then returns.

## Streams and file ownership

| Stream | Owner files | Depends on |
|---|---|---|
| W Windows backend | `crates/backend-windows/**` | contract |
| L loop + cli | `crates/core/src/pump.rs` (+ `pump/tests.rs`), `crates/cli/**` | contract |
| Acceptance run | tech lead with the owner (elevated, zapret stopped) | W, L |

## Risks

| Risk | Check |
|---|---|
| A packet is lost or duplicated by the loop | L: fake I/O tests count every received packet re-sent exactly once (Pass) or as its pieces (Replace) |
| Re-sent packets are diverted again (loop) | W: `!impostor` in the filter; acceptance run counters stay bounded |
| Checksum flags in the address make WinDivert or the NIC alter our segments | W: research WinDivertSend semantics for the IPChecksum/TCPChecksum flags and set them deliberately |
| zapret's loaded driver is used instead of the official one | Acceptance run: inspect the driver service binary path |
| Kaspersky blocks the official driver or snisplit.exe | Owner adds an exclusion; documented for milestone 7 |
