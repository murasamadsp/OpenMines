//! Единый каталог admin-команд для console, in-game slash и web GUI.

use crate::game::GameState;
use crate::game::player::{PlayerConnection, PlayerFlags, PlayerId, PlayerStats};
use crate::net::session::wire::make_u_packet_bytes;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AdminCommandSpec {
    pub name: &'static str,
    pub slash: &'static str,
    pub console: Option<&'static str>,
    pub description: &'static str,
}

pub const ADMIN_COMMANDS: &[AdminCommandSpec] = &[
    AdminCommandSpec {
        name: "give",
        slash: "/give ITEM_ID AMOUNT",
        console: Some("give -p <ID> -i <ITEM_ID> [-a <N>]"),
        description: "выдать предмет",
    },
    AdminCommandSpec {
        name: "giveall",
        slash: "/giveall",
        console: None,
        description: "выдать все предметы текущему админу",
    },
    AdminCommandSpec {
        name: "money",
        slash: "/money AMOUNT",
        console: Some("money -p <ID> -a <N>"),
        description: "добавить деньги",
    },
    AdminCommandSpec {
        name: "moneyall",
        slash: "/moneyall AMOUNT",
        console: None,
        description: "добавить деньги всем игрокам",
    },
    AdminCommandSpec {
        name: "tp",
        slash: "/tp X Y",
        console: Some("tp -p <ID> -x <X> -y <Y>"),
        description: "телепортировать",
    },
    AdminCommandSpec {
        name: "heal",
        slash: "/heal",
        console: Some("heal -p <ID>"),
        description: "восстановить HP",
    },
    AdminCommandSpec {
        name: "kill",
        slash: "",
        console: Some("kill -p <ID>"),
        description: "убить игрока",
    },
    AdminCommandSpec {
        name: "skill",
        slash: "/skill ИМЯ|me CODE LEVEL [SLOT] [EXP]",
        console: None,
        description: "установить скилл",
    },
    AdminCommandSpec {
        name: "kick",
        slash: "/kick ИМЯ",
        console: Some("kick -p <ID>"),
        description: "кикнуть игрока",
    },
    AdminCommandSpec {
        name: "role",
        slash: "/role ИМЯ admin|mod|player",
        console: Some("role -p <ID> -r admin|mod|player"),
        description: "установить роль",
    },
    AdminCommandSpec {
        name: "clan",
        slash: "/clan create ИМЯ ТЕГ | /clan leave | /clan kick ИМЯ",
        console: None,
        description: "администрировать клан",
    },
    AdminCommandSpec {
        name: "pack",
        slash: "/pack owner|clan|move|type ...",
        console: None,
        description: "администрировать здание",
    },
    AdminCommandSpec {
        name: "announce",
        slash: "",
        console: Some("announce <message>"),
        description: "отправить ST всем онлайн игрокам",
    },
    AdminCommandSpec {
        name: "online",
        slash: "",
        console: Some("online"),
        description: "показать онлайн игроков",
    },
    AdminCommandSpec {
        name: "find",
        slash: "",
        console: Some("find <name>"),
        description: "найти игрока online + DB",
    },
    AdminCommandSpec {
        name: "info",
        slash: "",
        console: Some("info -p <ID>"),
        description: "показать подробности игрока",
    },
    AdminCommandSpec {
        name: "save",
        slash: "",
        console: Some("save"),
        description: "сохранить игроков и мир",
    },
    AdminCommandSpec {
        name: "schedule",
        slash: "",
        console: Some("schedule <name> <ms>"),
        description: "изменить ECS schedule interval",
    },
    AdminCommandSpec {
        name: "shutdown",
        slash: "",
        console: Some("stop | shutdown"),
        description: "мягко остановить сервер",
    },
];

#[must_use]
pub fn slash_help() -> String {
    let mut lines = vec!["Админские команды:".to_string()];
    for spec in ADMIN_COMMANDS.iter().filter(|spec| !spec.slash.is_empty()) {
        lines.push(format!("{} — {}", spec.slash, spec.description));
    }
    lines.push("/admin — показать справку по админ-командам".to_string());
    lines.join("\n")
}

#[must_use]
pub fn console_help() -> String {
    let mut lines = vec!["Available commands:".to_string()];
    for spec in ADMIN_COMMANDS.iter().filter_map(|spec| {
        spec.console
            .map(|console| format!("  {console:<42} {}", spec.description))
    }) {
        lines.push(spec);
    }
    lines.push("  help | ?                                   показать справку".to_string());
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminCommandError {
    PlayerUnavailable,
    MissingPlayerState(&'static str),
}

pub type AdminCommandResult = Result<(), AdminCommandError>;

pub fn add_player_money(
    state: &Arc<GameState>,
    target_pid: PlayerId,
    amount: i64,
) -> AdminCommandResult {
    state
        .modify_player(target_pid, |ecs, entity| {
            if ecs.get::<PlayerStats>(entity).is_none() {
                return Err(AdminCommandError::MissingPlayerState("PlayerStats"));
            }
            if ecs.get::<PlayerFlags>(entity).is_none() {
                return Err(AdminCommandError::MissingPlayerState("PlayerFlags"));
            }

            let mut player_stats = ecs
                .get_mut::<PlayerStats>(entity)
                .expect("PlayerStats checked before admin money mutation");
            player_stats.money = player_stats.money.saturating_add(amount);
            let (money, creds) = (player_stats.money, player_stats.creds);

            let mut flags = ecs
                .get_mut::<PlayerFlags>(entity)
                .expect("PlayerFlags checked before admin money mutation");
            flags.dirty = true;

            if let Some(conn) = ecs.get::<PlayerConnection>(entity) {
                let packet = crate::protocol::packets::money(money, creds);
                if let Some(tx) = state.sessions.outbox_for_session(conn.session_id) {
                    let _ = tx.send(make_u_packet_bytes(packet.0, &packet.1));
                }
            }

            Ok(())
        })
        .unwrap_or(Err(AdminCommandError::PlayerUnavailable))
}

pub fn heal_player(state: &Arc<GameState>, target_pid: PlayerId) -> AdminCommandResult {
    state
        .modify_player(target_pid, |ecs, entity| {
            if ecs.get::<PlayerStats>(entity).is_none() {
                return Err(AdminCommandError::MissingPlayerState("PlayerStats"));
            }
            if ecs.get::<PlayerFlags>(entity).is_none() {
                return Err(AdminCommandError::MissingPlayerState("PlayerFlags"));
            }

            let mut player_stats = ecs
                .get_mut::<PlayerStats>(entity)
                .expect("PlayerStats checked before admin heal mutation");
            player_stats.health = player_stats.max_health;
            let (health, max_health) = (player_stats.health, player_stats.max_health);

            let mut flags = ecs
                .get_mut::<PlayerFlags>(entity)
                .expect("PlayerFlags checked before admin heal mutation");
            flags.dirty = true;

            if let Some(conn) = ecs.get::<PlayerConnection>(entity) {
                let packet = crate::protocol::packets::health(health, max_health);
                if let Some(tx) = state.sessions.outbox_for_session(conn.session_id) {
                    let _ = tx.send(make_u_packet_bytes(packet.0, &packet.1));
                }
            }

            Ok(())
        })
        .unwrap_or(Err(AdminCommandError::PlayerUnavailable))
}

#[cfg(test)]
mod tests {
    use super::{ADMIN_COMMANDS, console_help, slash_help};

    #[test]
    fn slash_help_uses_canonical_registry() {
        let help = slash_help();
        assert!(help.contains("/skill ИМЯ|me CODE LEVEL [SLOT] [EXP]"));
        assert!(help.contains("/role ИМЯ admin|mod|player"));
        assert!(!help.contains("save"));
    }

    #[test]
    fn console_help_uses_canonical_registry() {
        let help = console_help();
        assert!(help.contains("role -p <ID> -r admin|mod|player"));
        assert!(help.contains("save"));
        assert!(!help.contains("/skill ИМЯ|me"));
    }

    #[test]
    fn registry_has_stable_command_names_for_web_gui() {
        let names: Vec<_> = ADMIN_COMMANDS.iter().map(|spec| spec.name).collect();
        assert!(names.contains(&"give"));
        assert!(names.contains(&"role"));
        assert!(names.contains(&"schedule"));
    }
}
