//! Receive-only capture loop for Linux.
//!
//! The agent hands the input router ("owner") to the platform, which must run it on one
//! thread: install it, then call `poll` whenever it is woken or its deadline passes, until it
//! reports `stopped`. This device never captures local keyboard or mouse (it is only
//! controlled from another computer), so no native events are fed to the owner. The loop
//! still has to run: it drains the router's commands, timers and focus decisions, and the
//! owner is told that local capture is unavailable.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::InputError;
use crate::capture_loop::{self, CaptureLoop, Wake};

/// Upper bound for one sleep, so a missed wakeup can never stall shutdown for long.
const MAX_IDLE: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Signal {
    woken: Mutex<bool>,
    condvar: Condvar,
}

impl Signal {
    fn notify(&self) {
        *self.woken.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.condvar.notify_one();
    }

    /// Sleeps until notified or `deadline`, at most `MAX_IDLE`; consumes the notification.
    fn wait(&self, deadline: Option<Instant>) {
        let timeout = deadline
            .map_or(MAX_IDLE, |d| d.saturating_duration_since(Instant::now()))
            .min(MAX_IDLE);
        let guard = self.woken.lock().unwrap_or_else(|e| e.into_inner());
        let (mut woken, _) = self
            .condvar
            .wait_timeout_while(guard, timeout, |woken| !*woken)
            .unwrap_or_else(|e| e.into_inner());
        *woken = false;
    }
}

pub(crate) struct ReceiveLoop {
    stop: Arc<AtomicBool>,
    signal: Arc<Signal>,
    thread: JoinHandle<()>,
}

impl ReceiveLoop {
    pub(crate) fn spawn(owner: Box<dyn CaptureLoop>) -> Result<Self, InputError> {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::new(Signal::default());
        let thread = {
            let (stop, signal) = (stop.clone(), signal.clone());
            std::thread::Builder::new()
                .name("oink-linux-input-loop".into())
                .spawn(move || run(owner, &stop, &signal))
                .map_err(|e| InputError::Platform(format!("input loop thread: {e}")))?
        };
        Ok(Self { stop, signal, thread })
    }

    /// Stops the loop and waits for it, unless called from the loop's own thread
    /// (an owner callback may reach `stop`; joining itself would deadlock).
    pub(crate) fn stop(self) {
        self.stop.store(true, Ordering::Release);
        self.signal.notify();
        if self.thread.thread().id() != std::thread::current().id() {
            let _ = self.thread.join();
        }
    }
}

fn run(owner: Box<dyn CaptureLoop>, stop: &AtomicBool, signal: &Arc<Signal>) {
    let wake: Wake = {
        let signal = signal.clone();
        Arc::new(move || signal.notify())
    };
    capture_loop::install(owner, wake);
    // Same signal a failed hook installation gives on the other platforms.
    capture_loop::capture_available(false);
    while !stop.load(Ordering::Acquire) {
        let (deadline, stopped) = capture_loop::poll(Instant::now());
        if stopped {
            break;
        }
        signal.wait(deadline);
    }
    capture_loop::clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapturedEvent, Disposition};
    use std::sync::atomic::AtomicUsize;

    struct Probe {
        polls: Arc<AtomicUsize>,
        wake: Arc<Mutex<Option<Wake>>>,
        available: Arc<Mutex<Option<bool>>>,
    }

    impl CaptureLoop for Probe {
        fn set_wake(&mut self, wake: Wake) {
            *self.wake.lock().unwrap() = Some(wake);
        }
        fn event(&mut self, _: &CapturedEvent) -> Disposition {
            Disposition::Pass
        }
        fn capture_available(&mut self, available: bool) {
            *self.available.lock().unwrap() = Some(available);
        }
        fn poll(&mut self, _: Instant) -> Option<Instant> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            None
        }
        fn stopped(&self) -> bool {
            false
        }
    }

    fn wait_for(what: &str, condition: impl Fn() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(start.elapsed() < Duration::from_secs(3), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn runs_the_owner_wakes_on_signal_and_stops() {
        let polls = Arc::new(AtomicUsize::new(0));
        let wake = Arc::new(Mutex::new(None));
        let available = Arc::new(Mutex::new(None));
        let handle = ReceiveLoop::spawn(Box::new(Probe {
            polls: polls.clone(),
            wake: wake.clone(),
            available: available.clone(),
        }))
        .unwrap();

        wait_for("the startup poll", || polls.load(Ordering::SeqCst) >= 1);
        assert_eq!(*available.lock().unwrap(), Some(false), "owner must learn capture is unavailable");

        let before = polls.load(Ordering::SeqCst);
        let wake_fn = wake.lock().unwrap().clone().expect("wake handed to the owner");
        wake_fn();
        wait_for("a poll after wake", || polls.load(Ordering::SeqCst) > before);

        let started = Instant::now();
        handle.stop();
        assert!(started.elapsed() < Duration::from_secs(1), "stop must not wait for MAX_IDLE");
    }
}
