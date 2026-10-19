//! 倒计时结束的提示音：循环播放 Windows 的"闹钟"系统声音，直到用户响应或超时。

#[cfg(windows)]
mod imp {
    use std::ptr;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::LazyLock;
    use std::thread;
    use std::time::Duration;

    use windows_sys::Win32::Media::Audio::{
        PlaySoundW, SND_ALIAS, SND_ASYNC, SND_LOOP, SND_NODEFAULT,
    };

    /// 用户一直不响应时，最多响铃这么久
    const MAX_RINGING: Duration = Duration::from_secs(60);

    /// 每次开始/停止都会递增，用来让过期的超时线程失效
    static GENERATION: AtomicU64 = AtomicU64::new(0);

    // 异步播放期间系统可能仍会引用这些字符串，所以放在静态区
    static ALARM: LazyLock<Vec<u16>> = LazyLock::new(|| wide("Notification.Looping.Alarm"));
    static FALLBACK: LazyLock<Vec<u16>> = LazyLock::new(|| wide("SystemExclamation"));

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub fn start_alarm() {
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        // SAFETY: 参数是以 NUL 结尾、生命周期为 'static 的 UTF-16 字符串
        let played = unsafe {
            PlaySoundW(
                ALARM.as_ptr(),
                ptr::null_mut(),
                SND_ALIAS | SND_ASYNC | SND_LOOP | SND_NODEFAULT,
            )
        };
        if played == 0 {
            // 系统声音方案里没有"闹钟"时退回到普通提示音（只响一次）
            // SAFETY: 同上
            unsafe { PlaySoundW(FALLBACK.as_ptr(), ptr::null_mut(), SND_ALIAS | SND_ASYNC) };
            return;
        }
        thread::spawn(move || {
            thread::sleep(MAX_RINGING);
            if GENERATION.load(Ordering::SeqCst) == generation {
                stop_alarm();
            }
        });
    }

    pub fn stop_alarm() {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        // SAFETY: 传入空指针表示停止当前正在播放的声音
        unsafe { PlaySoundW(ptr::null(), ptr::null_mut(), 0) };
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn start_alarm() {}
    pub fn stop_alarm() {}
}

pub use imp::{start_alarm, stop_alarm};
