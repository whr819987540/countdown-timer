//! 用户设置：保存在应用配置目录下的 `settings.json`。

use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::timer;

const DEFAULT_DURATION_MS: u64 = 25 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 上一次开始倒计时使用的时长（毫秒），下次启动时恢复
    pub duration_ms: u64,
    /// 倒计时结束时播放系统闹钟提示音
    pub sound: bool,
    /// 关闭主窗口时隐藏到托盘，而不是退出程序
    pub close_to_tray: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            duration_ms: DEFAULT_DURATION_MS,
            sound: true,
            close_to_tray: true,
        }
    }
}

impl Settings {
    /// 读取设置；文件不存在或内容损坏时使用默认值，不让配置问题阻止程序启动。
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if timer::validate_duration(settings.duration()).is_err() {
            settings.duration_ms = DEFAULT_DURATION_MS;
        }
        settings
    }

    /// 先写临时文件再重命名，避免写到一半断电导致文件损坏。
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }

    pub fn duration(&self) -> Duration {
        Duration::from_millis(self.duration_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("countdown-timer-test-{}", std::process::id()))
            .join(name)
    }

    #[test]
    fn missing_file_gives_defaults() {
        let settings = Settings::load(&temp_path("missing.json"));
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn round_trips_through_disk() {
        let path = temp_path("roundtrip.json");
        let settings = Settings {
            duration_ms: 90_000,
            sound: false,
            close_to_tray: false,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn repairs_invalid_or_partial_content() {
        let path = temp_path("partial.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{ "durationMs": 0, "sound": false }"#).unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.duration_ms, DEFAULT_DURATION_MS);
        assert!(!settings.sound);
        assert!(settings.close_to_tray);

        fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_file(path).unwrap();
    }
}
