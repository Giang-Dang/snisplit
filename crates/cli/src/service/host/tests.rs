use super::*;

fn fields(status: SERVICE_STATUS) -> [u32; 7] {
    [
        status.dwServiceType,
        status.dwCurrentState,
        status.dwControlsAccepted,
        status.dwWin32ExitCode,
        status.dwServiceSpecificExitCode,
        status.dwCheckPoint,
        status.dwWaitHint,
    ]
}

#[test]
fn running_accepts_stop_and_shutdown_with_no_checkpoint_or_wait_hint() {
    assert_eq!(
        fields(service_status(Phase::Running, 0)),
        [0x10, 4, 0x1 | 0x4, 0, 0, 0, 0]
    );
}

#[test]
fn stop_pending_accepts_nothing_and_asks_for_thirty_seconds() {
    assert_eq!(
        fields(service_status(Phase::StopPending, 3)),
        [0x10, 3, 0, 0, 0, 3, 30_000]
    );
}

#[test]
fn a_clean_stop_reports_exit_code_zero() {
    assert_eq!(
        fields(service_status(Phase::Stopped(Exit::Clean), 0)),
        [0x10, 1, 0, 0, 0, 0, 0]
    );
}

#[test]
fn a_failed_session_reports_a_service_specific_error_with_code_one() {
    assert_eq!(
        fields(service_status(
            Phase::Stopped(Exit::ServiceSpecific(SESSION_FAILED)),
            0
        )),
        [0x10, 1, 0, 1066, 1, 0, 0]
    );
}

#[test]
fn stop_pending_checkpoints_rise_and_nothing_is_reported_after_stopped() {
    let mut reported = Reported::default();
    let sequence = [
        Phase::Running,
        Phase::StopPending,
        Phase::StopPending,
        Phase::StopPending,
        Phase::Stopped(Exit::Clean),
        Phase::StopPending,
        Phase::Running,
    ];
    let states_and_checkpoints: Vec<Option<(u32, u32)>> = sequence
        .into_iter()
        .map(|phase| {
            reported
                .next(phase)
                .map(|status| (status.dwCurrentState, status.dwCheckPoint))
        })
        .collect();
    assert_eq!(
        states_and_checkpoints,
        vec![
            Some((4, 0)),
            Some((3, 1)),
            Some((3, 2)),
            Some((3, 3)),
            Some((1, 0)),
            None,
            None,
        ]
    );
}

#[test]
fn the_control_handler_answers_stop_shutdown_and_interrogate_and_refuses_the_rest() {
    let handle = |control| handle_control(control, 0, ptr::null_mut(), ptr::null_mut());
    assert_eq!(handle(4), 0);
    assert_eq!(handle(1), 0);
    assert_eq!(handle(5), 0);
    assert_eq!(handle(2), 120);
    assert_eq!(handle(15), 120);
}

#[test]
fn a_dispatcher_run_outside_the_service_control_manager_says_so() {
    let error = dispatcher_failure(io::Error::from_raw_os_error(1063));
    assert!(
        error
            .to_string()
            .starts_with("snisplit service run is only for the Service Control Manager"),
        "{error}"
    );
    assert_eq!(error.raw_os_error(), None);
}

#[test]
fn other_dispatcher_errors_keep_their_windows_code() {
    let error = dispatcher_failure(io::Error::from_raw_os_error(5));
    assert_eq!(error.raw_os_error(), Some(5));
}

#[test]
fn a_stop_requested_before_serve_listens_is_still_seen_when_it_starts_listening() {
    let service = Service {
        options: RunOptions {
            verbose: false,
            profiles: vec!["medium"],
            profile_files: vec![],
            routing: true,
            upstreams: vec![],
        },
        stop: watch::Sender::new(false),
        status: StatusReporter::new(),
    };
    service.request_stop();
    let listener = service.stop.subscribe();
    assert!(*listener.borrow());
}
