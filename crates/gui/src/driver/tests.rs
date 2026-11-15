use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use snisplit::session::Failure;

use super::*;

const SLOW_STOP: Duration = Duration::from_millis(1500);
const PROMPT: Duration = Duration::from_millis(250);
const WAIT: Duration = Duration::from_secs(10);

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn quiet() -> Wake {
    Arc::new(|| {})
}

async fn stops_slowly(stopped: Stopped, ready: Ready, ended: io::Result<()>) -> io::Result<()> {
    ready();
    let _ = stopped.await;
    tokio::time::sleep(SLOW_STOP).await;
    ended
}

fn wait_until_ready(events: &Receiver<Event>) {
    loop {
        if let Event::SessionReady { .. } = events.recv_timeout(WAIT).unwrap() {
            return;
        }
    }
}

#[test]
fn dropping_the_driver_while_a_slow_session_stops_is_prompt_and_the_wait_after_sees_a_clean_end() {
    let keeper = Keeper::new().unwrap();
    let (mut driver, events) = keeper.driver(Folders::default(), quiet());
    driver.start_session_with(|stopped, _, ready| stops_slowly(stopped, ready, Ok(())));
    wait_until_ready(&events);
    driver.run(Effect::StopSession);
    let dropping = Instant::now();
    drop(driver);
    drop(events);
    assert!(dropping.elapsed() < PROMPT, "{:?}", dropping.elapsed());
    let still_stopping = keeper
        .tracked()
        .tidying
        .as_ref()
        .is_some_and(|task| !task.is_finished());
    assert!(still_stopping);
    let waiting = Instant::now();
    assert_eq!(keeper.finish(), Ending::Clean);
    assert!(
        waiting.elapsed() >= SLOW_STOP - PROMPT,
        "{:?}",
        waiting.elapsed()
    );
}

#[derive(Default)]
struct Watched {
    stop_sent: AtomicBool,
    ended: AtomicBool,
}

fn start_watched_slow_session(driver: &mut Driver<'_>, events: &Receiver<Event>) -> Arc<Watched> {
    let watched = Arc::new(Watched::default());
    let seen = Arc::clone(&watched);
    driver.start_session_with(move |stopped, _, ready| async move {
        ready();
        let sent = stopped.await.is_ok();
        seen.stop_sent.store(sent, Ordering::SeqCst);
        tokio::time::sleep(SLOW_STOP).await;
        seen.ended.store(true, Ordering::SeqCst);
        Ok(())
    });
    wait_until_ready(events);
    watched
}

#[test]
fn a_keeper_dropped_without_finish_still_stops_the_session_and_waits_for_its_end() {
    let keeper = Keeper::new().unwrap();
    let (mut driver, events) = keeper.driver(Folders::default(), quiet());
    let watched = start_watched_slow_session(&mut driver, &events);
    drop(driver);
    let dropping = Instant::now();
    drop(keeper);
    assert!(watched.stop_sent.load(Ordering::SeqCst));
    assert!(watched.ended.load(Ordering::SeqCst));
    assert!(
        dropping.elapsed() >= SLOW_STOP - PROMPT,
        "{:?}",
        dropping.elapsed()
    );
}

#[test]
fn a_panic_unwinding_past_the_keeper_still_stops_the_session_and_waits_for_its_end() {
    let watched = Arc::new(Mutex::new(None));
    let handed_out = Arc::clone(&watched);
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let keeper = Keeper::new().unwrap();
        let (mut driver, events) = keeper.driver(Folders::default(), quiet());
        *handed_out.lock().unwrap() = Some(start_watched_slow_session(&mut driver, &events));
        panic!("the window panicked on purpose");
    }));
    assert!(unwound.is_err());
    let watched = watched.lock().unwrap().take().unwrap();
    assert!(watched.stop_sent.load(Ordering::SeqCst));
    assert!(watched.ended.load(Ordering::SeqCst));
}

#[test]
fn the_wait_after_the_window_stops_a_running_session_and_reports_a_crash_as_rules_that_may_remain()
{
    let keeper = Keeper::new().unwrap();
    let (mut driver, events) = keeper.driver(Folders::default(), quiet());
    driver.start_session_with(|stopped, _, ready| async move {
        ready();
        let _ = stopped.await;
        tokio::time::sleep(SLOW_STOP).await;
        panic!("the session crashed while removing its rules, on purpose")
    });
    wait_until_ready(&events);
    drop(driver);
    assert_eq!(keeper.finish(), Ending::RulesMayRemain);
}

#[test]
fn a_session_that_failed_for_a_reason_other_than_removal_has_removed_its_rules() {
    let keeper = Keeper::new().unwrap();
    let (mut driver, events) = keeper.driver(Folders::default(), quiet());
    driver.start_session_with(|stopped, _, ready| {
        stops_slowly(
            stopped,
            ready,
            Err(io::Error::other("adapter list unreadable")),
        )
    });
    wait_until_ready(&events);
    drop(driver);
    assert_eq!(keeper.finish(), Ending::Clean);
}

#[test]
fn a_session_that_ignores_the_stop_is_given_up_after_the_limit() {
    let mut keeper = Keeper::new().unwrap();
    let (mut driver, _events) = keeper.driver(Folders::default(), quiet());
    driver.start_session_with(|_, _, _| std::future::pending::<io::Result<()>>());
    drop(driver);
    let waiting = Instant::now();
    assert_eq!(
        keeper.wind_down(Duration::from_millis(50)),
        Ending::TimedOut
    );
    assert!(waiting.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_successful_leftover_removal_after_a_failed_stop_makes_the_exit_clean() {
    let keeper = Keeper::new().unwrap();
    let (mut driver, events) = keeper.driver(Folders::default(), quiet());
    driver.start_session_with(|_, _, _| async { panic!("the session crashed, on purpose") });
    loop {
        if let Event::SessionEnded(_) = events.recv_timeout(WAIT).unwrap() {
            break;
        }
    }
    driver.remove_leftovers_with(|_| Ok(()));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::LeftoversRemoved(Ok(()))
    );
    drop(driver);
    assert_eq!(keeper.finish(), Ending::Clean);
}

#[test]
fn a_refused_leftover_removal_leaves_rules_that_may_remain_at_exit() {
    let keeper = Keeper::new().unwrap();
    let (driver, _events) = keeper.driver(Folders::default(), quiet());
    driver.remove_leftovers_with(|_| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Access is denied.",
        ))
    });
    drop(driver);
    assert_eq!(keeper.finish(), Ending::RulesMayRemain);
}

#[test]
fn nothing_started_exits_clean_without_waiting_for_a_blocking_task_that_never_returns() {
    let keeper = Keeper::new().unwrap();
    let blocked = Duration::from_secs(4);
    drop(
        keeper
            .runtime()
            .spawn_blocking(move || std::thread::sleep(blocked)),
    );
    std::thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    assert_eq!(keeper.finish(), Ending::Clean);
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
    let keeper = Keeper::new().unwrap();
    let (mut driver, _events) = keeper.driver(Folders::default(), quiet());
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
    assert_eq!(
        runtime.block_on(report_session_end(inbox.clone(), panicking)),
        Rules::MayRemain
    );
    assert_eq!(
        events.try_recv().unwrap(),
        Event::SessionEnded(SessionEnd::Failed {
            failure: Failure::RemovingRules,
            detail: "the session crashed, so DNS rules may remain".to_string()
        })
    );
    let stuck = runtime.spawn(std::future::pending::<io::Result<()>>());
    stuck.abort();
    assert_eq!(
        runtime.block_on(report_session_end(inbox.clone(), stuck)),
        Rules::MayRemain
    );
    assert_eq!(
        events.try_recv().unwrap(),
        Event::SessionEnded(SessionEnd::Failed {
            failure: Failure::RemovingRules,
            detail: "the session was cancelled, so DNS rules may remain".to_string()
        })
    );
    let clean = runtime.spawn(async { Ok(()) });
    assert_eq!(
        runtime.block_on(report_session_end(inbox, clean)),
        Rules::Removed
    );
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
    assert_eq!(
        runtime.block_on(report_leftovers_removed(inbox.clone(), panicking)),
        Rules::MayRemain
    );
    assert_eq!(
        events.try_recv().unwrap(),
        Event::LeftoversRemoved(Err("the removal crashed before it reported".to_string()))
    );
    let refused = runtime.spawn_blocking(|| Err("Access is denied.".to_string()));
    assert_eq!(
        runtime.block_on(report_leftovers_removed(inbox, refused)),
        Rules::MayRemain
    );
    assert_eq!(
        events.try_recv().unwrap(),
        Event::LeftoversRemoved(Err("Access is denied.".to_string()))
    );
}
