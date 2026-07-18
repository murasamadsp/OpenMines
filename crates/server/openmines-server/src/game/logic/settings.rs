use crate::game::{GameState, PlayerId};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct SettingsView {
    pub settings: crate::game::player::PlayerSettings,
    pub has_clan: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerSettingMutation {
    Changed(bool),
    Unchanged,
    MissingState(&'static str),
    MissingEntity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsSaveError {
    MalformedPayload,
    InvalidInteger(&'static str),
    InvalidBool(&'static str),
    MissingState,
}

pub fn settings_view(state: &Arc<GameState>, pid: PlayerId) -> Option<SettingsView> {
    state.query_player_opt(pid, |ecs, entity| {
        let settings = ecs
            .get::<crate::game::player::PlayerSettings>(entity)
            .copied()?;
        let has_clan = ecs
            .get::<crate::game::player::PlayerStats>(entity)
            .and_then(|st| st.clan_id)
            .unwrap_or(0)
            != 0;

        Some(SettingsView { settings, has_clan })
    })
}

pub fn save_settings(
    state: &Arc<GameState>,
    pid: PlayerId,
    data: &str,
) -> Result<Vec<u8>, SettingsSaveError> {
    let pairs = parse_settings_pairs(data).ok_or(SettingsSaveError::MalformedPayload)?;
    let patch = SettingsPatch::from_pairs(&pairs)?;

    let saved = state
        .modify_player(pid, |ecs, entity| {
            if ecs
                .get::<crate::game::player::PlayerSettings>(entity)
                .is_none()
                || ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_none()
            {
                return None;
            }

            let wire = {
                let mut settings = ecs
                    .get_mut::<crate::game::player::PlayerSettings>(entity)
                    .expect("PlayerSettings checked before settings save");
                patch.apply(&mut settings);
                settings_wire_payload(&settings)
            };

            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before settings save")
                .dirty = true;
            Some(wire)
        })
        .flatten()
        .ok_or(SettingsSaveError::MissingState)?;

    Ok(saved)
}

pub fn toggle_auto_dig(state: &Arc<GameState>, pid: PlayerId) -> PlayerSettingMutation {
    mutate_player_setting(state, pid, |settings| {
        settings.auto_dig = !settings.auto_dig;
        (true, settings.auto_dig)
    })
}

pub fn set_auto_dig(state: &Arc<GameState>, pid: PlayerId, enabled: bool) -> PlayerSettingMutation {
    mutate_player_setting(state, pid, |settings| {
        if settings.auto_dig == enabled {
            (false, enabled)
        } else {
            settings.auto_dig = enabled;
            (true, enabled)
        }
    })
}

pub fn toggle_aggression(state: &Arc<GameState>, pid: PlayerId) -> PlayerSettingMutation {
    mutate_player_setting(state, pid, |settings| {
        settings.aggression = !settings.aggression;
        (true, settings.aggression)
    })
}

pub fn set_aggression(
    state: &Arc<GameState>,
    pid: PlayerId,
    enabled: bool,
) -> PlayerSettingMutation {
    mutate_player_setting(state, pid, |settings| {
        if settings.aggression == enabled {
            (false, enabled)
        } else {
            settings.aggression = enabled;
            (true, enabled)
        }
    })
}

fn mutate_player_setting(
    state: &Arc<GameState>,
    pid: PlayerId,
    mutate: impl FnOnce(&mut crate::game::player::PlayerSettings) -> (bool, bool),
) -> PlayerSettingMutation {
    state
        .modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if ecs
                .get::<crate::game::player::PlayerSettings>(entity)
                .is_none()
            {
                return Some(PlayerSettingMutation::MissingState("PlayerSettings"));
            }
            if ecs
                .get::<crate::game::player::PlayerFlags>(entity)
                .is_none()
            {
                return Some(PlayerSettingMutation::MissingState("PlayerFlags"));
            }

            let mut settings = ecs
                .get_mut::<crate::game::player::PlayerSettings>(entity)
                .expect("PlayerSettings checked before settings mutation");
            let (changed, value) = mutate(&mut settings);
            if changed {
                ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                    .expect("PlayerFlags checked before settings mutation")
                    .dirty = true;
                Some(PlayerSettingMutation::Changed(value))
            } else {
                Some(PlayerSettingMutation::Unchanged)
            }
        })
        .flatten()
        .unwrap_or(PlayerSettingMutation::MissingEntity)
}

fn parse_settings_pairs(data: &str) -> Option<HashMap<&str, &str>> {
    let mut fields = HashMap::new();
    let trimmed = data.strip_suffix('#').unwrap_or(data);
    if trimmed.is_empty() {
        return None;
    }
    for pair in trimmed.split('#') {
        let (key, value) = pair.split_once(':')?;
        if key.is_empty() || value.is_empty() {
            return None;
        }
        fields.insert(key, value);
    }
    Some(fields)
}

#[derive(Default)]
struct SettingsPatch {
    isca: Option<i32>,
    tsca: Option<i32>,
    mous: Option<bool>,
    pot: Option<bool>,
    frc: Option<bool>,
    ctrl: Option<bool>,
    mof: Option<bool>,
}

impl SettingsPatch {
    fn from_pairs(pairs: &HashMap<&str, &str>) -> Result<Self, SettingsSaveError> {
        Ok(Self {
            isca: parse_i32(pairs, "isca")?,
            tsca: parse_i32(pairs, "tsca")?,
            mous: parse_bool(pairs, "mous")?,
            pot: parse_bool(pairs, "pot")?,
            frc: parse_bool(pairs, "frc")?,
            ctrl: parse_bool(pairs, "ctrl")?,
            mof: parse_bool(pairs, "mof")?,
        })
    }

    const fn apply(self, settings: &mut crate::game::player::PlayerSettings) {
        if let Some(v) = self.isca {
            settings.isca = v;
        }
        if let Some(v) = self.tsca {
            settings.tsca = v;
        }
        if let Some(v) = self.mous {
            settings.mous = v;
        }
        if let Some(v) = self.pot {
            settings.pot = v;
        }
        if let Some(v) = self.frc {
            settings.frc = v;
        }
        if let Some(v) = self.ctrl {
            settings.ctrl = v;
        }
        if let Some(v) = self.mof {
            settings.mof = v;
        }
    }
}

fn parse_i32(
    pairs: &HashMap<&str, &str>,
    key: &'static str,
) -> Result<Option<i32>, SettingsSaveError> {
    pairs
        .get(key)
        .map(|v| {
            v.parse::<i32>()
                .map_err(|_| SettingsSaveError::InvalidInteger(key))
        })
        .transpose()
}

fn parse_bool(
    pairs: &HashMap<&str, &str>,
    key: &'static str,
) -> Result<Option<bool>, SettingsSaveError> {
    pairs
        .get(key)
        .map(|v| match *v {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(SettingsSaveError::InvalidBool(key)),
        })
        .transpose()
}

fn settings_wire_payload(settings: &crate::game::player::PlayerSettings) -> Vec<u8> {
    let pairs: &[(&str, String)] = &[
        ("cc", settings.cc.to_string()),
        ("snd", bool_wire(settings.snd).to_string()),
        ("mus", bool_wire(settings.mus).to_string()),
        ("isca", settings.isca.to_string()),
        ("tsca", settings.tsca.to_string()),
        ("mous", bool_wire(settings.mous).to_string()),
        ("pot", bool_wire(settings.pot).to_string()),
        ("frc", bool_wire(settings.frc).to_string()),
        ("ctrl", bool_wire(settings.ctrl).to_string()),
        ("mof", bool_wire(settings.mof).to_string()),
    ];
    let inner = pairs
        .iter()
        .map(|(k, v)| format!("{k}#{v}"))
        .collect::<Vec<_>>()
        .join("#");
    format!("#{inner}").into_bytes()
}

const fn bool_wire(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player::PlayerSettings;

    #[test]
    fn parse_settings_pairs_accepts_unity_richlist_payload() {
        let parsed = parse_settings_pairs("isca:1#mous:0#").unwrap();
        assert_eq!(parsed.get("isca"), Some(&"1"));
        assert_eq!(parsed.get("mous"), Some(&"0"));
    }

    #[test]
    fn parse_settings_pairs_rejects_missing_or_empty_fields() {
        assert!(parse_settings_pairs("").is_none());
        assert!(parse_settings_pairs("isca").is_none());
        assert!(parse_settings_pairs("isca:").is_none());
        assert!(parse_settings_pairs(":1").is_none());
        assert!(parse_settings_pairs("isca:1##mous:0").is_none());
    }

    #[test]
    fn parse_settings_pairs_rejects_legacy_equals_comma_payload() {
        assert!(parse_settings_pairs("isca=1,mous=0").is_none());
    }

    #[test]
    fn settings_wire_payload_matches_client_format() {
        let settings = PlayerSettings {
            isca: 1,
            mous: false,
            ..PlayerSettings::default()
        };
        let wire = settings_wire_payload(&settings);
        assert_eq!(
            std::str::from_utf8(&wire).unwrap(),
            "#cc#10#snd#0#mus#0#isca#1#tsca#0#mous#0#pot#0#frc#1#ctrl#1#mof#1"
        );
    }
}
