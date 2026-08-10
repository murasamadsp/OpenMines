use super::{PlayerCommand, SaveKind};
use bytes::Bytes;

#[test]
fn chat_slash_commands_reserve_their_durable_kind_before_apply() {
    for (message, kind) in [
        ("/moneyall 7", SaveKind::AdminMoneyAll),
        ("/skill me U 1", SaveKind::AdminSkill),
        ("/role me admin", SaveKind::AdminRole),
        ("/clan create Kernel KRN", SaveKind::ClanCommand),
    ] {
        assert_eq!(
            PlayerCommand::LocalChat {
                message: message.to_string(),
            }
            .persistence_kind(),
            Some(kind)
        );
        assert_eq!(
            PlayerCommand::ChannelChat {
                payload: Bytes::from(format!("GLOBAL:0#{message}")),
            }
            .persistence_kind(),
            Some(kind)
        );
    }
    assert_eq!(
        PlayerCommand::LocalChat {
            message: "ordinary chat".to_string(),
        }
        .persistence_kind(),
        None
    );
}

#[test]
fn empty_program_selection_reserves_program_menu_before_apply() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0_i32.to_le_bytes());
    payload.extend_from_slice(&0_i32.to_le_bytes());
    assert_eq!(
        PlayerCommand::ProgramAction {
            event: "PROG".to_string(),
            payload: Bytes::from(payload),
        }
        .persistence_kind(),
        Some(SaveKind::ProgramMenu)
    );
}

#[test]
fn gui_programmer_button_reserves_program_menu_before_apply() {
    assert_eq!(
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("prog".to_string()),
        }
        .persistence_kind(),
        Some(SaveKind::ProgramMenu)
    );
}

#[test]
fn program_copy_reserves_its_durable_kind_before_apply() {
    assert_eq!(
        PlayerCommand::ProgramAction {
            event: "PCOP".to_string(),
            payload: Bytes::from_static(b"42"),
        }
        .persistence_kind(),
        Some(SaveKind::ProgramCopy)
    );
}

#[test]
fn building_menu_reserves_its_durable_kind_before_apply() {
    assert_eq!(
        PlayerCommand::RequestMyBuildings.persistence_kind(),
        Some(SaveKind::BuildingMenu)
    );
}

#[test]
fn pack_withdrawals_reserve_building_persistence_before_apply() {
    for action in ["pack_op:take_money:10:10", "pack_op:take_crys:10:10"] {
        assert_eq!(
            PlayerCommand::Gui {
                command: super::GuiCommand::parse(action.to_owned()),
            }
            .persistence_kind(),
            Some(SaveKind::Building)
        );
    }
}

#[test]
fn auction_grid_reserves_its_durable_kind_before_apply() {
    assert_eq!(
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("auc".to_string()),
        }
        .persistence_kind(),
        Some(SaveKind::AuctionGrid)
    );
}

#[test]
fn auction_item_orders_reserves_its_durable_kind_before_apply() {
    assert_eq!(
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("choose:1".to_string()),
        }
        .persistence_kind(),
        Some(SaveKind::AuctionItemOrders)
    );
}

#[test]
fn auction_order_reserves_its_durable_kind_before_apply() {
    assert_eq!(
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("openorder:42".to_string()),
        }
        .persistence_kind(),
        Some(SaveKind::AuctionOrder)
    );
}

#[test]
fn auction_bet_mutations_reserve_their_durable_kind_before_apply() {
    for action in ["aucminbet:42", "aucbet:42:100"] {
        assert_eq!(
            PlayerCommand::Gui {
                command: super::GuiCommand::parse(action.to_owned()),
            }
            .persistence_kind(),
            Some(SaveKind::AuctionBet)
        );
    }
}

#[test]
fn whois_reserves_its_durable_kind_before_apply() {
    assert_eq!(
        PlayerCommand::Whois { ids: vec![1] }.persistence_kind(),
        Some(SaveKind::Whois)
    );
}

#[test]
fn clan_menu_entries_reserve_before_world_or_gui_apply() {
    for command in [
        PlayerCommand::OpenClan,
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("clan_menu".to_string()),
        },
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("clan_view:42".to_string()),
        },
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("clan_members".to_string()),
        },
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("clan_invite_list".to_string()),
        },
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("clan_invites_view".to_string()),
        },
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("clan_requests".to_string()),
        },
        PlayerCommand::Gui {
            command: super::GuiCommand::parse("pack_op:open:3:7".to_string()),
        },
    ] {
        assert_eq!(command.persistence_kind(), Some(SaveKind::ClanMenu));
    }
}
