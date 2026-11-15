# snisplit architecture for new developers

This page explains how the code fits together, with diagrams. It describes the
driverless design (PLAN.md, decision D1) as it stands at version 0.2.0. For why each
choice was made, read PLAN.md and the milestone records in `docs/`.

The diagrams are Mermaid; GitHub and most editors render them.

## 1. The idea in one picture

snisplit never decrypts anything. It only changes how the first bytes of a connection
are cut into TCP segments, so the provider's filter cannot read the server name in one
piece.

```mermaid
flowchart LR
    App["App<br/>(browser, Steam)"]
    OS["Windows DNS client<br/>+ NRPT rules"]
    DNS["snisplit DnsServer<br/>127.0.0.N:53"]
    Relay["snisplit Relay<br/>127.0.0.N:443 / :80 / ..."]
    Up["Upstream DNS<br/>(adapter's servers)"]
    Real["Real server"]

    App -- "1. resolve store.steampowered.com" --> OS
    OS -- "2. listed suffix: ask 127.0.0.N" --> DNS
    OS -. "other names: unchanged" .-> Up
    DNS -- "3. real address exists?" --> Up
    DNS -- "4. A = 127.0.0.N, TTL 5 s" --> OS
    App -- "5. TCP to 127.0.0.N:443" --> Relay
    Relay -- "6. real address" --> Up
    Relay -- "7. ClientHello in 2-3 pieces,<br/>then bytes both ways" --> Real
```

Each selected profile (site) gets its own loopback address, starting at `127.0.0.2`
(`crates/cli/src/plan.rs`). The NRPT rule for a profile's domains names that address as
the DNS server, the DNS server on that address answers with that same address, and the
relay listens on that address for the profile's ports. So the address alone tells the
relay which profile a connection belongs to.

## 2. Crates

```mermaid
flowchart TD
    gui["crates/gui<br/>snisplit-gui.exe<br/>(eframe / egui)"]
    cli["crates/cli<br/>snisplit.exe + lib 'snisplit'"]
    relay["crates/relay<br/>snisplit-relay<br/>(tokio)"]
    core["crates/core<br/>snisplit-core<br/>(no I/O, no unsafe)"]
    bw["crates/backend-windows<br/>WinDivert FFI<br/>(parked)"]

    gui --> cli
    gui --> relay
    gui --> core
    cli --> relay
    cli --> core
    relay --> core
    bw -.-> core
```

| Crate | Job | Main entry points |
|---|---|---|
| `core` | Pure byte logic: parse a ClientHello or HTTP request, find the host, match it against a domain list, compute cut offsets, DNS codec, `hoSt:` respelling, probe assessment. No sockets, so everything is unit-tested with fixtures. | `first_flight::decide`, `DomainList`, `Strategy`, `HostRespeller`, `dns`, `probe` |
| `relay` | The async network parts on tokio: the TCP relay, the DNS server, the upstream resolver, the probe. Knows nothing about files, Windows or profiles on disk. | `Relay`, `DnsServer`, `UdpResolver`, `Upstreams`, `probe::probe` |
| `cli` | Library and binary. Reads profiles and the site folder, picks loopback addresses, adds and removes NRPT rules through PowerShell, holds the single-instance lock, runs a session. | `session::serve`, `plan`, `routing`, `sites`, `instance` |
| `gui` | The window. A pure state machine (`state`) plus a driver that turns effects into calls on the `cli` library. | `State::update`, `Driver::run`, `Keeper` |
| `backend-windows` | Packet-level WinDivert code from the first design. Parked, not built into the release. | - |

Parked modules in `core` (unused by the relay, kept with their tests): `segment`,
`checksum`, `packet`, `flow`, `engine`, `pump`. Ignore them unless a packet-level
backend comes back.

## 3. Core: deciding about the first flight

The "first flight" is the first bytes the app sends: a TLS ClientHello on a TLS port,
an HTTP request head on an HTTP port.

```mermaid
classDiagram
    direction LR
    class RelayProfile {
        +String name
        +DomainList domains
        +Vec~SplitPosition~ tls_positions
        +Vec~u16~ tls_ports
        +Vec~u16~ http_ports
        +HttpMethod http_method
    }
    class DomainList {
        +new(domains) DomainList
        +matched_domain(host) Option~Range~
    }
    class SplitPosition {
        <<enum>>
        HostStart
        DomainMid
        Offset(usize)
    }
    class Strategy {
        +Order order
        +Vec~SplitPosition~ positions
        +stream_offsets(host_range, host, matched) Vec~usize~
    }
    class HttpMethod {
        <<enum>>
        PassThrough
        HostSpell
    }
    class Protocol {
        <<enum>>
        Tls
        Http
    }
    class Inspection {
        <<enum>>
        Host(Range)
        NeedMoreData
        NoHost
    }
    class Decision {
        <<enum>>
        NeedMoreData
        Forward(host, cuts)
        Refuse(Refusal)
    }
    class Refusal {
        <<enum>>
        NoHost
        NotListed
        TooLong
    }
    class first_flight {
        <<module>>
        +decide(profile, protocol, bytes, max_len) Decision
    }
    class tls {
        <<module>>
        +inspect_client_hello(bytes) Inspection
    }
    class http {
        <<module>>
        +inspect_request(bytes) Inspection
    }
    class HostRespeller {
        +feed(input, output)
        +finish(output)
        +respelled() usize
        +stopped() Option~StopReason~
    }
    class StopReason {
        <<enum>>
        TransferEncoding
        ContentLength
        Upgrade
        Connect
        HeadTooLong
        NotHttp
    }

    RelayProfile *-- DomainList
    RelayProfile o-- SplitPosition
    RelayProfile --> HttpMethod
    Strategy o-- SplitPosition
    first_flight ..> RelayProfile : reads
    first_flight ..> tls : Protocol Tls
    first_flight ..> http : Protocol Http
    first_flight ..> Strategy : TLS cuts
    first_flight ..> Decision : returns
    tls ..> Inspection
    http ..> Inspection
    Decision --> Refusal
    HostRespeller ..> StopReason
```

`decide` is the one function the relay calls on every new connection:

```mermaid
flowchart TD
    A["decide(profile, protocol, first_bytes, max_len)"] --> B{"inspect:<br/>tls or http"}
    B -- "NeedMoreData and len >= max_len" --> R1["Refuse(TooLong)"]
    B -- "NeedMoreData" --> W["NeedMoreData<br/>(relay reads more)"]
    B -- "NoHost" --> R2["Refuse(NoHost)"]
    B -- "Host(range)" --> C{"host printable ASCII?"}
    C -- no --> R2
    C -- yes --> D{"DomainList matches?"}
    D -- no --> R3["Refuse(NotListed)"]
    D -- yes --> E{"protocol"}
    E -- Tls --> F["Strategy.stream_offsets<br/>keep cuts in 1..len"]
    E -- Http --> G["no cuts"]
    F --> H["Forward { host, cuts }"]
    G --> H
```

Split positions, for a ClientHello for `a.store.steampowered.com` with
`steampowered.com` listed:

- `Offset(1)`: after the first byte of the TLS record (the measured working cut).
- `HostStart`: right before `a.store...` inside the SNI extension.
- `DomainMid`: in the middle of the first label of the matched entry, `steamp|owered`.

HTTP is not cut. With `http-method host-spell`, `HostRespeller` rewrites the header name
`Host:` to `hoSt:` on every request head of a keep-alive connection, following
`Content-Length` framing, and stops (passes bytes through unchanged) on cases it cannot
frame safely: `StopReason`.

## 4. Relay crate

```mermaid
classDiagram
    direction TB
    class Config {
        +Vec~Listener~ listeners
        +Vec~RelayProfile~ profiles
        +Duration piece_gap
        +Duration first_flight_timeout
        +usize max_first_flight
        +Duration connect_timeout
    }
    class Listener {
        +SocketAddr listen
        +u16 upstream_port
        +Protocol protocol
        +usize profile
    }
    class Relay {
        -Vec~Bound~ bound
        -Arc~Shared~ shared
        +bind(config, resolver, on_event) Relay
        +local_addrs() Vec~SocketAddr~
        +run(shutdown)
    }
    class Shared {
        <<crate>>
        profiles
        piece_gap, timeouts
        resolver: boxed dyn ResolveBoxed
        on_event: EventSink
        +emit(Event)
    }
    class Route {
        <<crate>>
        +Protocol protocol
        +usize profile
        +u16 upstream_port
    }
    class DnsServer {
        -UdpSocket socket
        -Arc~Policy~ policy
        +bind(listen, profiles, answer, upstreams, resolver) DnsServer
        +run(shutdown)
    }
    class Policy {
        <<crate>>
        profiles
        answer: Ipv4Addr
        upstreams: Upstreams
        resolver
    }
    class Resolve {
        <<trait>>
        +resolve(host) Vec~Ipv4Addr~
    }
    class UdpResolver {
        -Upstreams servers
        -cache of answers, 30 s
        +new(servers, timeout)
    }
    class Upstreams {
        shared list of SocketAddr
        +current() Vec~SocketAddr~
        +replace(servers) bool
    }
    class Event {
        <<enum>>
        Forwarded(host, port, pieces)
        Refused(port, refusal)
        Failed(host, port, error)
        Closed(host, port, bytes, respelled, stopped)
    }

    Config *-- Listener
    Relay ..> Config : bind
    Relay *-- Shared
    Relay o-- Route : one per listener
    Shared --> Resolve
    Shared ..> Event : emits
    DnsServer *-- Policy
    Policy --> Upstreams
    Policy --> Resolve
    UdpResolver ..|> Resolve
    UdpResolver --> Upstreams
```

`Upstreams` is a shared, replaceable list: the session re-reads the adapter's DNS servers
every 30 s and swaps them in, and the resolver and every DNS server see the change.

### One TLS connection through the relay

`crates/relay/src/connection.rs`, `serve`:

```mermaid
sequenceDiagram
    autonumber
    participant App
    participant Relay as Relay accept loop
    participant Conn as serve (one task)
    participant Core as core::decide
    participant Res as UdpResolver
    participant Srv as Real server

    App->>Relay: TCP connect 127.0.0.2:443
    Relay->>Conn: spawn serve(client, shared, route)
    loop until decide says Forward or Refuse (10 s, 16 KiB max)
        App->>Conn: bytes of the ClientHello
        Conn->>Core: decide(profile, Tls, bytes)
        Core-->>Conn: NeedMoreData
    end
    Core-->>Conn: Forward { host, cuts }
    Conn->>Res: resolve(host)
    Res-->>Conn: real IPv4 (loopback answers rejected)
    Conn->>Srv: TCP connect, port = route.upstream_port
    Conn->>Srv: piece 1
    Note over Conn: wait piece_gap (30 ms)
    Conn->>Srv: piece 2 (and 3)
    Conn-->>Relay: Event::Forwarded
    par both directions until both end
        App->>Srv: bytes unchanged
    and
        Srv->>App: bytes unchanged
    end
    Conn-->>Relay: Event::Closed (byte counts)
```

For an HTTP port the first flight is not cut; it goes through `HostRespeller`, and so
does the rest of the client-to-server direction (`copy_respelled`). A refusal or a
failure ends the connection and emits `Event::Refused` or `Event::Failed`.

### One DNS query

`crates/relay/src/dns_server.rs`:

```mermaid
flowchart TD
    Q["query datagram"] --> P{"parse_query ok?"}
    P -- no --> X["drop"]
    P -- yes --> L{"name matches<br/>a profile's DomainList?"}
    L -- no --> F["forward to upstreams in order,<br/>return first reply with matching id"]
    L -- yes --> V{"upstream has an IPv4<br/>for this name?"}
    V -- no --> F
    V -- yes --> T{"qtype"}
    T -- A --> A1["answer A = this profile's<br/>loopback address, TTL 5 s"]
    T -- "AAAA, HTTPS, other" --> A2["empty answer<br/>(no IPv6 bypass, no ECH hints)"]
```

## 5. CLI crate: a session

```mermaid
classDiagram
    direction TB
    class RunOptions {
        +bool verbose
        +Vec~str~ profiles
        +Vec~PathBuf~ profile_files
        +Option~PathBuf~ site_folder
        +bool routing
        +Vec~Ipv4Addr~ upstreams
    }
    class ProfileSpec {
        +String name
        +Vec~String~ domains
        +Vec~u16~ tls_ports
        +Vec~SplitPosition~ tls_positions
        +Vec~u16~ http_ports
        +HttpMethod http_method
        +relay_profile() RelayProfile
    }
    class BuiltIn {
        +name: steam or medium
        +text: include_str profile
        +spec() ProfileSpec
    }
    class SiteFile {
        +String name
        +loaded: ProfileSpec or error text
    }
    class Selected {
        +ProfileSpec profile
        +Ipv4Addr address
    }
    class plan {
        <<module>>
        +select(built_ins, files) Vec~Selected~
        +relay_config(selected) Config
        +upstream_servers(explicit, system) Vec~SocketAddr~
    }
    class Session~H~ {
        <<private>>
        host: H
        options, selected, out
        start_up(pending, stop)
        relay_until_stopped(bound, stop, ready)
        remove_rules_if_routed()
    }
    class Host {
        <<trait>>
        dns_servers()
        begin_rules(rules) Start
        remove_rules() usize
        count_rules() usize
    }
    class System {
        real Windows host
    }
    class PendingStart {
        +add() Result
        +abandon()
    }
    class Gated {
        powershell.exe child
        waits on stdin before adding
    }
    class Instance {
        open lock file and folder
    }
    class Claim {
        <<enum>>
        Ours(Instance)
        Elsewhere
        Unlocked(error)
    }
    class Failure {
        <<enum>>
        Binding
        ChangingRules
        RemovingRules
        NothingSwitchedOn
        Other
    }

    BuiltIn ..> ProfileSpec : parses to
    SiteFile --> ProfileSpec
    plan ..> Selected : creates
    Selected *-- ProfileSpec
    Session --> RunOptions
    Session --> Selected
    Session --> Host
    System ..|> Host
    System ..> PendingStart : begin_rules
    PendingStart *-- Gated
    Claim --> Instance
```

`Host` is the seam for tests: the real `System` reads adapter DNS servers and runs
PowerShell; tests pass a fake host, so no test needs administrator rights.

### `snisplit run` from start to Ctrl+C

`crates/cli/src/main.rs` and `crates/cli/src/session.rs`:

```mermaid
sequenceDiagram
    autonumber
    participant Main as main / run
    participant Inst as instance
    participant Ses as session::serve
    participant PS as PowerShell (Gated)
    participant Net as Relay + DnsServers
    participant Win as Windows NRPT

    Main->>Inst: claim(%ProgramData%\snisplit\instance.lock)
    Inst-->>Main: Ours (else "already running")
    Main->>Ses: serve(options, ctrl_c, stdout)
    Ses->>Ses: load_selection: built-ins + site folder - off.list
    Ses->>Ses: plan::select: one loopback address per profile
    Ses->>PS: begin_rules: spawn script
    PS->>Win: remove stale snisplit rules
    PS-->>Ses: "stale rules removed", now waits on stdin
    Ses->>Ses: read adapter DNS servers (upstreams)
    Ses->>Net: Relay::bind + DnsServer::bind per profile
    Ses->>PS: add(): write newline to stdin
    PS->>Win: Add-DnsClientNrptRule per profile, Clear-DnsClientCache
    Ses-->>Main: prints the ready line
    par until stop
        Net->>Net: relay.run, dns.run
    and
        Ses->>Ses: refresh upstreams every 30 s
    end
    Main-->>Ses: Ctrl+C
    Ses->>Win: remove rules with Comment 'snisplit', count what is left
    Ses->>Net: stop tasks
    Ses-->>Main: Ok, or Failure::RemovingRules if rules remain
```

The two-step PowerShell start exists so that rules are never added before the sockets
are bound: if binding fails, or the user stops during start-up, the child is abandoned
(stdin closed) and adds nothing. Rules carry the comment `snisplit`, so `cleanup` and
the next `run` can find and remove leftovers after a crash.

Where things live on disk:

| Path | Written by | Read by |
|---|---|---|
| `%APPDATA%\snisplit\sites\<name>.profile` | GUI "Add", or the user by hand | `run`, GUI |
| `%APPDATA%\snisplit\off.list` | GUI switches | `run` (unless `--profile` is given), GUI |
| `%ProgramData%\snisplit\instance.lock` | whichever snisplit runs | CLI and GUI, one instance per machine |
| `crates/cli/profiles/*.profile` | developers | compiled in as `BUILT_IN` |

## 6. GUI crate

The GUI is an Elm-style loop. `State` is pure and fully unit-tested: it takes an `Event`
and returns `Effect`s. `Driver` performs effects (files, sessions, probes) on a tokio
runtime and sends results back as new events. `view` turns `State` into plain view
structs, and `window` / `widgets` draw them with egui.

```mermaid
flowchart LR
    subgraph ui["UI thread"]
        W["window::App<br/>(eframe::App)"]
        S["state::State<br/>update(Event) -> Vec of Effect"]
        V["view::*<br/>hero, rows, details, add_panel"]
        G["widgets<br/>(egui painting)"]
    end
    subgraph Keeper["driver::Keeper (owns tokio runtime)"]
        D["Driver::run(Effect)"]
        T["tasks: session, probe,<br/>leftover removal"]
    end
    CLI["snisplit lib<br/>session, sites, instance"]
    RL["snisplit-relay<br/>probe"]

    W -- "user input -> Event" --> S
    S -- Effect --> D
    D --> T
    T --> CLI
    T --> RL
    T -- "Event via mpsc Inbox<br/>+ request_repaint" --> W
    S --> V --> G
```

```mermaid
classDiagram
    direction LR
    class State {
        -Relay relay
        -Vec~Site~ sites
        -VecDeque~String~ log
        -Open open
        -Adding adding
        -Option~Probing~ probe
        +update(Event) Vec~Effect~
    }
    class Event {
        <<enum>>
        Launched, StartPressed, StopPressed
        SessionReady, SessionLine, SessionEnded
        SwitchPressed, TestPressed, ProbeFinished
        AddPressed, SiteSaved, CloseRequested
        ...
    }
    class Effect {
        <<enum>>
        ClaimInstance, CountLeftovers
        StartSession, StopSession, RemoveLeftovers
        Probe, CancelProbe
        SetSwitch, SaveSite, DeleteSite, LoadSites
        SaveLog, Exit
    }
    class Site {
        +String name
        +Vec~String~ domains
        +SiteKind kind
        +bool on
    }
    class Keeper {
        -Option~Runtime~ runtime
        -Mutex~Tracked~ tracked
        +driver(folders, wake) Driver
        +finish() Ending
    }
    class Driver {
        -Inbox inbox
        -Folders folders
        +run(Effect)
    }
    class App {
        state: State
        driver: Driver
        events: Receiver~Event~
    }

    App *-- State
    App *-- Driver
    Driver --> Keeper
    State *-- Site
    State ..> Event : consumes
    State ..> Effect : produces
    Driver ..> Effect : performs
    Driver ..> Event : sends back
```

`Keeper` outlives the window: `Keeper::finish` (and its `Drop`) stops a running session
and waits up to 60 s for the DNS rules to be removed, so closing the window, or a panic
in the UI, does not leave rules behind.

The round button follows `state::Relay`:

```mermaid
stateDiagram-v2
    [*] --> Off: Launched, lock is ours
    [*] --> AlreadyOpen: lock held elsewhere
    AlreadyOpen --> Off: ClaimAgainPressed, lock now ours
    Off --> Leftovers: CountLeftovers found rules
    Off --> Starting: StartPressed
    Failed --> Starting: StartPressed (Try again)
    Leftovers --> Starting: StartPressed
    Leftovers --> RemovingLeftovers: RemoveLeftoversPressed
    RemovingLeftovers --> Off: removed
    RemovingLeftovers --> Leftovers: removal refused
    Starting --> On: SessionReady
    Starting --> Stopping: StopPressed (Cancel) or site switched
    On --> Stopping: StopPressed or site switched
    Starting --> Failed: SessionEnded with error
    Stopping --> Off: SessionEnded clean
    Stopping --> Leftovers: SessionEnded, Failure RemovingRules
    Stopping --> Starting: restart_pending (a switch changed while on)
```

## 7. Probe: "is this site blocked, and what works?"

Used by `snisplit probe <domain>` and the GUI's Test button. It does not need routing or
administrator rights: it connects straight to the real address.

```mermaid
sequenceDiagram
    autonumber
    participant Caller as cli probe / gui Driver
    participant P as relay::probe
    participant Core as core::probe
    participant Srv as Real server

    Caller->>P: probe(resolver, domain, settings)
    P->>P: resolve domain to a usable IPv4
    loop runs (default 2)
        loop each Attempt
            Note over Core: TlsWhole, TlsSplit(positions) x4,<br/>HttpPlain, HttpRespelled
            P->>Core: first_flight(attempt, domain, random)
            Core-->>P: synthetic ClientHello or GET, plus cuts
            P->>Srv: connect, write_pieces
            Srv-->>P: reply, reset or timeout
            P->>Core: reply_outcome -> Outcome
        end
    end
    P->>Core: assess(domain, records)
    Core-->>P: Assessment { verdict, https, http, tls_split, profile }
    P-->>Caller: ProbeReport (the GUI maps it to ProbeView)
```

`Verdict` is one of `NotBlocked`, `ProfileSuggested` (with the profile text to save),
`NoProfileHelps`, `NothingWorked`.

## 8. Where to start reading

1. `crates/core/src/first_flight.rs`, then `strategy.rs`, `domains.rs`, `tls.rs`.
2. `crates/relay/src/connection.rs` (`serve`), `pieces.rs`, `dns_server.rs`.
3. `crates/cli/src/session.rs` (`serve_on`, `Session`), `plan.rs`, `routing.rs`.
4. `crates/gui/src/state.rs` (`update`), then `driver.rs`.

Tests: `cargo test --workspace`. Integration tests under `crates/*/tests/` run the relay
and DNS server on loopback with fake resolvers; `crates/core/tests/` replays real
ClientHellos captured from Chrome and Steam. Elevated, end-to-end checks are the
PowerShell scripts in `tools/acceptance/`.
