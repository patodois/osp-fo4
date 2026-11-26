//! Wall time this thread spent reading archive bytes and inflating them
//! (measurement only: a caller takes [`now`] before and after a step and
//! reports the difference; nothing reads it to decide anything).
use std::cell::Cell;
use std::time::Instant;

thread_local! {
    static READ: Cell<u64> = const { Cell::new(0) };
    static INFLATE: Cell<u64> = const { Cell::new(0) };
}

/// This thread's totals so far: (seconds reading stored bytes, seconds
/// inflating them).
pub fn now() -> (f64, f64) {
    (
        READ.with(|c| c.get()) as f64 * 1e-9,
        INFLATE.with(|c| c.get()) as f64 * 1e-9,
    )
}

fn timed<T>(cell: &'static std::thread::LocalKey<Cell<u64>>, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    let ns = start.elapsed().as_nanos() as u64;
    cell.with(|c| c.set(c.get().saturating_add(ns)));
    out
}

/// Runs `f`, counting its time as reading stored bytes.
pub fn reading<T>(f: impl FnOnce() -> T) -> T {
    timed(&READ, f)
}

/// Runs `f`, counting its time as inflating.
pub fn inflating<T>(f: impl FnOnce() -> T) -> T {
    timed(&INFLATE, f)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_counters_grow_only_on_this_thread() {
        let (r0, i0) = super::now();
        super::reading(|| std::thread::sleep(std::time::Duration::from_millis(2)));
        super::inflating(|| std::thread::sleep(std::time::Duration::from_millis(2)));
        let (r1, i1) = super::now();
        assert!(r1 - r0 >= 0.002 && i1 - i0 >= 0.002);
        let other = std::thread::spawn(super::now).join().unwrap();
        assert_eq!(other, (0.0, 0.0));
    }
}
