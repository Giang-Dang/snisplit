use std::time::{Duration, Instant};

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
