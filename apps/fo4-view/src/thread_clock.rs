//! The play data thread's own clock (measurement only, no game behaviour):
//! the CPU time this thread ran, beside the wall time a load took, tells a
//! load that waited for a core (wall much above CPU) from one that ran
//! slower (both up together). Raising the thread's scheduling class is not
//! done: the platform call needs code this repository forbids (`unsafe`),
//! and the safe class setter of the system's thread object does not reach
//! a thread it did not start (it reads back the default).

/// Seconds of CPU time the calling thread has run so far, when the
/// platform gives a per-thread clock.
pub fn thread_cpu_seconds() -> Option<f64> {
    #[cfg(unix)]
    {
        let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
        Some(t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// The line a load's clocks print: its wall seconds and the data thread's
/// CPU seconds over it, with their ratio.
pub fn clocks_line(wall: f64, cpu: Option<f64>) -> String {
    match cpu {
        Some(c) if wall > 0.0 => format!(
            "wall {wall:.2} s, the data thread's CPU {c:.2} s ({:.0}% of the wall)",
            100.0 * c / wall
        ),
        Some(c) => format!("wall {wall:.2} s, the data thread's CPU {c:.2} s"),
        None => format!("wall {wall:.2} s, the data thread's CPU time not read on this platform"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thread_clock_counts_this_threads_work_not_its_sleep() {
        let Some(c0) = thread_cpu_seconds() else {
            return;
        };
        std::thread::sleep(std::time::Duration::from_millis(30));
        let c1 = thread_cpu_seconds().unwrap();
        assert!(c1 - c0 < 0.02, "slept: {}", c1 - c0);
        let start = std::time::Instant::now();
        let mut x = 0u64;
        while start.elapsed().as_secs_f64() < 0.03 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        std::hint::black_box(x);
        let c2 = thread_cpu_seconds().unwrap();
        assert!(c2 - c1 > 0.01, "worked: {}", c2 - c1);
    }

    #[test]
    fn the_clocks_line_reads_as_written() {
        assert_eq!(
            clocks_line(2.0, Some(1.0)),
            "wall 2.00 s, the data thread's CPU 1.00 s (50% of the wall)"
        );
        assert!(clocks_line(1.0, None).contains("not read"));
    }
}
