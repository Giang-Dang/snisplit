use std::time::{Duration, Instant};

use snisplit::session::Failure;

use super::*;

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn waiting_for_a_session_task_from_outside_the_runtime_sees_it_end() {
    let runtime = runtime();
    let task = runtime.spawn(async { tokio::time::sleep(Duration::from_millis(20)).await });
    assert!(wait_until_ended(&runtime, task, Duration::from_secs(10)));
}

#[test]
fn a_session_task_that_never_ends_is_given_up_after_the_limit() {
    let runtime = runtime();
    let task = runtime.spawn(std::future::pending::<()>());
    let started = Instant::now();
    assert!(!wait_until_ended(&runtime, task, Duration::from_millis(50)));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn dropping_the_driver_does_not_wait_for_a_blocking_task_that_never_returns() {
    let (driver, _events) = Driver::new(Folders::default(), Arc::new(|| {})).unwrap();
    let blocked = Duration::from_secs(4);
    drop(
        driver
            .runtime()
            .spawn_blocking(move || std::thread::sleep(blocked)),
    );
    std::thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    drop(driver);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_session_result_becomes_a_clean_end_or_a_failure_with_its_text() {
    assert_eq!(session_end(Ok(())), SessionEnd::Clean);
    assert_eq!(
        session_end(Err(io::Error::other("adapter list unreadable"))),
        SessionEnd::Failed {
            failure: Failure::Other,
            detail: "adapter list unreadable".to_string()
        }
    );
}

#[test]
fn only_the_probe_with_the_cancelled_id_is_aborted() {
    let (mut driver, _events) = Driver::new(Folders::default(), Arc::new(|| {})).unwrap();
    let task = driver.runtime().spawn(std::future::pending::<()>());
    driver.probe = Some((7, task));
    driver.run(Effect::CancelProbe(6));
    assert_eq!(driver.probe.as_ref().map(|(id, _)| *id), Some(7));
    driver.run(Effect::CancelProbe(7));
    assert!(driver.probe.is_none());
}

fn inbox() -> (Inbox, Receiver<Event>) {
    let (events, received) = channel();
    let inbox = Inbox {
        events,
        wake: Arc::new(|| {}),
    };
    (inbox, received)
}

#[test]
fn a_session_task_that_panics_or_is_cancelled_still_ends_with_possible_leftovers() {
    let runtime = runtime();
    let (inbox, events) = inbox();
    let panicking = runtime.spawn(async { panic!("session task panicked on purpose") });
    runtime.block_on(report_session_end(inbox.clone(), panicking));
    assert_eq!(
        events.try_recv().unwrap(),
        Event::SessionEnded(SessionEnd::Failed {
            failure: Failure::RemovingRules,
            detail: "the session crashed, so DNS rules may remain".to_string()
        })
    );
    let stuck = runtime.spawn(std::future::pending::<io::Result<()>>());
    stuck.abort();
    runtime.block_on(report_session_end(inbox.clone(), stuck));
    assert_eq!(
        events.try_recv().unwrap(),
        Event::SessionEnded(SessionEnd::Failed {
            failure: Failure::RemovingRules,
            detail: "the session was cancelled, so DNS rules may remain".to_string()
        })
    );
    let clean = runtime.spawn(async { Ok(()) });
    runtime.block_on(report_session_end(inbox, clean));
    assert_eq!(
        events.try_recv().unwrap(),
        Event::SessionEnded(SessionEnd::Clean)
    );
}

#[test]
fn a_leftover_removal_that_panics_reports_a_failed_removal() {
    let runtime = runtime();
    let (inbox, events) = inbox();
    let panicking =
        runtime.spawn_blocking(|| -> Result<(), String> { panic!("removal panicked on purpose") });
    runtime.block_on(report_leftovers_removed(inbox.clone(), panicking));
    assert_eq!(
        events.try_recv().unwrap(),
        Event::LeftoversRemoved(Err("the removal crashed before it reported".to_string()))
    );
    let refused = runtime.spawn_blocking(|| Err("Access is denied.".to_string()));
    runtime.block_on(report_leftovers_removed(inbox, refused));
    assert_eq!(
        events.try_recv().unwrap(),
        Event::LeftoversRemoved(Err("Access is denied.".to_string()))
    );
}
