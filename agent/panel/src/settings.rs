//! Widget preferences in `<data>/widget.json`: whether it stays on top and
//! whether it is shown. (`x`/`y` are kept for compatibility; the widget always
//! starts in the top-right corner.)

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetSettings {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub on_top: bool,
    pub visible: bool,
}

impl Default for WidgetSettings {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            on_top: true,
            visible: true,
        }
    }
}

impl WidgetSettings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("widget.json")
    }

    pub fn load(data_dir: &Path) -> Self {
        std::fs::read(Self::path(data_dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) {
        if std::fs::create_dir_all(data_dir).is_ok() {
            let tmp = Self::path(data_dir).with_extension("json.tmp");
            if let Ok(bytes) = serde_json::to_vec_pretty(self)
                && std::fs::write(&tmp, bytes).is_ok()
            {
                let _ = std::fs::rename(&tmp, Self::path(data_dir));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(WidgetSettings::load(dir.path()), WidgetSettings::default());
        let s = WidgetSettings {
            x: Some(10.0),
            y: Some(20.0),
            on_top: false,
            visible: false,
        };
        s.save(dir.path());
        assert_eq!(WidgetSettings::load(dir.path()), s);
    }
}
