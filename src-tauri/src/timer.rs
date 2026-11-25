//! 倒计时状态机。
//!
//! 这里只有纯逻辑：所有与时间有关的方法都显式接收 `now`，方便单元测试，
//! 也让计时完全基于单调时钟（`Instant`），不受窗口隐藏或系统改时间的影响。
//!
//! 一次计时由若干"段"组成：普通模式每段都是用户设置的时长；20-20-20 护眼模式在
//! 20 分钟专注（[`Stage::Work`]）和 20 秒远眺休息（[`Stage::Rest`]）之间交替。
//! 每段结束后都停在 [`Phase::Finished`]，由用户手动 [`Timer::advance`] 进入下一段。

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// 允许设置的最长时长：99:59:59。
pub const MAX_DURATION: Duration = Duration::from_secs(99 * 3600 + 59 * 60 + 59);

/// 20-20-20 模式的专注时长
pub const EYE_CARE_WORK: Duration = Duration::from_secs(20 * 60);
/// 20-20-20 模式的远眺休息时长
pub const EYE_CARE_REST: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// 普通倒计时：每轮都使用用户设置的时长
    #[default]
    Normal,
    /// 20-20-20 护眼：专注 20 分钟 → 远眺 20 秒 → 专注 20 分钟 ……
    EyeCare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// 普通模式的倒计时，或护眼模式的专注段
    Work,
    /// 护眼模式的远眺休息段
    Rest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// 未开始，或已被停止
    Idle,
    Running,
    Paused,
    /// 本段已结束，等待用户选择进入下一段（[`Timer::advance`]）或停止
    Finished,
}

/// 推送给前端和托盘的状态快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub mode: Mode,
    pub stage: Stage,
    pub phase: Phase,
    /// 当前段的总时长
    pub duration_ms: u64,
    pub remaining_ms: u64,
    /// 本次会话中已完成的轮数（护眼模式只统计专注段），停止后清零
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
    mode: Mode,
    stage: Stage,
    phase: Phase,
    /// 普通模式的时长（护眼模式下保持不变，切回普通模式时继续使用）
    duration: Duration,
    /// 非运行状态下的剩余时长
    remaining: Duration,
    /// 运行状态下本段的结束时刻
    deadline: Option<Instant>,
    rounds: u32,
}

impl Timer {
    pub fn new(duration: Duration, mode: Mode) -> Self {
        let duration = validate_duration(duration).unwrap_or(Duration::from_secs(25 * 60));
        let mut timer = Self {
            mode,
            stage: Stage::Work,
            phase: Phase::Idle,
            duration,
            remaining: duration,
            deadline: None,
            rounds: 0,
        };
        timer.remaining = timer.segment();
        timer
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 切换计时模式。仅在空闲时可用。
    pub fn set_mode(&mut self, mode: Mode) -> Result<(), TimerError> {
        if self.phase != Phase::Idle {
            return Err(self.invalid("set_mode"));
        }
        self.mode = mode;
        self.stage = Stage::Work;
        self.remaining = self.segment();
        Ok(())
    }

    /// 开始新的一次计时，轮数清零。仅在空闲或已结束时可用。
    ///
    /// 给出 `duration` 时更新普通模式的时长；省略时沿用上次的时长。
    /// 护眼模式的时长固定，总是从 20 分钟专注开始。
    pub fn start(&mut self, duration: Option<Duration>, now: Instant) -> Result<(), TimerError> {
        if !matches!(self.phase, Phase::Idle | Phase::Finished) {
            return Err(self.invalid("start"));
        }
        if let Some(duration) = duration {
            self.duration = validate_duration(duration)?;
        }
        self.rounds = 0;
        self.stage = Stage::Work;
        self.run_for(self.segment(), now);
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

    /// 本段结束后进入下一段，保留已完成的轮数。仅在已结束时可用。
    ///
    /// 普通模式即"重置"：以相同时长再来一轮；护眼模式在专注与休息之间切换。
    pub fn advance(&mut self, now: Instant) -> Result<(), TimerError> {
        if self.phase != Phase::Finished {
            return Err(self.invalid("advance"));
        }
        self.stage = match (self.mode, self.stage) {
            (Mode::EyeCare, Stage::Work) => Stage::Rest,
            _ => Stage::Work,
        };
        self.run_for(self.segment(), now);
        Ok(())
    }

    /// 停止并回到空闲状态，轮数清零。
    pub fn stop(&mut self) {
        self.phase = Phase::Idle;
        self.stage = Stage::Work;
        self.remaining = self.segment();
        self.deadline = None;
        self.rounds = 0;
    }

    /// 推进时间。若本段在 `now` 时已到期，切换为 [`Phase::Finished`] 并返回 `true`。
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.deadline {
            Some(deadline) if self.phase == Phase::Running && now >= deadline => {
                self.phase = Phase::Finished;
                self.remaining = Duration::ZERO;
                self.deadline = None;
                if self.stage == Stage::Work {
                    self.rounds = self.rounds.saturating_add(1);
                }
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
            mode: self.mode,
            stage: self.stage,
            phase: self.phase,
            duration_ms: duration_ms(self.segment()),
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

    /// 当前段的总时长
    fn segment(&self) -> Duration {
        match (self.mode, self.stage) {
            (Mode::Normal, _) => self.duration,
            (Mode::EyeCare, Stage::Work) => EYE_CARE_WORK,
            (Mode::EyeCare, Stage::Rest) => EYE_CARE_REST,
        }
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
        let mut timer = Timer::new(SEC * 60, Mode::Normal);
        timer.start(Some(SEC * secs as u32), now).unwrap();
        (timer, now)
    }

    fn eye_care() -> (Timer, Instant) {
        let now = Instant::now();
        let mut timer = Timer::new(SEC * 60, Mode::EyeCare);
        timer.start(None, now).unwrap();
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
    fn advance_repeats_with_same_duration_and_keeps_rounds() {
        let (mut timer, t0) = running(5);
        timer.tick(t0 + SEC * 5);
        timer.advance(t0 + SEC * 8).unwrap();
        let snap = timer.snapshot(t0 + SEC * 8);
        assert_eq!(snap.phase, Phase::Running);
        assert_eq!(snap.stage, Stage::Work);
        assert_eq!(snap.remaining_ms, 5000);
        assert_eq!(snap.rounds, 1);
        assert!(timer.tick(t0 + SEC * 13));
        assert_eq!(timer.snapshot(t0 + SEC * 13).rounds, 2);
    }

    #[test]
    fn start_without_duration_reuses_last_one() {
        let (mut timer, t0) = running(5);
        timer.stop();
        timer.start(None, t0).unwrap();
        assert_eq!(timer.snapshot(t0).duration_ms, 5000);
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
        let mut timer = Timer::new(SEC * 60, Mode::Normal);
        assert!(timer.pause(now).is_err());
        assert!(timer.resume(now).is_err());
        assert!(timer.advance(now).is_err());
        timer.start(Some(SEC * 10), now).unwrap();
        assert!(timer.start(Some(SEC * 10), now).is_err());
        assert!(timer.resume(now).is_err());
        assert!(timer.advance(now).is_err());
        assert!(timer.set_mode(Mode::EyeCare).is_err());
    }

    #[test]
    fn rejects_invalid_durations() {
        let now = Instant::now();
        let mut timer = Timer::new(SEC * 60, Mode::Normal);
        assert_eq!(
            timer.start(Some(Duration::ZERO), now),
            Err(TimerError::InvalidDuration)
        );
        assert_eq!(
            timer.start(Some(MAX_DURATION + SEC), now),
            Err(TimerError::InvalidDuration)
        );
        assert!(timer.start(Some(MAX_DURATION), now).is_ok());
        // 非法的初始时长回退为默认值
        assert_eq!(
            Timer::new(Duration::ZERO, Mode::Normal)
                .snapshot(now)
                .duration_ms,
            25 * 60 * 1000
        );
    }

    #[test]
    fn eye_care_alternates_work_and_rest() {
        let (mut timer, t0) = eye_care();
        let snap = timer.snapshot(t0);
        assert_eq!((snap.stage, snap.phase), (Stage::Work, Phase::Running));
        assert_eq!(snap.duration_ms, 20 * 60 * 1000);

        // 专注结束：停下等待用户确认，计一轮
        let t1 = t0 + EYE_CARE_WORK;
        assert!(timer.tick(t1));
        let snap = timer.snapshot(t1);
        assert_eq!(
            (snap.stage, snap.phase, snap.rounds),
            (Stage::Work, Phase::Finished, 1)
        );

        // 确认后开始 20 秒休息
        let t2 = t1 + SEC * 3;
        timer.advance(t2).unwrap();
        let snap = timer.snapshot(t2);
        assert_eq!((snap.stage, snap.phase), (Stage::Rest, Phase::Running));
        assert_eq!((snap.duration_ms, snap.remaining_ms), (20_000, 20_000));

        // 休息结束：同样停下等待用户手动开始下一轮，休息不计轮数
        let t3 = t2 + EYE_CARE_REST;
        assert!(timer.tick(t3));
        let snap = timer.snapshot(t3);
        assert_eq!(
            (snap.stage, snap.phase, snap.rounds),
            (Stage::Rest, Phase::Finished, 1)
        );

        timer.advance(t3).unwrap();
        let snap = timer.snapshot(t3);
        assert_eq!((snap.stage, snap.phase), (Stage::Work, Phase::Running));
        assert_eq!(snap.remaining_ms, 20 * 60 * 1000);
        assert_eq!(snap.rounds, 1);
    }

    #[test]
    fn eye_care_rest_can_pause_and_stop() {
        let (mut timer, t0) = eye_care();
        timer.tick(t0 + EYE_CARE_WORK);
        let t1 = t0 + EYE_CARE_WORK;
        timer.advance(t1).unwrap();
        timer.pause(t1 + SEC * 5).unwrap();
        assert_eq!(timer.remaining(t1 + SEC * 60), SEC * 15);

        timer.stop();
        let snap = timer.snapshot(t1);
        assert_eq!(
            (snap.mode, snap.stage, snap.phase),
            (Mode::EyeCare, Stage::Work, Phase::Idle)
        );
        assert_eq!((snap.remaining_ms, snap.rounds), (20 * 60 * 1000, 0));
    }

    #[test]
    fn switching_mode_keeps_normal_duration() {
        let now = Instant::now();
        let mut timer = Timer::new(SEC * 90, Mode::Normal);
        timer.set_mode(Mode::EyeCare).unwrap();
        let snap = timer.snapshot(now);
        assert_eq!(
            (snap.mode, snap.duration_ms, snap.remaining_ms),
            (Mode::EyeCare, 1_200_000, 1_200_000)
        );
        // 护眼模式固定从 20 分钟专注开始；切回普通模式后仍是原来的时长
        timer.start(None, now).unwrap();
        assert_eq!(timer.snapshot(now).remaining_ms, 1_200_000);
        timer.stop();
        timer.set_mode(Mode::Normal).unwrap();
        assert_eq!(timer.snapshot(now).duration_ms, 90_000);
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
        assert_eq!(Timer::new(SEC, Mode::Normal).next_wakeup(t0), None);
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
