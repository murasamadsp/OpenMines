//! Authenticated TY command boundary.
//!
//! This module is the single place that maps legacy wire TY events into
//! `PlayerCommand` values consumed by the simulation tick.

use std::sync::Arc;
use std::time::Instant;

use crate::game::{GameState, PlayerCommand, PlayerId, SessionId};
use crate::protocol::packets::{
    LoclClient, TyPacket, XbldClient, decode_gui_button, decode_whoi, decode_xdig, decode_xmov,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TyRouteClass {
    Gameplay,
    Presentation,
    Chat,
    Admin,
    Noop,
    Unknown,
}

impl TyRouteClass {
    const fn metric_name(self) -> &'static str {
        match self {
            Self::Gameplay => "gameplay",
            Self::Presentation => "presentation",
            Self::Chat => "chat",
            Self::Admin => "admin",
            Self::Noop => "noop",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug)]
enum TyRoute {
    Accepted {
        class: TyRouteClass,
        command: PlayerCommand,
    },
    Rejected {
        class: TyRouteClass,
        reason: &'static str,
        event: String,
        payload: bytes::Bytes,
    },
}

pub fn classify_ty_event(event: &str) -> TyRouteClass {
    match event {
        "Xmov" | "Xdig" | "Xbld" | "Xgeo" | "Xhea" | "INVN" | "INCL" | "TADG" | "RESP" | "PROG"
        | "INUS" | "TAGR" | "DPBX" | "GDon" | "PDEL" | "pRST" | "PREN" | "PCOP" => {
            TyRouteClass::Gameplay
        }

        "GUI_" | "Pope" | "Blds" | "Clan" | "Sett" => TyRouteClass::Presentation,

        "Locl" | "Chat" | "Chin" | "Cmen" | "Choo" | "Cset" | "Cpri" | "Whoi" => TyRouteClass::Chat,

        "ADMN" => TyRouteClass::Admin,

        "Xhur" | "FINV" | "Help" | "Miso" | "THID" | "Miss" | "Rndm" | "TAUR" => TyRouteClass::Noop,

        _ => TyRouteClass::Unknown,
    }
}

pub fn enqueue_ty_command(
    state: &Arc<GameState>,
    session_id: SessionId,
    player_id: PlayerId,
    packet: &TyPacket,
    received_at: Instant,
) {
    match route_ty_command(player_id, session_id, packet) {
        TyRoute::Accepted { class, command } => {
            tracing::debug!(
                player_id = %player_id,
                class = ?class,
                command = command.name(),
                "accepted TY command"
            );
            state.enqueue_command_received(command, received_at);
        }
        TyRoute::Rejected {
            class,
            reason,
            event,
            payload,
        } => {
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[class.metric_name(), reason])
                .inc();
            tracing::warn!(
                player_id = %player_id,
                class = ?class,
                reason,
                event,
                payload = ?payload,
                "rejected TY command"
            );
        }
    }
}

fn route_ty_command(player_id: PlayerId, session_id: SessionId, packet: &TyPacket) -> TyRoute {
    let event = packet.event_str().to_owned();
    let class = classify_ty_event(&event);
    let payload = packet.sub_payload.clone();
    match decode_ty_command(player_id, session_id, packet) {
        Some(command) => TyRoute::Accepted { class, command },
        None if class == TyRouteClass::Unknown => TyRoute::Rejected {
            class,
            reason: "unknown_event",
            event,
            payload,
        },
        None => TyRoute::Rejected {
            class,
            reason: "malformed_payload",
            event,
            payload,
        },
    }
}

fn decode_ty_command(
    player_id: PlayerId,
    session_id: SessionId,
    packet: &TyPacket,
) -> Option<PlayerCommand> {
    match packet.event_str() {
        "Xmov" => decode_xmov(&packet.sub_payload).map(|direction| PlayerCommand::Move {
            player_id,
            session_id,
            time: packet.time,
            x: packet.x.cast_signed(),
            y: packet.y.cast_signed(),
            direction,
            programmatic: false,
        }),
        "Xdig" => decode_xdig(&packet.sub_payload).map(|direction| PlayerCommand::Dig {
            player_id,
            direction,
            programmatic: false,
        }),
        "Xbld" => XbldClient::decode(&packet.sub_payload).map(|bld| PlayerCommand::Build {
            player_id,
            direction: bld.direction,
            block_type: bld.block_type.to_owned(),
            programmatic: false,
        }),
        "Xgeo" => Some(PlayerCommand::Geology {
            player_id,
            programmatic: false,
        }),
        "Xhea" => Some(PlayerCommand::Heal {
            player_id,
            programmatic: false,
        }),
        "GUI_" => decode_gui_button(&packet.sub_payload).map(|button| PlayerCommand::Gui {
            session_id,
            player_id,
            command: crate::game::GuiCommand::parse(button.into_owned()),
        }),
        "Locl" => LoclClient::decode(&packet.sub_payload).map(|locl| PlayerCommand::LocalChat {
            player_id,
            message: locl.message.to_owned(),
        }),
        "INVN" => Some(PlayerCommand::InventoryToggle { player_id }),
        "INCL" => Some(PlayerCommand::InventoryChoose {
            player_id,
            payload: packet.sub_payload.clone(),
        }),
        "INUS" => Some(PlayerCommand::InventoryUse {
            session_id,
            player_id,
        }),
        "TADG" => Some(PlayerCommand::ToggleAutoDig { player_id }),
        "TAGR" => Some(PlayerCommand::ToggleAggression { player_id }),
        "Whoi" => {
            decode_whoi(&packet.sub_payload).map(|ids| PlayerCommand::Whois { player_id, ids })
        }
        "Chat" => Some(PlayerCommand::ChannelChat {
            player_id,
            payload: packet.sub_payload.clone(),
        }),
        "Chin" => Some(PlayerCommand::ChatResync {
            player_id,
            payload: packet.sub_payload.clone(),
        }),
        "Cmen" => Some(PlayerCommand::ChatMenu {
            player_id,
            payload: packet.sub_payload.clone(),
        }),
        "Choo" => Some(PlayerCommand::ChatChoose {
            player_id,
            payload: packet.sub_payload.clone(),
        }),
        "Cset" => Some(PlayerCommand::ChatSettings {
            player_id,
            payload: packet.sub_payload.clone(),
        }),
        "Cpri" => Some(PlayerCommand::ChatPrivate {
            player_id,
            payload: packet.sub_payload.clone(),
        }),
        "RESP" => Some(PlayerCommand::Respawn { player_id }),
        "Pope" => Some(PlayerCommand::OpenProgrammer { player_id }),
        "Blds" => Some(PlayerCommand::RequestMyBuildings { player_id }),
        "Clan" => Some(PlayerCommand::OpenClan { player_id }),
        "Sett" => Some(PlayerCommand::SettingsSave {
            player_id,
            payload: packet.sub_payload.clone(),
        }),
        "ADMN" => Some(PlayerCommand::AdminAction { player_id }),
        "DPBX" => Some(PlayerCommand::OpenBox { player_id }),
        "GDon" => Some(PlayerCommand::ClaimBonus { player_id }),
        "PROG" | "PDEL" | "pRST" | "PREN" | "PCOP" => Some(PlayerCommand::ProgramAction {
            player_id,
            session_id,
            event: packet.event_str().to_owned(),
            payload: packet.sub_payload.clone(),
        }),
        "Xhur" | "FINV" | "Help" | "Miso" | "THID" | "Miss" | "Rndm" | "TAUR" => {
            Some(PlayerCommand::KnownNoopTy {
                player_id,
                event: packet.event_str().to_owned(),
                payload: packet.sub_payload.clone(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(event: &[u8], payload: &'static [u8]) -> TyPacket {
        TyPacket {
            event_name: event
                .try_into()
                .expect("test event must contain four bytes"),
            time: 0,
            x: 0,
            y: 0,
            sub_payload: bytes::Bytes::from_static(payload),
        }
    }

    #[test]
    fn all_known_wire_events_have_an_explicit_class() {
        let known_events = [
            "Xmov", "Xdig", "Xbld", "GUI_", "Locl", "Xgeo", "Xhea", "Xhur", "INVN", "INUS", "INCL",
            "FINV", "TADG", "TAGR", "Whoi", "Chat", "Chin", "Cmen", "Choo", "Cset", "Cpri", "RESP",
            "Pope", "Blds", "Clan", "Sett", "ADMN", "DPBX", "GDon", "Help", "Miso", "THID", "PROG",
            "PDEL", "pRST", "PREN", "PCOP", "Miss", "Rndm", "TAUR",
        ];
        for event in known_events {
            assert_ne!(
                classify_ty_event(event),
                TyRouteClass::Unknown,
                "{event} is known on the wire and must be classified"
            );
        }
    }

    #[test]
    fn unknown_events_are_marked_unknown() {
        assert_eq!(classify_ty_event("NOPE"), TyRouteClass::Unknown);
    }

    #[test]
    fn unknown_ty_does_not_enter_simulation() {
        let packet = ty(b"NOPE", b"");

        assert!(decode_ty_command(PlayerId(1), SessionId::new(1), &packet).is_none());
    }

    #[test]
    fn route_accepts_valid_ty_as_typed_command() {
        let packet = ty(b"Xmov", b"1");

        let route = route_ty_command(PlayerId(7), SessionId::new(1), &packet);

        assert!(matches!(
            route,
            TyRoute::Accepted {
                class: TyRouteClass::Gameplay,
                command: PlayerCommand::Move {
                    player_id: PlayerId(7),
                    direction: 1,
                    ..
                }
            }
        ));
    }

    #[test]
    fn gui_open_pack_is_parsed_once_with_session_and_static_label() {
        let packet = ty(b"GUI_", br#"{"b":"pack_op:open:42:17"}"#);

        let route = route_ty_command(PlayerId(7), SessionId::new(9), &packet);

        let TyRoute::Accepted { command, .. } = route else {
            panic!("GUI command must be accepted");
        };
        assert_eq!(command.name(), "gui.pack.open");
        assert!(matches!(
            command,
            PlayerCommand::Gui {
                session_id,
                player_id: PlayerId(7),
                command: crate::game::GuiCommand::OpenPack { x: 42, y: 17 },
            } if session_id == SessionId::new(9)
        ));
    }

    #[test]
    fn gui_profiler_labels_are_finite_and_payload_independent() {
        let cases = [
            ("tp:10:20", "gui.teleport"),
            ("craft_start:1:2:3:4", "gui.craft"),
            ("clan_view:123", "gui.clan"),
            ("aucbet:99:1000", "gui.auction"),
            ("unknown:arbitrary-payload", "gui.other"),
        ];
        for (raw, expected) in cases {
            assert_eq!(
                crate::game::GuiCommand::parse(raw.to_owned()).label(),
                expected
            );
        }
    }

    #[test]
    fn route_rejects_unknown_ty_before_simulation() {
        let packet = ty(b"NOPE", b"payload");

        let route = route_ty_command(PlayerId(7), SessionId::new(1), &packet);

        assert!(matches!(
            route,
            TyRoute::Rejected {
                class: TyRouteClass::Unknown,
                reason: "unknown_event",
                ..
            }
        ));
    }

    #[test]
    fn route_rejects_malformed_known_ty_before_simulation() {
        let packet = ty(b"Xmov", b"not-a-direction");

        let route = route_ty_command(PlayerId(7), SessionId::new(1), &packet);

        assert!(matches!(
            route,
            TyRoute::Rejected {
                class: TyRouteClass::Gameplay,
                reason: "malformed_payload",
                ..
            }
        ));
    }

    #[test]
    fn all_known_wire_events_decode_into_commands() {
        let valid_packets = [
            ty(b"Xmov", b"1"),
            ty(b"Xdig", b"1"),
            ty(b"Xbld", b"1R"),
            ty(b"GUI_", br#"{"b":"exit"}"#),
            ty(b"Locl", b"hello"),
            ty(b"Xgeo", b"_"),
            ty(b"Xhea", b"_"),
            ty(b"Xhur", b"_"),
            ty(b"INVN", b"_"),
            ty(b"INUS", b"_"),
            ty(b"INCL", b"0"),
            ty(b"FINV", b"0"),
            ty(b"TADG", b"_"),
            ty(b"TAGR", b"_"),
            ty(b"Whoi", b"1,2"),
            ty(b"Chat", b"hello"),
            ty(b"Chin", b"_"),
            ty(b"Cmen", b"_"),
            ty(b"Choo", b"FED"),
            ty(b"Cset", b"_"),
            ty(b"Cpri", b"1"),
            ty(b"RESP", b"_"),
            ty(b"Pope", b"_"),
            ty(b"Blds", b"_"),
            ty(b"Clan", b"_"),
            ty(b"Sett", b"_"),
            ty(b"ADMN", b"_"),
            ty(b"DPBX", b"_"),
            ty(b"GDon", b"_"),
            ty(b"PROG", b"1\n$z"),
            ty(b"PDEL", b"1"),
            ty(b"pRST", b"_"),
            ty(b"PREN", b"1"),
            ty(b"PCOP", b"1"),
            ty(b"Miss", b"0"),
            ty(b"Rndm", b"hash=device"),
            ty(b"TAUR", b"_"),
        ];

        for packet in valid_packets {
            let event = packet.event_str().to_owned();
            assert!(
                decode_ty_command(PlayerId(1), SessionId::new(1), &packet).is_some(),
                "{event} must decode into PlayerCommand"
            );
        }
    }
}
