//! Countdown and stopwatch driven by a monotonic clock (no tick accumulation).

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Running,
    Paused,
    Finished,
}

/// Elapsed-time accumulator shared by both clocks.
#[derive(Debug, Clone)]
struct Clock {
    state: RunState,
    started: Option<Instant>,
    banked: Duration,
}

impl Clock {
    fn new() -> Clock {
        Clock { state: RunState::Idle, started: None, banked: Duration::ZERO }
    }
    fn elapsed(&self, now: Instant) -> Duration {
        self.banked + self.started.map(|s| now.saturating_duration_since(s)).unwrap_or_default()
    }
    fn start(&mut self, now: Instant) {
        if self.state != RunState::Running {
            self.started = Some(now);
            self.state = RunState::Running;
        }
    }
    fn pause(&mut self, now: Instant) {
        if self.state == RunState::Running {
            self.banked = self.elapsed(now);
            self.started = None;
            self.state = RunState::Paused;
        }
    }
    fn reset(&mut self) {
        *self = Clock::new();
    }
}

#[derive(Debug, Clone)]
pub struct Countdown {
    clock: Clock,
    pub total: Duration,
}

impl Countdown {
    pub fn new(total: Duration) -> Countdown {
        Countdown { clock: Clock::new(), total }
    }
    pub fn state(&self) -> RunState {
        self.clock.state
    }
    pub fn set_total(&mut self, total: Duration) {
        self.total = total.min(Duration::from_secs(99 * 60 + 59));
        self.clock.reset();
    }
    pub fn remaining(&self, now: Instant) -> Duration {
        self.total.saturating_sub(self.clock.elapsed(now))
    }
    pub fn start(&mut self, now: Instant) {
        if self.clock.state == RunState::Finished {
            self.clock.reset();
        }
        if self.total > Duration::ZERO {
            self.clock.start(now);
        }
    }
    pub fn pause(&mut self, now: Instant) {
        self.clock.pause(now)
    }
    pub fn toggle(&mut self, now: Instant) {
        match self.clock.state {
            RunState::Running => self.pause(now),
            _ => self.start(now),
        }
    }
    pub fn reset(&mut self) {
        self.clock.reset()
    }
    /// Advance; returns true exactly once, when the countdown reaches zero.
    pub fn update(&mut self, now: Instant) -> bool {
        if self.clock.state == RunState::Running && self.remaining(now).is_zero() {
            self.clock.banked = self.total;
            self.clock.started = None;
            self.clock.state = RunState::Finished;
            return true;
        }
        false
    }
    /// Time until the displayed second changes (for scheduling redraws).
    pub fn next_tick_in(&self, now: Instant) -> Option<Duration> {
        if self.clock.state != RunState::Running {
            return None;
        }
        let rem = self.remaining(now);
        let frac = rem.as_nanos() % 1_000_000_000;
        Some(Duration::from_nanos(frac as u64).max(Duration::from_millis(5)))
    }
}

#[derive(Debug, Clone)]
pub struct Stopwatch {
    clock: Clock,
    pub laps: Vec<Duration>,
}

impl Default for Stopwatch {
    fn default() -> Self {
        Stopwatch { clock: Clock::new(), laps: Vec::new() }
    }
}

impl Stopwatch {
    pub fn state(&self) -> RunState {
        self.clock.state
    }
    pub fn elapsed(&self, now: Instant) -> Duration {
        self.clock.elapsed(now)
    }
    pub fn start(&mut self, now: Instant) {
        self.clock.start(now)
    }
    pub fn pause(&mut self, now: Instant) {
        self.clock.pause(now)
    }
    pub fn toggle(&mut self, now: Instant) {
        match self.clock.state {
            RunState::Running => self.pause(now),
            _ => self.start(now),
        }
    }
    pub fn lap(&mut self, now: Instant) {
        if self.clock.state == RunState::Running {
            self.laps.push(self.elapsed(now));
        }
    }
    pub fn reset(&mut self) {
        self.clock.reset();
        self.laps.clear();
    }
}

/// Adjust a countdown duration by whole minutes (0..=99, clamped) and/or
/// seconds (0..=59, wrapping without carrying into the minutes).
pub fn adjust_mmss(total: Duration, dmin: i32, dsec: i32) -> Duration {
    let t = total.as_secs() as i64;
    let m = (t / 60 + dmin as i64).clamp(0, 99);
    let sec = (t % 60 + dsec as i64).rem_euclid(60);
    Duration::from_secs((m * 60 + sec) as u64)
}

/// "MM:SS" (or "H:MM:SS" past an hour).
pub fn format_mmss(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// Countdown display rounds up so "00:01" is shown until zero is reached.
pub fn format_countdown(d: Duration) -> String {
    let ms = d.as_millis() as u64;
    format_mmss(Duration::from_secs(ms.div_ceil(1000)))
}

/// Stopwatch display with tenths: "MM:SS.t".
pub fn format_stopwatch(d: Duration) -> String {
    format!("{}.{}", format_mmss(d), (d.subsec_millis() / 100))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn countdown_no_drift() {
        let t0 = Instant::now();
        let mut c = Countdown::new(Duration::from_secs(60));
        c.start(t0);
        assert_eq!(c.remaining(t0 + Duration::from_millis(30_500)), Duration::from_millis(29_500));
        c.pause(t0 + Duration::from_secs(40));
        assert_eq!(c.remaining(t0 + Duration::from_secs(100)), Duration::from_secs(20));
        c.start(t0 + Duration::from_secs(100));
        assert!(!c.update(t0 + Duration::from_secs(119)));
        assert!(c.update(t0 + Duration::from_secs(121)));
        assert_eq!(c.state(), RunState::Finished);
        assert!(!c.update(t0 + Duration::from_secs(122)));
    }
    #[test]
    fn stopwatch_independent() {
        let t0 = Instant::now();
        let mut s = Stopwatch::default();
        s.start(t0);
        s.lap(t0 + Duration::from_secs(3));
        s.pause(t0 + Duration::from_secs(5));
        assert_eq!(s.elapsed(t0 + Duration::from_secs(50)), Duration::from_secs(5));
        assert_eq!(s.laps.len(), 1);
        s.reset();
        assert_eq!(s.elapsed(t0), Duration::ZERO);
    }
    #[test]
    fn minutes_and_seconds_adjust_separately() {
        let d = |s| Duration::from_secs(s);
        assert_eq!(adjust_mmss(d(5 * 60), 1, 0), d(6 * 60));
        assert_eq!(adjust_mmss(d(5 * 60), 0, 1), d(5 * 60 + 1));
        assert_eq!(adjust_mmss(d(5 * 60), 0, -1), d(5 * 60 + 59));
        assert_eq!(adjust_mmss(d(5 * 60 + 59), 0, 1), d(5 * 60));
        assert_eq!(adjust_mmss(d(30), -1, 0), d(30));
        assert_eq!(adjust_mmss(d(99 * 60 + 10), 1, 0), d(99 * 60 + 10));
    }
    #[test]
    fn formats() {
        assert_eq!(format_countdown(Duration::from_millis(59_001)), "01:00");
        assert_eq!(format_countdown(Duration::from_millis(900)), "00:01");
        assert_eq!(format_stopwatch(Duration::from_millis(65_430)), "01:05.4");
        assert_eq!(format_mmss(Duration::from_secs(3725)), "1:02:05");
    }
}
