//! Automation options: rules about *when* MacroMagi may act, shared by the
//! Auto Clicker and macros.
//!
//! They live in the app's settings and can travel inside a `.magi` file, so a
//! macro made for one game can bring "only ever click in that game" along.

use serde::{Deserialize, Serialize};

use crate::hotkey::Hotkey;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationOptions {
    /// A hotkey fires only when the held modifier keys match it exactly.
    /// Off: extra modifiers (Shift held while gaming) are ignored.
    pub strict_modifiers: bool,
    /// Stop everything when the user switches tasks (Alt+Tab, Win+Tab).
    pub stop_on_task_switch: bool,
    /// More key combinations that stop everything.
    pub stop_hotkeys: Vec<Hotkey>,
    /// Stop the Auto Clicker as soon as a hotkey field starts listening.
    pub stop_when_editing_hotkey: bool,
    /// Which programs MacroMagi may send input to.
    pub process_rules: ProcessRules,
}

impl Default for AutomationOptions {
    fn default() -> Self {
        Self {
            strict_modifiers: false,
            stop_on_task_switch: false,
            stop_hotkeys: Vec::new(),
            stop_when_editing_hotkey: true,
            process_rules: ProcessRules::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleMode {
    /// Only the listed programs may receive input.
    #[default]
    Whitelist,
    /// Every program except the listed ones may receive input.
    Blacklist,
}

/// What happens when input would go to a program it must not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    /// End the Auto Clicker or macro.
    #[default]
    Stop,
    /// Wait until an allowed program is in front again, then carry on.
    Pause,
}

impl RuleAction {
    pub fn label(self) -> &'static str {
        match self {
            RuleAction::Stop => "Stop",
            RuleAction::Pause => "Pause",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRule {
    /// Executable name, e.g. `game.exe`. Matched without regard to case.
    pub name: String,
    pub enabled: bool,
    /// What to do when this rule is broken. In a whitelist: when the user
    /// leaves this program. In a blacklist: when this program comes to the front.
    pub action: RuleAction,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessRules {
    pub enabled: bool,
    pub mode: RuleMode,
    pub whitelist: Vec<ProcessRule>,
    pub blacklist: Vec<ProcessRule>,
}

/// The decision for the program currently in front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Denied(RuleAction),
}

impl ProcessRules {
    pub fn list(&self, mode: RuleMode) -> &[ProcessRule] {
        match mode {
            RuleMode::Whitelist => &self.whitelist,
            RuleMode::Blacklist => &self.blacklist,
        }
    }

    pub fn list_mut(&mut self, mode: RuleMode) -> &mut Vec<ProcessRule> {
        match mode {
            RuleMode::Whitelist => &mut self.whitelist,
            RuleMode::Blacklist => &mut self.blacklist,
        }
    }

    /// Whether input may go to `foreground` (an executable name, if known).
    ///
    /// `last_allowed` is the program input last went to; in a whitelist, its
    /// rule decides what happens when the user leaves it.
    pub fn verdict(&self, foreground: Option<&str>, last_allowed: Option<&str>) -> Verdict {
        if !self.enabled {
            return Verdict::Allowed;
        }
        let Some(foreground) = foreground else {
            // The program in front cannot be determined (e.g. this platform
            // does not support it). Blocking everything would make the
            // feature a trap, so input is allowed.
            return Verdict::Allowed;
        };
        let rule = |list: &[ProcessRule], name: &str| {
            list.iter().find(|rule| rule.enabled && rule.name.eq_ignore_ascii_case(name)).cloned()
        };
        match self.mode {
            RuleMode::Whitelist => {
                if rule(&self.whitelist, foreground).is_some() {
                    return Verdict::Allowed;
                }
                let action =
                    last_allowed.and_then(|name| rule(&self.whitelist, name)).map_or(RuleAction::Stop, |r| r.action);
                Verdict::Denied(action)
            }
            RuleMode::Blacklist => match rule(&self.blacklist, foreground) {
                Some(rule) => Verdict::Denied(rule.action),
                None => Verdict::Allowed,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(name: &str, action: RuleAction) -> ProcessRule {
        ProcessRule { name: name.into(), enabled: true, action }
    }

    #[test]
    fn disabled_rules_allow_everything() {
        let rules = ProcessRules { enabled: false, ..ProcessRules::default() };
        assert_eq!(rules.verdict(Some("anything.exe"), None), Verdict::Allowed);
    }

    #[test]
    fn a_whitelist_allows_only_its_programs() {
        let rules = ProcessRules {
            enabled: true,
            mode: RuleMode::Whitelist,
            whitelist: vec![rule("Game.exe", RuleAction::Pause)],
            ..ProcessRules::default()
        };
        assert_eq!(rules.verdict(Some("game.exe"), None), Verdict::Allowed);
        assert_eq!(rules.verdict(Some("chrome.exe"), Some("game.exe")), Verdict::Denied(RuleAction::Pause));
        assert_eq!(rules.verdict(Some("chrome.exe"), None), Verdict::Denied(RuleAction::Stop));
    }

    #[test]
    fn a_blacklist_blocks_only_its_enabled_programs() {
        let mut off = rule("notepad.exe", RuleAction::Stop);
        off.enabled = false;
        let rules = ProcessRules {
            enabled: true,
            mode: RuleMode::Blacklist,
            blacklist: vec![rule("discord.exe", RuleAction::Pause), off],
            ..ProcessRules::default()
        };
        assert_eq!(rules.verdict(Some("Discord.exe"), None), Verdict::Denied(RuleAction::Pause));
        assert_eq!(rules.verdict(Some("notepad.exe"), None), Verdict::Allowed);
        assert_eq!(rules.verdict(None, None), Verdict::Allowed);
    }

    #[test]
    fn options_tolerate_missing_fields() {
        let options: AutomationOptions = serde_json::from_str(r#"{"stop_on_task_switch": true}"#).unwrap();
        assert!(options.stop_on_task_switch);
        assert!(options.stop_when_editing_hotkey);
    }
}
