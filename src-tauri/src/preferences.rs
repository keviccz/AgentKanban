use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    pub theme: String,
    pub always_on_top: bool,
    pub compact: bool,
    pub filter: String,
    pub collapsed_projects: Vec<i64>,
    pub expanded_projects: Vec<i64>,
    pub completed_projects: Vec<i64>,
    pub focused_project: Option<i64>,
    pub pinned_projects: Vec<i64>,
    pub stale_after_hours: u32,
    pub shortcut_enabled: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            always_on_top: true,
            compact: false,
            filter: "all".into(),
            collapsed_projects: vec![],
            expanded_projects: vec![],
            completed_projects: vec![],
            focused_project: None,
            pinned_projects: vec![],
            stale_after_hours: 24,
            shortcut_enabled: true,
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if !["light", "dark"].contains(&self.theme.as_str())
            || !["all", "attention", "in_progress", "blocked", "todo", "review"].contains(&self.filter.as_str())
            || ![0, 1, 4, 8, 24, 48, 168].contains(&self.stale_after_hours)
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
    fn v01_preferences_keep_their_values_and_receive_v02_defaults() {
        let prefs: Preferences = serde_json::from_str(
            r#"{"theme":"dark","always_on_top":false,"compact":true,"filter":"blocked","collapsed_projects":[2],"expanded_projects":[4],"completed_projects":[7]}"#,
        ).unwrap();
        assert_eq!(prefs.theme, "dark");
        assert!(!prefs.always_on_top);
        assert!(prefs.compact);
        assert_eq!(prefs.filter, "blocked");
        assert_eq!(prefs.collapsed_projects, [2]);
        assert_eq!(prefs.expanded_projects, [4]);
        assert_eq!(prefs.completed_projects, [7]);
        assert_eq!(prefs.focused_project, None);
        assert!(prefs.pinned_projects.is_empty());
        assert_eq!(prefs.stale_after_hours, 24);
        assert!(prefs.shortcut_enabled);
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
        for filter in ["attention", "blocked", "todo", "review"] {
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
