use super::*;

fn fit(resizer: &mut Resizer, target: f32, inner: f32) -> Vec<Command> {
    resizer.update(Sizing::Fit, target, Some(inner), Some(false))
}

fn free(resizer: &mut Resizer, inner: f32) -> Vec<Command> {
    resizer.update(Sizing::Free, 400.0, Some(inner), Some(false))
}

#[test]
fn the_first_fit_frame_locks_the_window_and_sends_the_height() {
    let mut resizer = Resizer::new();
    assert_eq!(
        fit(&mut resizer, 400.0, 680.0),
        [
            Command::Resizable(false),
            Command::MaximizeButton(false),
            Command::MinHeight(300.0),
            Command::Height(400.0),
        ]
    );
}

#[test]
fn fit_resends_only_when_the_target_moves_by_a_point_or_more() {
    let mut resizer = Resizer::new();
    fit(&mut resizer, 400.0, 400.0);
    assert_eq!(fit(&mut resizer, 400.4, 400.0), []);
    assert_eq!(fit(&mut resizer, 401.0, 400.0), [Command::Height(401.0)]);
}

#[test]
fn fit_never_asks_for_less_than_the_minimum() {
    let mut resizer = Resizer::new();
    assert_eq!(
        fit(&mut resizer, 250.0, 400.0).last(),
        Some(&Command::Height(300.0))
    );
}

#[test]
fn a_maximized_window_in_fit_is_restored_before_the_height_is_sent() {
    let mut resizer = Resizer::new();
    fit(&mut resizer, 400.0, 400.0);
    assert_eq!(
        resizer.update(Sizing::Fit, 400.0, Some(1000.0), Some(true)),
        [Command::Maximize(false), Command::Height(400.0)]
    );
}

#[test]
fn a_window_changed_behind_fits_back_but_a_refused_size_is_not_resent_every_frame() {
    let mut resizer = Resizer::new();
    fit(&mut resizer, 400.0, 400.0);
    assert_eq!(fit(&mut resizer, 400.0, 380.0), [Command::Height(400.0)]);
    assert_eq!(fit(&mut resizer, 400.0, 380.0), []);
    assert_eq!(fit(&mut resizer, 400.0, 380.0), []);
    assert_eq!(fit(&mut resizer, 400.0, 390.0), [Command::Height(400.0)]);
}

#[test]
fn entering_free_restores_the_default_and_unlocks_the_window() {
    let mut resizer = Resizer::new();
    fit(&mut resizer, 400.0, 400.0);
    assert_eq!(
        free(&mut resizer, 400.0),
        [
            Command::Resizable(true),
            Command::MaximizeButton(true),
            Command::MinHeight(480.0),
            Command::Height(720.0),
        ]
    );
}

#[test]
fn the_fit_height_seen_while_free_settles_does_not_overwrite_the_remembered_height() {
    let mut resizer = Resizer::new();
    fit(&mut resizer, 400.0, 400.0);
    free(&mut resizer, 400.0);
    assert_eq!(free(&mut resizer, 400.0), []);
    free(&mut resizer, 720.0);
    fit(&mut resizer, 400.0, 720.0);
    assert_eq!(
        free(&mut resizer, 400.0).last(),
        Some(&Command::Height(720.0))
    );
}

#[test]
fn a_free_resize_by_the_user_is_restored_after_a_stay_in_fit() {
    let mut resizer = Resizer::new();
    fit(&mut resizer, 400.0, 400.0);
    free(&mut resizer, 400.0);
    free(&mut resizer, 720.0);
    free(&mut resizer, 900.0);
    fit(&mut resizer, 400.0, 900.0);
    assert_eq!(
        free(&mut resizer, 400.0).last(),
        Some(&Command::Height(900.0))
    );
    assert_eq!(free(&mut resizer, 400.0), []);
    assert_eq!(free(&mut resizer, 900.0), []);
    fit(&mut resizer, 400.0, 900.0);
    assert_eq!(
        free(&mut resizer, 400.0).last(),
        Some(&Command::Height(900.0))
    );
}

#[test]
fn a_maximized_free_window_does_not_become_the_remembered_height() {
    let mut resizer = Resizer::new();
    fit(&mut resizer, 400.0, 400.0);
    free(&mut resizer, 400.0);
    free(&mut resizer, 720.0);
    resizer.update(Sizing::Free, 400.0, Some(1400.0), Some(true));
    fit(&mut resizer, 400.0, 1400.0);
    assert_eq!(
        free(&mut resizer, 400.0).last(),
        Some(&Command::Height(720.0))
    );
}

#[test]
fn the_layout_uses_the_free_height_while_the_resize_is_settling() {
    let mut resizer = Resizer::new();
    fit(&mut resizer, 400.0, 400.0);
    assert_eq!(resizer.layout_height(Sizing::Free, 400.0), 720.0);
    free(&mut resizer, 400.0);
    assert_eq!(resizer.layout_height(Sizing::Free, 400.0), 720.0);
    free(&mut resizer, 720.0);
    assert_eq!(resizer.layout_height(Sizing::Free, 650.0), 650.0);
    assert_eq!(resizer.layout_height(Sizing::Fit, 400.0), 400.0);
}
