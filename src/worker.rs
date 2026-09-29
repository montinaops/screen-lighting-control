//! Latest-value-wins mailbox for background workers: posting a job for a key replaces any
//! pending job for the same key, so slow devices only ever see the newest value.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

struct Inner<T> {
    jobs: BTreeMap<usize, T>,
    last_put: Option<Instant>,
    quit: bool,
}

pub struct Mailbox<T> {
    inner: Mutex<Inner<T>>,
    cv: Condvar,
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Mailbox {
            inner: Mutex::new(Inner { jobs: BTreeMap::new(), last_put: None, quit: false }),
            cv: Condvar::new(),
        }
    }
}

impl<T> Mailbox<T> {
    pub fn put(&self, key: usize, job: T) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.jobs.insert(key, job);
        g.last_put = Some(Instant::now());
        self.cv.notify_one();
    }

    pub fn quit(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.quit = true;
        self.cv.notify_all();
    }

    /// Blocks until there are jobs and no new job arrived for `quiet` (debounce), then takes them
    /// all. Returns `None` once `quit` was called.
    pub fn take(&self, quiet: Duration) -> Option<Vec<(usize, T)>> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if g.quit {
                return None;
            }
            if !g.jobs.is_empty() {
                let since = g.last_put.map(|t| t.elapsed()).unwrap_or(quiet);
                if since >= quiet {
                    return Some(std::mem::take(&mut g.jobs).into_iter().collect());
                }
                g = self.cv.wait_timeout(g, quiet - since).unwrap_or_else(|e| e.into_inner()).0;
            } else {
                g = self.cv.wait(g).unwrap_or_else(|e| e.into_inner());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn latest_value_wins() {
        let mb = Mailbox::default();
        mb.put(1, "a");
        mb.put(1, "b");
        mb.put(0, "x");
        assert_eq!(mb.take(Duration::ZERO), Some(vec![(0, "x"), (1, "b")]));
    }

    #[test]
    fn debounce_waits_for_quiet() {
        let mb = Arc::new(Mailbox::default());
        mb.put(0, 1);
        let t0 = Instant::now();
        let got = mb.take(Duration::from_millis(60));
        assert!(t0.elapsed() >= Duration::from_millis(50));
        assert_eq!(got, Some(vec![(0, 1)]));
    }

    #[test]
    fn quit_unblocks() {
        let mb: Arc<Mailbox<u8>> = Arc::new(Mailbox::default());
        let m2 = mb.clone();
        let h = std::thread::spawn(move || m2.take(Duration::ZERO));
        std::thread::sleep(Duration::from_millis(20));
        mb.quit();
        assert_eq!(h.join().unwrap(), None);
    }
}
