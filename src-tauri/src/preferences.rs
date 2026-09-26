use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    pub theme: String,
    pub always_on_top: bool,
    pub compact: bool,
    /// Single-line task rows; independent of the compact tray strip.
    pub concise: bool,
    pub filter: String,
    pub collapsed_projects: Vec<i64>,
    pub expanded_projects: Vec<i64>,
    pub completed_projects: Vec<i64>,
    pub focused_project: Option<i64>,
    pub pinned_projects: Vec<i64>,
    pub stale_after_hours: u32,
    pub shortcut_enabled: bool,
    /// Windows notification when a task starts needing the user.
    pub notify: bool,
    /// Archive accepted work after this many days without changes; 0 keeps it.
    pub auto_archive_days: u32,
    /// Stay in the tray when Windows starts the board at sign-in.
    pub start_hidden: bool,
    /// Whole-interface scale in percent (80-130, steps of 5).
    pub font_scale: u32,
    /// Window opacity in percent (50-100, steps of 5).
    pub opacity: u32,
    /// Checks run in the desktop process, never through Agent calls.
    pub auto_check_updates: bool,
    pub auto_download_updates: bool,
    /// Project order after pinned ones: "recent" activity or "name".
    pub project_sort: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            always_on_top: true,
            compact: false,
            concise: false,
            filter: "all".into(),
            collapsed_projects: vec![],
            expanded_projects: vec![],
            completed_projects: vec![],
            focused_project: None,
            pinned_projects: vec![],
            stale_after_hours: 24,
            shortcut_enabled: true,
            notify: true,
            auto_archive_days: 7,
            start_hidden: true,
            font_scale: 100,
            opacity: 100,
            auto_check_updates: true,
            auto_download_updates: false,
            project_sort: "recent".into(),
        }
    }
}

impl Preferences {
    /// WebView zoom reduces the available CSS viewport. Keep the full board's
    /// minimum usable area available at every supported font scale.
    pub fn minimum_window_size(&self) -> (f64, f64) {
        let zoom = f64::from(self.font_scale) / 100.0;
        let width = (320.0 * zoom).ceil().max(320.0);
        let height = if self.compact {
            (48.0 * zoom).ceil()
        } else {
            (360.0 * zoom).ceil().max(360.0)
        };
        (width, height)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !["light", "dark"].contains(&self.theme.as_str())
            || ![
                "all",
                "attention",
                "in_progress",
                "blocked",
                "todo",
                "review",
                "recent",
            ]
            .contains(&self.filter.as_str())
            || !["recent", "name"].contains(&self.project_sort.as_str())
            || ![0, 1, 4, 8, 24, 48, 168].contains(&self.stale_after_hours)
            || ![0, 1, 3, 7, 30].contains(&self.auto_archive_days)
            || !(80..=130).contains(&self.font_scale)
            || !self.font_scale.is_multiple_of(5)
            || !(50..=100).contains(&self.opacity)
            || !self.opacity.is_multiple_of(5)
            || self.focused_project.is_some_and(|id| id <= 0)
            || self.pinned_projects.iter().any(|id| *id <= 0)
        {
            return Err("无效的显示设置".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoomed_windows_keep_a_usable_css_viewport() {
        for font_scale in (80..=130).step_by(5) {
            let zoom = f64::from(font_scale) / 100.0;
            for compact in [false, true] {
                let prefs = Preferences {
                    font_scale,
                    compact,
                    ..Default::default()
                };
                let (width, height) = prefs.minimum_window_size();
                assert!(width / zoom >= 320.0);
                assert!(height / zoom >= if compact { 48.0 } else { 360.0 });
            }
        }
    }

    #[test]
    fn v01_preferences_keep_their_values_and_receive_v02_defaults() {
        let prefs: Preferences = serde_json::from_str(
            r#"{"theme":"dark","always_on_top":false,"compact":true,"filter":"blocked","collapsed_projects":[2],"expanded_projects":[4],"completed_projects":[7]}"#,
        ).unwrap();
        assert_eq!(prefs.theme, "dark");
        assert!(!prefs.always_on_top);
        assert!(prefs.compact);
        assert!(!prefs.concise);
        assert_eq!(prefs.filter, "blocked");
        assert_eq!(prefs.collapsed_projects, [2]);
        assert_eq!(prefs.expanded_projects, [4]);
        assert_eq!(prefs.completed_projects, [7]);
        assert_eq!(prefs.focused_project, None);
        assert!(prefs.pinned_projects.is_empty());
        assert_eq!(prefs.stale_after_hours, 24);
        assert!(prefs.shortcut_enabled);
        assert!(prefs.notify && prefs.start_hidden);
        assert_eq!(prefs.auto_archive_days, 7);
        assert_eq!((prefs.font_scale, prefs.opacity), (100, 100));
        assert!(prefs.auto_check_updates);
        assert!(!prefs.auto_download_updates);
        prefs.validate().unwrap();
    }

    #[test]
    fn allowed_staleness_choices_include_disabled_and_reject_invalid_values() {
        for hours in [0, 1, 4, 8, 24, 48, 168] {
            Preferences {
                stale_after_hours: hours,
                ..Default::default()
            }
            .validate()
            .unwrap();
        }
        assert!(Preferences {
            stale_after_hours: 3,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(Preferences {
            focused_project: Some(0),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(Preferences {
            pinned_projects: vec![-1],
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn attention_filter_is_valid_and_legacy_filters_still_load() {
        for filter in ["attention", "blocked", "todo", "review", "recent"] {
            Preferences {
                filter: filter.into(),
                ..Default::default()
            }
            .validate()
            .unwrap();
        }
    }

    #[test]
    fn review_filter_is_valid_without_changing_previous_preferences() {
        let prefs: Preferences = serde_json::from_str(r#"{"filter":"review"}"#).unwrap();
        prefs.validate().unwrap();
        assert_eq!(prefs.filter, "review");
        assert!(prefs.shortcut_enabled);
        assert_eq!(prefs.stale_after_hours, 24);
        assert!(Preferences {
            filter: "accepted".into(),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
