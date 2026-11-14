use super::*;

#[test]
fn a_relayed_connection_names_host_port_and_pieces() {
    let event = Event::Forwarded {
        host: "medium.com".to_string(),
        port: 443,
        pieces: 2,
    };
    assert_eq!(describe_event(&event), "relayed medium.com:443 in 2 pieces");
    let single = Event::Forwarded {
        host: "medium.com".to_string(),
        port: 80,
        pieces: 1,
    };
    assert_eq!(describe_event(&single), "relayed medium.com:80 in 1 piece");
}

#[test]
fn an_empty_first_write_says_the_request_head_is_still_arriving() {
    let event = Event::Forwarded {
        host: "cache5-hkg1.steamcontent.com".to_string(),
        port: 80,
        pieces: 0,
    };
    assert_eq!(
        describe_event(&event),
        "relayed cache5-hkg1.steamcontent.com:80 with the request head still arriving"
    );
}

#[test]
fn refusals_say_why_in_words() {
    let cases = [
        (
            Refusal::NoHost,
            "refused a connection on port 80: no host name in the first bytes",
        ),
        (
            Refusal::NotListed,
            "refused a connection on port 80: host is not in the profile",
        ),
        (
            Refusal::TooLong,
            "refused a connection on port 80: first bytes too long without a host name",
        ),
    ];
    for (refusal, expected) in cases {
        assert_eq!(
            describe_event(&Event::Refused { port: 80, refusal }),
            expected
        );
    }
}

#[test]
fn failures_name_the_host_when_it_is_known() {
    let known = Event::Failed {
        host: "store.steampowered.com".to_string(),
        port: 443,
        error: "connecting timed out".to_string(),
    };
    let unknown = Event::Failed {
        host: String::new(),
        port: 27019,
        error: "first flight timed out".to_string(),
    };
    assert_eq!(
        describe_event(&known),
        "failed store.steampowered.com:443: connecting timed out"
    );
    assert_eq!(
        describe_event(&unknown),
        "failed a connection on port 27019: first flight timed out"
    );
}

#[test]
fn a_closed_connection_reports_bytes_respelling_and_why_it_stopped() {
    let plain = Event::Closed {
        host: "medium.com".to_string(),
        port: 80,
        to_server: 210,
        to_client: 4096,
        respelled: 2,
        stopped: None,
    };
    let stopped = Event::Closed {
        host: "store.steampowered.com".to_string(),
        port: 80,
        to_server: 90,
        to_client: 0,
        respelled: 1,
        stopped: Some(StopReason::Upgrade),
    };
    assert_eq!(
        describe_event(&plain),
        "closed medium.com:80: 210 bytes sent, 4096 received, 2 Host headers respelled"
    );
    assert_eq!(
        describe_event(&stopped),
        "closed store.steampowered.com:80: 90 bytes sent, 0 received, 1 Host header respelled; \
         stopped respelling: the request asks for a protocol upgrade"
    );
    let tls = Event::Closed {
        host: "medium.com".to_string(),
        port: 443,
        to_server: 693,
        to_client: 10713,
        respelled: 0,
        stopped: None,
    };
    assert_eq!(
        describe_event(&tls),
        "closed medium.com:443: 693 bytes sent, 10713 received"
    );
}

#[test]
fn a_routing_failure_keeps_only_the_first_line_and_says_what_to_do() {
    let raw = io::Error::other(
        "powershell: Add-DnsClientNrptRule : Failed to load the NRPT information.\nAt line:1 char:88\n+ ...",
    );
    let shown = routing_failure(&raw);
    assert_eq!(
        shown.to_string(),
        "adding DNS rules failed (run from an administrator terminal, or pass --no-routing): \
         powershell: Add-DnsClientNrptRule : Failed to load the NRPT information."
    );
    assert_eq!(shown.kind(), io::ErrorKind::Other);
}

#[test]
fn the_start_line_lists_profiles_addresses_upstreams_and_routing() {
    let selected = plan::select(&["steam", "medium"], Vec::new()).unwrap();
    let upstreams = ["1.1.1.1:53".parse().unwrap(), "1.0.0.1:53".parse().unwrap()];
    assert_eq!(
        describe_start(&selected, &upstreams, true),
        format!(
            "snisplit {VERSION}: relaying steam (14 domains) on 127.0.0.2, \
             medium (1 domain) on 127.0.0.3; upstream DNS 1.1.1.1:53, 1.0.0.1:53; \
             DNS rules added; Ctrl+C to stop"
        )
    );
    assert!(describe_start(&selected, &upstreams, false).contains("; no DNS rules;"));
}

#[test]
fn the_start_line_shows_file_profiles_after_the_built_ins_with_their_own_addresses() {
    let file = profile::parse_file(
        "game.profile",
        "name game\ndomains game.example.com cdn.example.org\n",
    )
    .unwrap();
    let selected = plan::select(&["medium"], vec![file]).unwrap();
    let upstreams = ["1.1.1.1:53".parse().unwrap()];
    assert_eq!(
        describe_start(&selected, &upstreams, false),
        format!(
            "snisplit {VERSION}: relaying medium (1 domain) on 127.0.0.3, \
             game (2 domains) on 127.0.0.4; upstream DNS 1.1.1.1:53; \
             no DNS rules; Ctrl+C to stop"
        )
    );
}

mod start_up {
    use std::collections::VecDeque;
    use std::sync::{Mutex, PoisonError};

    use tokio::sync::Notify;

    use super::super::*;

    const TEST_TIMING: Timing = Timing {
        upstream_retry: Duration::from_millis(5),
        retries_per_report: 3,
        refresh_every: Duration::from_millis(20),
    };
    const HANG_LIMIT: Duration = Duration::from_secs(20);
    const WAITING: &str = "waiting for a usable upstream DNS server (none found yet; loopback addresses are skipped; pass --upstream <ipv4> to name one)";
    const RULES_LEFT: &str = "snisplit DNS rules left: 0";
    const BEGIN_MEDIUM: &str = "begin medium.com@127.0.0.3";

    fn ip(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
        Ipv4Addr::new(a, b, c, d)
    }

    fn options(routing: bool, upstreams: &[Ipv4Addr]) -> RunOptions {
        RunOptions {
            verbose: false,
            profiles: vec!["medium"],
            profile_files: Vec::new(),
            routing,
            upstreams: upstreams.to_vec(),
        }
    }

    fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        lookups: Mutex<VecDeque<io::Result<Vec<Ipv4Addr>>>>,
        lookups_used_up: Notify,
        add_result: Mutex<Option<Result<(), StartFailure>>>,
        add_takes: Duration,
        add_begun: Notify,
    }

    #[derive(Clone, Default)]
    struct FakeHost(Arc<Fake>);

    impl FakeHost {
        fn with(fake: Fake) -> Self {
            Self(Arc::new(fake))
        }

        fn answering(lookups: Vec<io::Result<Vec<Ipv4Addr>>>) -> Self {
            Self::with(Fake {
                lookups: Mutex::new(lookups.into()),
                ..Fake::default()
            })
        }

        fn record(&self, call: String) {
            locked(&self.0.calls).push(call);
        }

        fn calls(&self) -> Vec<String> {
            locked(&self.0.calls).clone()
        }
    }

    struct FakeStart(FakeHost);

    impl StartRules for FakeStart {
        fn add(self) -> Result<(), StartFailure> {
            self.0.record("add begins".to_string());
            self.0.0.add_begun.notify_one();
            std::thread::sleep(self.0.0.add_takes);
            self.0.record("add ends".to_string());
            locked(&self.0.0.add_result).take().unwrap_or(Ok(()))
        }

        fn abandon(self) {
            self.0.record("abandon".to_string());
        }
    }

    impl Host for FakeHost {
        type Start = FakeStart;

        fn dns_servers(&self) -> io::Result<Vec<Ipv4Addr>> {
            self.record("lookup".to_string());
            let next = locked(&self.0.lookups).pop_front();
            next.unwrap_or_else(|| {
                self.0.lookups_used_up.notify_one();
                Ok(Vec::new())
            })
        }

        fn begin_rules(&self, rules: &[ProfileRule]) -> io::Result<FakeStart> {
            let described: Vec<String> = rules
                .iter()
                .map(|rule| format!("{}@{}", rule.suffixes.join(" "), rule.server))
                .collect();
            self.record(format!("begin {}", described.join(", ")));
            Ok(FakeStart(self.clone()))
        }

        fn remove_rules(&self) -> io::Result<usize> {
            self.record("remove".to_string());
            Ok(0)
        }
    }

    type StopRule = Box<dyn Fn(&[String]) -> bool + Send + Sync>;

    struct Recorder {
        lines: Mutex<Vec<String>>,
        stop: Notify,
        stop_when: StopRule,
    }

    impl Recorder {
        fn stopping_when(
            stop_when: impl Fn(&[String]) -> bool + Send + Sync + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                lines: Mutex::new(Vec::new()),
                stop: Notify::new(),
                stop_when: Box::new(stop_when),
            })
        }

        fn output(self: &Arc<Self>) -> Output {
            let recorder = Arc::clone(self);
            Arc::new(move |line| {
                let mut lines = locked(&recorder.lines);
                lines.push(line.to_string());
                if (recorder.stop_when)(&lines) {
                    recorder.stop.notify_one();
                }
            })
        }

        fn stop_requested(self: &Arc<Self>) -> impl Future<Output = ()> + Send + 'static {
            let recorder = Arc::clone(self);
            async move { recorder.stop.notified().await }
        }

        fn lines(&self) -> Vec<String> {
            locked(&self.lines).clone()
        }
    }

    fn never_stops(_lines: &[String]) -> bool {
        false
    }

    fn run<T>(work: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async { tokio::time::timeout(HANG_LIMIT, work).await })
            .expect("the session hung")
    }

    fn serve_fake(
        host: &FakeHost,
        options: RunOptions,
        start: StartPolicy,
        stop_requested: impl Future<Output = ()> + Send,
        out: Output,
    ) -> io::Result<()> {
        run(serve_on(
            host.clone(),
            &TEST_TIMING,
            options,
            start,
            stop_requested,
            out,
        ))
    }

    const WAIT: StartPolicy = StartPolicy {
        wait_for_upstreams: true,
    };
    const NO_WAIT: StartPolicy = StartPolicy {
        wait_for_upstreams: false,
    };

    #[test]
    fn a_stop_while_waiting_for_upstreams_abandons_the_start_and_removes_every_rule() {
        let host = FakeHost::default();
        let recorder = Recorder::stopping_when(|lines| {
            lines.iter().filter(|line| line.as_str() == WAITING).count() == 2
        });
        let served = serve_fake(
            &host,
            options(true, &[]),
            WAIT,
            recorder.stop_requested(),
            recorder.output(),
        );
        assert!(served.is_ok());
        assert_eq!(recorder.lines(), [WAITING, WAITING, "stopping", RULES_LEFT]);
        assert_eq!(
            host.calls(),
            [
                BEGIN_MEDIUM,
                "lookup",
                "lookup",
                "lookup",
                "lookup",
                "abandon",
                "remove"
            ]
        );
    }

    #[test]
    fn without_waiting_a_missing_upstream_fails_at_once_and_still_removes_every_rule() {
        let host = FakeHost::answering(vec![Ok(vec![ip(127, 0, 0, 1)])]);
        let recorder = Recorder::stopping_when(never_stops);
        let error = serve_fake(
            &host,
            options(true, &[]),
            NO_WAIT,
            recorder.stop_requested(),
            recorder.output(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "no usable upstream DNS server found; pass --upstream <ipv4>"
        );
        assert_eq!(recorder.lines(), [RULES_LEFT]);
        assert_eq!(host.calls(), [BEGIN_MEDIUM, "lookup", "abandon", "remove"]);
    }

    #[test]
    fn without_waiting_a_failed_lookup_ends_start_up_with_its_error_and_removes_every_rule() {
        let host = FakeHost::answering(vec![Err(io::Error::other("adapter list unreadable"))]);
        let recorder = Recorder::stopping_when(never_stops);
        let error = serve_fake(
            &host,
            options(true, &[]),
            NO_WAIT,
            recorder.stop_requested(),
            recorder.output(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "adapter list unreadable");
        assert_eq!(recorder.lines(), [RULES_LEFT]);
        assert_eq!(host.calls(), [BEGIN_MEDIUM, "lookup", "abandon", "remove"]);
    }

    #[test]
    fn while_waiting_a_failed_lookup_is_retried_and_its_cause_logged() {
        let host = FakeHost::answering(vec![
            Err(io::Error::other("adapter list unreadable")),
            Ok(Vec::new()),
            Ok(Vec::new()),
        ]);
        let recorder = Recorder::stopping_when(|lines| lines.len() == 2);
        let served = serve_fake(
            &host,
            options(false, &[]),
            WAIT,
            recorder.stop_requested(),
            recorder.output(),
        );
        assert!(served.is_ok());
        assert_eq!(
            recorder.lines(),
            [
                "waiting for a usable upstream DNS server (none found yet; loopback addresses are skipped; pass --upstream <ipv4> to name one); reading them failed: adapter list unreadable",
                WAITING,
                "stopping"
            ]
        );
        assert_eq!(host.calls(), ["lookup", "lookup", "lookup", "lookup"]);
    }

    #[test]
    fn the_serve_future_can_be_spawned_on_a_multi_threaded_runtime() {
        fn assert_send<T: Send>(_: &T) {}
        let future = serve(
            options(false, &[ip(192, 0, 2, 1)]),
            NO_WAIT,
            std::future::ready(()),
            stdout(),
        );
        assert_send(&future);
    }

    #[test]
    fn production_timing_follows_the_plan() {
        assert_eq!(TIMING.upstream_retry, Duration::from_secs(2));
        assert_eq!(
            TIMING.upstream_retry * TIMING.retries_per_report,
            Duration::from_secs(60)
        );
        assert_eq!(TIMING.refresh_every, Duration::from_secs(30));
    }

    #[test]
    fn explicit_upstreams_that_are_all_loopback_fail_without_waiting_or_a_lookup() {
        let host = FakeHost::default();
        let recorder = Recorder::stopping_when(never_stops);
        let error = serve_fake(
            &host,
            options(true, &[ip(127, 0, 0, 1)]),
            WAIT,
            recorder.stop_requested(),
            recorder.output(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "no usable upstream DNS server found; pass --upstream <ipv4>"
        );
        assert_eq!(host.calls(), [BEGIN_MEDIUM, "abandon", "remove"]);
    }

    #[test]
    fn a_stop_before_start_up_ends_it_before_anything_is_bound() {
        let routed = FakeHost::default();
        let recorder = Recorder::stopping_when(never_stops);
        let served = serve_fake(
            &routed,
            options(true, &[ip(192, 0, 2, 1)]),
            NO_WAIT,
            std::future::ready(()),
            recorder.output(),
        );
        assert!(served.is_ok());
        assert_eq!(recorder.lines(), ["stopping", RULES_LEFT]);
        assert_eq!(routed.calls(), [BEGIN_MEDIUM, "abandon", "remove"]);

        let unrouted = FakeHost::default();
        let recorder = Recorder::stopping_when(never_stops);
        let served = serve_fake(
            &unrouted,
            options(false, &[ip(192, 0, 2, 1)]),
            NO_WAIT,
            std::future::ready(()),
            recorder.output(),
        );
        assert!(served.is_ok());
        assert_eq!(recorder.lines(), ["stopping"]);
        assert!(unrouted.calls().is_empty());
    }

    #[test]
    fn a_profile_file_gets_one_rule_with_all_its_domains_on_its_own_address() {
        let path =
            std::env::temp_dir().join(format!("snisplit-rule-{}.profile", std::process::id()));
        std::fs::write(
            &path,
            "name game\ndomains game.example.com cdn.example.org\ntls-ports 443\nhttp-ports none\n",
        )
        .unwrap();
        let host = FakeHost::default();
        let recorder = Recorder::stopping_when(never_stops);
        let mut options = options(true, &[ip(192, 0, 2, 1)]);
        options.profile_files = vec![path.clone()];
        let served = serve_fake(
            &host,
            options,
            NO_WAIT,
            std::future::ready(()),
            recorder.output(),
        );
        let _ = std::fs::remove_file(&path);
        assert!(served.is_ok());
        assert_eq!(
            host.calls()[0],
            "begin medium.com@127.0.0.3, game.example.com cdn.example.org@127.0.0.4"
        );
    }

    #[test]
    fn the_refresh_swaps_in_a_changed_list_and_ignores_empty_failed_and_equal_reads() {
        let first = vec![
            "192.0.2.1:53".parse::<SocketAddr>().unwrap(),
            "192.0.2.2:53".parse().unwrap(),
        ];
        let host = FakeHost::answering(vec![
            Ok(vec![ip(192, 0, 2, 1), ip(192, 0, 2, 2)]),
            Ok(Vec::new()),
            Err(io::Error::other("adapter list unreadable")),
            Ok(vec![ip(127, 0, 0, 1), ip(0, 0, 0, 0)]),
            Ok(vec![ip(192, 0, 2, 2), ip(192, 0, 2, 1)]),
            Ok(vec![ip(192, 0, 2, 2), ip(127, 0, 0, 1), ip(192, 0, 2, 1)]),
        ]);
        let upstreams = Upstreams::new(first);
        let recorder = Recorder::stopping_when(never_stops);
        let (stop, stopped) = watch::channel(false);
        let refresher = refresh_upstreams(
            host.clone(),
            upstreams.clone(),
            TEST_TIMING.refresh_every,
            recorder.output(),
            stopped,
        );
        let fake = Arc::clone(&host.0);
        run(async move {
            let refreshing = tokio::spawn(refresher);
            fake.lookups_used_up.notified().await;
            let _ = stop.send(true);
            refreshing.await.unwrap();
        });
        assert_eq!(
            recorder.lines(),
            ["upstream DNS now 192.0.2.2:53, 192.0.2.1:53"]
        );
        assert_eq!(
            upstreams.current(),
            [
                "192.0.2.2:53".parse::<SocketAddr>().unwrap(),
                "192.0.2.1:53".parse().unwrap()
            ]
        );
    }

    #[cfg(windows)]
    mod bound {
        use std::net::{TcpListener, UdpSocket};
        use std::path::PathBuf;

        use super::*;

        static LOOPBACK_PORTS: Mutex<()> = Mutex::new(());
        const BEGIN_GAME: &str = "begin game.example.com@127.0.0.4";
        const GAME_FILE: &str =
            "name game\ndomains game.example.com\ntls-ports 443\nhttp-ports none\n";

        struct GameProfile(PathBuf);

        impl GameProfile {
            fn new() -> Self {
                let path = std::env::temp_dir()
                    .join(format!("snisplit-game-{}.profile", std::process::id()));
                std::fs::write(&path, GAME_FILE).unwrap();
                Self(path)
            }

            fn options(&self, routing: bool, upstreams: &[Ipv4Addr]) -> RunOptions {
                RunOptions {
                    profiles: Vec::new(),
                    profile_files: vec![self.0.clone()],
                    ..options(routing, upstreams)
                }
            }
        }

        impl Drop for GameProfile {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }

        fn ready_line(upstreams: &str, rules: &str) -> String {
            format!(
                "snisplit {VERSION}: relaying game (1 domain) on 127.0.0.4; upstream DNS {upstreams}; {rules}; Ctrl+C to stop"
            )
        }

        fn says_ready(lines: &[String]) -> bool {
            lines.iter().any(|line| line.ends_with("Ctrl+C to stop"))
        }

        #[test]
        fn a_stop_after_the_ready_line_removes_rules_then_releases_the_ports() {
            let _ports = locked(&LOOPBACK_PORTS);
            let game = GameProfile::new();
            let host = FakeHost::default();
            let recorder = Recorder::stopping_when(says_ready);
            let served = serve_fake(
                &host,
                game.options(true, &[ip(192, 0, 2, 1)]),
                NO_WAIT,
                recorder.stop_requested(),
                recorder.output(),
            );
            assert!(served.is_ok());
            assert_eq!(
                recorder.lines(),
                [
                    ready_line("192.0.2.1:53", "DNS rules added"),
                    "stopping".to_string(),
                    RULES_LEFT.to_string()
                ]
            );
            assert_eq!(
                host.calls(),
                [BEGIN_GAME, "add begins", "add ends", "remove"]
            );
            assert!(UdpSocket::bind("127.0.0.4:53").is_ok());
            assert!(TcpListener::bind("127.0.0.4:443").is_ok());
        }

        #[test]
        fn a_stop_while_rules_are_being_added_waits_for_the_add_before_removing() {
            let _ports = locked(&LOOPBACK_PORTS);
            let game = GameProfile::new();
            let host = FakeHost::with(Fake {
                add_takes: Duration::from_millis(300),
                ..Fake::default()
            });
            let recorder = Recorder::stopping_when(never_stops);
            let fake = Arc::clone(&host.0);
            let stop_once_adding = async move { fake.add_begun.notified().await };
            let served = serve_fake(
                &host,
                game.options(true, &[ip(192, 0, 2, 1)]),
                NO_WAIT,
                stop_once_adding,
                recorder.output(),
            );
            assert!(served.is_ok());
            assert_eq!(recorder.lines(), ["stopping", RULES_LEFT]);
            assert_eq!(
                host.calls(),
                [BEGIN_GAME, "add begins", "add ends", "remove"]
            );
        }

        #[test]
        fn a_failed_add_says_what_to_do_and_a_failed_stale_removal_keeps_its_own_message() {
            let _ports = locked(&LOOPBACK_PORTS);
            let game = GameProfile::new();
            let cases = [
                (
                    StartFailure::Adding(io::Error::other("powershell: Access is denied.")),
                    "adding DNS rules failed (run from an administrator terminal, or pass --no-routing): powershell: Access is denied.",
                ),
                (
                    StartFailure::RemovingStale(io::Error::other("powershell: Access is denied.")),
                    "powershell: Access is denied.",
                ),
            ];
            for (failure, expected) in cases {
                let host = FakeHost::with(Fake {
                    add_result: Mutex::new(Some(Err(failure))),
                    ..Fake::default()
                });
                let recorder = Recorder::stopping_when(never_stops);
                let error = serve_fake(
                    &host,
                    game.options(true, &[ip(192, 0, 2, 1)]),
                    NO_WAIT,
                    recorder.stop_requested(),
                    recorder.output(),
                )
                .unwrap_err();
                assert_eq!(error.to_string(), expected);
                assert_eq!(recorder.lines(), [RULES_LEFT]);
                assert_eq!(
                    host.calls(),
                    [BEGIN_GAME, "add begins", "add ends", "remove"]
                );
            }
        }

        #[test]
        fn a_bind_failure_abandons_the_start_and_removes_every_rule() {
            let _ports = locked(&LOOPBACK_PORTS);
            let game = GameProfile::new();
            let _taken = UdpSocket::bind("127.0.0.4:53").unwrap();
            let host = FakeHost::default();
            let recorder = Recorder::stopping_when(never_stops);
            let error = serve_fake(
                &host,
                game.options(true, &[ip(192, 0, 2, 1)]),
                NO_WAIT,
                recorder.stop_requested(),
                recorder.output(),
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
            assert_eq!(recorder.lines(), [RULES_LEFT]);
            assert_eq!(host.calls(), [BEGIN_GAME, "abandon", "remove"]);
        }

        #[test]
        fn the_wait_ends_when_servers_appear_and_the_refresh_swaps_them_later() {
            let _ports = locked(&LOOPBACK_PORTS);
            let game = GameProfile::new();
            let host = FakeHost::answering(vec![
                Ok(Vec::new()),
                Ok(Vec::new()),
                Ok(vec![ip(127, 0, 0, 1), ip(192, 0, 2, 1)]),
                Ok(vec![ip(192, 0, 2, 7)]),
            ]);
            let recorder = Recorder::stopping_when(|lines| {
                lines
                    .iter()
                    .any(|line| line.starts_with("upstream DNS now"))
            });
            let served = serve_fake(
                &host,
                game.options(false, &[]),
                WAIT,
                recorder.stop_requested(),
                recorder.output(),
            );
            assert!(served.is_ok());
            assert_eq!(
                recorder.lines(),
                [
                    WAITING.to_string(),
                    ready_line("192.0.2.1:53", "no DNS rules"),
                    "upstream DNS now 192.0.2.7:53".to_string(),
                    "stopping".to_string()
                ]
            );
        }

        #[test]
        fn explicit_upstreams_turn_the_refresh_off() {
            let _ports = locked(&LOOPBACK_PORTS);
            let game = GameProfile::new();
            let host = FakeHost::answering(vec![Ok(vec![ip(192, 0, 2, 7)])]);
            let recorder = Recorder::stopping_when(says_ready);
            let ready = recorder.stop_requested();
            let several_refresh_periods_after_ready = async move {
                ready.await;
                tokio::time::sleep(TEST_TIMING.refresh_every * 5).await;
            };
            let served = serve_fake(
                &host,
                game.options(false, &[ip(192, 0, 2, 1)]),
                NO_WAIT,
                several_refresh_periods_after_ready,
                recorder.output(),
            );
            assert!(served.is_ok());
            assert!(host.calls().is_empty());
            assert_eq!(
                recorder.lines(),
                [
                    ready_line("192.0.2.1:53", "no DNS rules"),
                    "stopping".to_string()
                ]
            );
        }
    }
}
