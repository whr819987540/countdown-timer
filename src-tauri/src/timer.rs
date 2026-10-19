//! 倒计时状态机。
//!
//! 这里只有纯逻辑：所有与时间有关的方法都显式接收 `now`，方便单元测试，
//! 也让计时完全基于单调时钟（`Instant`），不受窗口隐藏或系统改时间的影响。

use std::fmt;
use std::time::{Duration, Instant};

use serde::Serialize;

/// 允许设置的最长时长：99:59:59。
pub const MAX_DURATION: Duration = Duration::from_secs(99 * 3600 + 59 * 60 + 59);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// 未开始，或已被停止
    Idle,
    Running,
    Paused,
    /// 本轮已结束，等待用户选择"重置"（再来一轮）或"停止"
    Finished,
}

/// 推送给前端和托盘的状态快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub phase: Phase,
    pub duration_ms: u64,
    pub remaining_ms: u64,
    /// 本次会话中已完成的轮数，停止后清零
    pub rounds: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerError {
    InvalidDuration,
    InvalidTransition { from: Phase, action: &'static str },
}

impl fmt::Display for TimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDuration => write!(f, "时长必须在 1 秒到 99:59:59 之间"),
            Self::InvalidTransition { from, action } => {
                write!(f, "当前状态 {from:?} 下不能执行 {action}")
            }
        }
    }
}

impl std::error::Error for TimerError {}

#[derive(Debug)]
pub struct Timer {
    phase: Phase,
    duration: Duration,
    /// 非运行状态下的剩余时长
    remaining: Duration,
    /// 运行状态下本轮的结束时刻
    deadline: Option<Instant>,
    rounds: u32,
}

impl Timer {
    pub fn new(duration: Duration) -> Self {
        let duration = validate_duration(duration).unwrap_or(Duration::from_secs(25 * 60));
        Self {
            phase: Phase::Idle,
            duration,
            remaining: duration,
            deadline: None,
            rounds: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 以给定时长开始新的一次计时，轮数清零。仅在空闲或已结束时可用。
    pub fn start(&mut self, duration: Duration, now: Instant) -> Result<(), TimerError> {
        if !matches!(self.phase, Phase::Idle | Phase::Finished) {
            return Err(self.invalid("start"));
        }
        self.duration = validate_duration(duration)?;
        self.rounds = 0;
        self.run_for(self.duration, now);
        Ok(())
    }

    pub fn pause(&mut self, now: Instant) -> Result<(), TimerError> {
        if self.phase != Phase::Running {
            return Err(self.invalid("pause"));
        }
        self.remaining = self.remaining(now);
        self.deadline = None;
        self.phase = Phase::Paused;
        Ok(())
    }

    pub fn resume(&mut self, now: Instant) -> Result<(), TimerError> {
        if self.phase != Phase::Paused {
            return Err(self.invalid("resume"));
        }
        self.run_for(self.remaining, now);
        Ok(())
    }

    /// "重置"：以当前时长立即重新开始一轮，保留已完成的轮数。
    pub fn restart(&mut self, now: Instant) {
        self.run_for(self.duration, now);
    }

    /// 停止并回到空闲状态，轮数清零。
    pub fn stop(&mut self) {
        self.phase = Phase::Idle;
        self.remaining = self.duration;
        self.deadline = None;
        self.rounds = 0;
    }

    /// 推进时间。若本轮在 `now` 时已到期，切换为 [`Phase::Finished`] 并返回 `true`。
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.deadline {
            Some(deadline) if self.phase == Phase::Running && now >= deadline => {
                self.phase = Phase::Finished;
                self.remaining = Duration::ZERO;
                self.deadline = None;
                self.rounds = self.rounds.saturating_add(1);
                true
            }
            _ => false,
        }
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        match self.deadline {
            Some(deadline) => deadline.saturating_duration_since(now),
            None => self.remaining,
        }
    }

    pub fn snapshot(&self, now: Instant) -> Snapshot {
        Snapshot {
            phase: self.phase,
            duration_ms: duration_ms(self.duration),
            remaining_ms: duration_ms(self.remaining(now)),
            rounds: self.rounds,
        }
    }

    /// 距离下一次需要唤醒的时长：显示的整秒数变化或本轮结束。非运行状态返回 `None`。
    pub fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        if self.phase != Phase::Running {
            return None;
        }
        let remaining = self.remaining(now);
        // 显示值按秒向上取整，所以剩余时长每跨过一个整秒边界，显示才会变化
        let fraction = Duration::from_nanos(u64::from(remaining.subsec_nanos()));
        let step = if fraction.is_zero() {
            Duration::from_secs(1)
        } else {
            fraction
        };
        Some(step.min(remaining))
    }

    fn run_for(&mut self, remaining: Duration, now: Instant) {
        self.phase = Phase::Running;
        self.remaining = remaining;
        self.deadline = Some(now + remaining);
    }

    fn invalid(&self, action: &'static str) -> TimerError {
        TimerError::InvalidTransition {
            from: self.phase,
            action,
        }
    }
}

pub fn validate_duration(duration: Duration) -> Result<Duration, TimerError> {
    if duration < Duration::from_secs(1) || duration > MAX_DURATION {
        Err(TimerError::InvalidDuration)
    } else {
        Ok(duration)
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// 把毫秒格式化为 `MM:SS`，超过一小时为 `H:MM:SS`；秒数向上取整，与界面一致。
pub fn format_clock(ms: u64) -> String {
    let total = ms.div_ceil(1000);
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Duration = Duration::from_secs(1);

    fn running(secs: u64) -> (Timer, Instant) {
        let now = Instant::now();
        let mut timer = Timer::new(SEC * 60);
        timer.start(SEC * secs as u32, now).unwrap();
        (timer, now)
    }

    #[test]
    fn counts_down_while_running() {
        let (timer, t0) = running(10);
        assert_eq!(timer.phase(), Phase::Running);
        assert_eq!(timer.remaining(t0 + SEC * 3), SEC * 7);
        assert_eq!(timer.snapshot(t0 + SEC * 3).remaining_ms, 7000);
    }

    #[test]
    fn pause_freezes_and_resume_continues() {
        let (mut timer, t0) = running(10);
        timer.pause(t0 + SEC * 4).unwrap();
        assert_eq!(timer.phase(), Phase::Paused);
        // 暂停期间时间流逝不影响剩余时长
        assert_eq!(timer.remaining(t0 + SEC * 100), SEC * 6);
        timer.resume(t0 + SEC * 100).unwrap();
        assert_eq!(timer.remaining(t0 + SEC * 101), SEC * 5);
    }

    #[test]
    fn tick_finishes_at_deadline_and_counts_rounds() {
        let (mut timer, t0) = running(5);
        assert!(!timer.tick(t0 + SEC * 4));
        assert!(timer.tick(t0 + SEC * 5));
        assert_eq!(timer.phase(), Phase::Finished);
        assert_eq!(timer.snapshot(t0 + SEC * 6).rounds, 1);
        // 已结束后不会重复计数
        assert!(!timer.tick(t0 + SEC * 7));
        assert_eq!(timer.snapshot(t0 + SEC * 7).rounds, 1);
    }

    #[test]
    fn restart_repeats_with_same_duration_and_keeps_rounds() {
        let (mut timer, t0) = running(5);
        timer.tick(t0 + SEC * 5);
        timer.restart(t0 + SEC * 8);
        let snap = timer.snapshot(t0 + SEC * 8);
        assert_eq!(snap.phase, Phase::Running);
        assert_eq!(snap.remaining_ms, 5000);
        assert_eq!(snap.rounds, 1);
        assert!(timer.tick(t0 + SEC * 13));
        assert_eq!(timer.snapshot(t0 + SEC * 13).rounds, 2);
    }

    #[test]
    fn stop_resets_everything() {
        let (mut timer, t0) = running(5);
        timer.tick(t0 + SEC * 5);
        timer.stop();
        let snap = timer.snapshot(t0 + SEC * 6);
        assert_eq!(snap.phase, Phase::Idle);
        assert_eq!(snap.remaining_ms, 5000);
        assert_eq!(snap.rounds, 0);
    }

    #[test]
    fn rejects_invalid_transitions() {
        let now = Instant::now();
        let mut timer = Timer::new(SEC * 60);
        assert!(timer.pause(now).is_err());
        assert!(timer.resume(now).is_err());
        timer.start(SEC * 10, now).unwrap();
        assert!(timer.start(SEC * 10, now).is_err());
        assert!(timer.resume(now).is_err());
    }

    #[test]
    fn rejects_invalid_durations() {
        let now = Instant::now();
        let mut timer = Timer::new(SEC * 60);
        assert_eq!(
            timer.start(Duration::ZERO, now),
            Err(TimerError::InvalidDuration)
        );
        assert_eq!(
            timer.start(MAX_DURATION + SEC, now),
            Err(TimerError::InvalidDuration)
        );
        assert!(timer.start(MAX_DURATION, now).is_ok());
        // 非法的初始时长回退为默认值
        assert_eq!(
            Timer::new(Duration::ZERO).snapshot(now).duration_ms,
            25 * 60 * 1000
        );
    }

    #[test]
    fn wakes_up_on_each_displayed_second() {
        let (timer, t0) = running(10);
        assert_eq!(timer.next_wakeup(t0), Some(SEC));
        assert_eq!(
            timer.next_wakeup(t0 + Duration::from_millis(300)),
            Some(Duration::from_millis(700))
        );
        assert_eq!(timer.next_wakeup(t0 + SEC * 10), Some(Duration::ZERO));
        assert_eq!(Timer::new(SEC).next_wakeup(t0), None);
    }

    #[test]
    fn formats_clock_rounding_up() {
        assert_eq!(format_clock(0), "00:00");
        assert_eq!(format_clock(1), "00:01");
        assert_eq!(format_clock(59_001), "01:00");
        assert_eq!(format_clock(25 * 60_000), "25:00");
        assert_eq!(format_clock(3_600_000), "1:00:00");
        assert_eq!(format_clock(MAX_DURATION.as_millis() as u64), "99:59:59");
    }
}
