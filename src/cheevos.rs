//! RetroAchievements, delivered through the launch config layer.
//!
//! Nothing here talks to retroachievements.org. RetroArch already implements
//! the whole protocol; it just needs three keys set, and the login token it
//! already stores is reusable — logging in through RetroArch once puts
//! `cheevos_token` in its own config, and that token is what these settings
//! ride on.
//!
//! The trap is `cheevos_username`. RetroArch writes the token and password on
//! login but can be left with an empty username, and with no username the token
//! authenticates nothing — achievements stay silently off even though the
//! credentials are right there. That is the state this machine was in.
//!
//! ## Hardcore mode
//!
//! `cheevos_hardcore_mode_enable` disables save states, fast-forward and
//! rewind, because those are how you would cheat an achievement. Four of the
//! gamepad hotkeys this app ships are exactly those functions, so turning
//! hardcore on silently breaks Select+LB, Select+RB, Select+RT and the save
//! slot d-pad bindings.
//!
//! It is therefore written explicitly on every launch rather than inherited.
//! An install with `cheevos_hardcore_mode_enable = "true"` sitting in its own
//! config — which is RetroArch's default once you enable achievements — would
//! otherwise take those hotkeys away with no indication why.

/// What the user configured under `[cheevos]`.
#[derive(Debug, Default, Clone)]
pub struct Settings {
    pub enabled: bool,
    /// RetroAchievements account name. Required: the token alone does nothing.
    pub username: Option<String>,
    /// Login token. Normally absent here and inherited from the user's own
    /// `retroarch.cfg`, which is where RetroArch put it at login.
    pub token: Option<String>,
    /// Disables save states, fast-forward and rewind. Off unless asked for.
    pub hardcore: bool,
    /// Unofficial/test achievement sets.
    pub test_unofficial: bool,
}

impl Settings {
    /// True when there is enough to actually authenticate.
    ///
    /// A username is required even though the token does the authenticating,
    /// because RetroArch sends the pair. Enabling without one produces a login
    /// failure on every launch and no achievements.
    pub fn usable(&self) -> bool {
        self.enabled && self.username.as_deref().is_some_and(|u| !u.trim().is_empty())
    }
}

/// RetroArch settings for the generated launch config.
///
/// Returns an empty string when achievements are off, so the user's own
/// `retroarch.cfg` is left to decide — this app not being configured for
/// achievements must not turn off achievements someone set up themselves.
pub fn config_lines(s: &Settings) -> String {
    if !s.enabled {
        return String::new();
    }
    if !s.usable() {
        // Enabled but unusable. Say so in the file rather than writing a
        // half-configured login that fails on every launch.
        return "\n# ---- RetroAchievements ----\n\
                # [cheevos] enabled = true, but no username is set, and the token\n\
                # authenticates nothing without one. Left untouched.\n"
            .to_owned();
    }

    let mut out = String::from(
        "\n# ---- RetroAchievements ----\n\
         # The token comes from your own retroarch.cfg, written when you logged\n\
         # in through RetroArch. It is not stored by this app.\n\
         cheevos_enable = \"true\"\n",
    );
    if let Some(u) = s.username.as_deref().map(str::trim) {
        out.push_str(&format!("cheevos_username = \"{}\"\n", escape(u)));
    }
    if let Some(t) = s.token.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        out.push_str(&format!("cheevos_token = \"{}\"\n", escape(t)));
    }

    // Always explicit — see the module docs on why inheriting this is a trap.
    out.push_str(&format!(
        "\n# Hardcore disables save states, fast-forward and rewind, which are\n\
         # four of the gamepad hotkeys this app binds. Written on every launch\n\
         # so an install that has it on cannot take them away unannounced.\n\
         cheevos_hardcore_mode_enable = \"{}\"\n",
        s.hardcore
    ));
    if s.test_unofficial {
        out.push_str("cheevos_test_unofficial = \"true\"\n");
    }
    out
}

/// A line for the launch output, so it is visible that achievements are on and
/// what that costs.
pub fn describe(s: &Settings) -> Option<String> {
    if !s.enabled {
        return None;
    }
    if !s.usable() {
        return Some(
            "achievements: enabled but no username set — add [cheevos] username".to_owned(),
        );
    }
    let who = s.username.as_deref().unwrap_or("").trim();
    Some(if s.hardcore {
        format!("achievements: {who} (HARDCORE — save states and fast-forward are disabled)")
    } else {
        format!("achievements: {who}")
    })
}

/// RetroArch config values are double-quoted, so a quote or backslash in one
/// would end the value early.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(username: Option<&str>) -> Settings {
        Settings {
            enabled: true,
            username: username.map(str::to_owned),
            ..Default::default()
        }
    }

    /// Off means untouched. Someone who set achievements up in RetroArch and
    /// never configured this app must not have them switched off by it.
    #[test]
    fn disabled_writes_nothing_at_all() {
        assert_eq!(config_lines(&Settings::default()), "");
        assert!(describe(&Settings::default()).is_none());
    }

    /// The exact state this machine was in: a valid token, no username. The
    /// token authenticates nothing without one, so writing a half-configured
    /// login would fail on every launch with no explanation.
    #[test]
    fn a_token_without_a_username_is_refused_and_explained() {
        let mut s = on(None);
        s.token = Some("abc123".to_owned());
        assert!(!s.usable());

        let out = config_lines(&s);
        assert!(!out.contains("cheevos_enable = \"true\""), "must not half-enable");
        assert!(!out.contains("abc123"), "and must not write the token: {out}");
        assert!(describe(&s).unwrap().contains("no username"));

        // Whitespace is not a username either.
        s.username = Some("   ".to_owned());
        assert!(!s.usable());
    }

    /// The normal case: username here, token inherited from the user's own
    /// retroarch.cfg. This app never stores the token.
    #[test]
    fn a_username_alone_is_enough_because_the_token_is_inherited() {
        let s = on(Some("frank"));
        assert!(s.usable());
        let out = config_lines(&s);
        assert!(out.contains("cheevos_enable = \"true\""));
        assert!(out.contains("cheevos_username = \"frank\""));
        assert!(!out.contains("cheevos_token"), "not ours to write: {out}");
    }

    /// Hardcore is written on every launch whether on or off, because
    /// inheriting it lets an install silently disable four shipped hotkeys.
    #[test]
    fn hardcore_is_always_stated_never_inherited() {
        let out = config_lines(&on(Some("frank")));
        assert!(
            out.contains("cheevos_hardcore_mode_enable = \"false\""),
            "off must be written explicitly, not left to the user's config: {out}"
        );

        let mut hard = on(Some("frank"));
        hard.hardcore = true;
        assert!(config_lines(&hard).contains("cheevos_hardcore_mode_enable = \"true\""));
        assert!(
            describe(&hard).unwrap().contains("HARDCORE"),
            "the cost has to be visible at launch"
        );
    }

    /// A quote in a username would end the value early and corrupt every
    /// setting after it in the generated file.
    #[test]
    fn quotes_in_a_username_cannot_break_the_config() {
        let mut s = on(Some("odd\"name"));
        s.token = Some("tok\\en".to_owned());
        let out = config_lines(&s);
        assert!(out.contains(r#"cheevos_username = "odd\"name""#), "{out}");
        assert!(out.contains(r#"cheevos_token = "tok\\en""#), "{out}");
    }

    /// Every emitted line must parse as a RetroArch assignment or a comment.
    #[test]
    fn every_emitted_line_is_a_setting_or_a_comment() {
        let mut s = on(Some("frank"));
        s.token = Some("abc".to_owned());
        s.test_unofficial = true;
        for line in config_lines(&s).lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, _) = line.split_once(" = ").unwrap_or_else(|| panic!("malformed: {line}"));
            assert!(
                key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "key {key:?} has characters RetroArch will not accept"
            );
        }
    }
}
