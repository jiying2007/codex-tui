//! Synthetic CPU timings; input mutation/setup is outside the measured commit.
use super::*;
pub(crate) fn measure_once(state: &mut AppState, now: u64) -> [f64; 3] {
    let started = Instant::now();
    let input = Input::capture(state, now);
    let capture = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let output = input.compute();
    let compute = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    assert!(output.apply_at(state, now).is_some());
    [capture, compute, started.elapsed().as_secs_f64() * 1000.0]
}
pub(crate) fn measure_rebase_once(state: &mut AppState, now: u64) -> f64 {
    let output = Input::capture(state, now).compute();
    let mut threads = state.threads.clone();
    let thread = threads.first_mut().expect("nonempty diagnostic fixture");
    let id = thread.id.0.clone();
    let title = format!("Source changed during worker generation {now}");
    thread.title = title.clone();
    reduce(state, Action::ReplaceThreads(threads));
    let started = Instant::now();
    let accepted = output.apply_at(state, now).is_some();
    let retry = (!accepted).then(|| Input::capture(state, now));
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    if let Some(fresh) = retry {
        assert_eq!(state.threads.len(), 50_000);
        assert!(fresh.compute().apply_at(state, now).is_some());
    }
    let index = state.work_card_by_thread[&id];
    assert_eq!(state.work_cards[index].title, title);
    elapsed
}
