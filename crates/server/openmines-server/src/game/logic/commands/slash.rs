#![allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::assigning_clones,
    clippy::items_after_statements,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::missing_panics_doc
)]
use super::CommandEffects;

pub(super) fn apply_slash_command(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    command: crate::game::SlashCommand,
) -> CommandEffects {
    match command {
        crate::game::SlashCommand::Invalid { title, message } => {
            context.slash_ok_effect(session_id, player_id, &title, &message)
        }
        crate::game::SlashCommand::Help | crate::game::SlashCommand::Unknown { .. }
            if context.is_admin(player_id) =>
        {
            context.slash_ok_effect(
                session_id,
                player_id,
                "Админ-команды",
                &crate::admin::slash_help(),
            )
        }
        crate::game::SlashCommand::Help => {
            context.slash_ok_effect(session_id, player_id, "Ошибка", "Нет прав на админ-команду")
        }
        crate::game::SlashCommand::Unknown { command } => context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            &format!("Неизвестная команда: {command}"),
        ),
        crate::game::SlashCommand::MoneyAll { amount } => {
            admin_economy_money_all(context, player_id, session_id, amount)
        }
        crate::game::SlashCommand::Give { item_id, amount } => {
            admin_economy_give(context, player_id, session_id, item_id, amount)
        }
        crate::game::SlashCommand::GiveAll => {
            admin_economy_give_all(context, player_id, session_id)
        }
        crate::game::SlashCommand::Money { amount } => {
            admin_economy_money(context, player_id, session_id, amount)
        }
        crate::game::SlashCommand::Heal => admin_economy_heal(context, player_id, session_id),
        crate::game::SlashCommand::SkillHelp => admin_skill_help(context, player_id, session_id),
        crate::game::SlashCommand::Skill {
            target,
            code,
            level,
            slot,
            exp,
        } => admin_skill(
            context,
            player_id,
            session_id,
            AdminSkillArgs {
                target,
                code,
                level,
                slot,
                exp,
            },
        ),
        crate::game::SlashCommand::Kick { target } => {
            admin_kick(context, player_id, session_id, target)
        }
        crate::game::SlashCommand::Teleport { x, y } => {
            admin_teleport(context, player_id, session_id, x, y)
        }
        crate::game::SlashCommand::Pack { action } => {
            admin_pack(context, player_id, session_id, action)
        }
        crate::game::SlashCommand::Clan {
            action: crate::game::ClanAction::Invalid { message },
        } => context.slash_ok_effect(session_id, player_id, "Клан", &message),
        crate::game::SlashCommand::Clan { action } => {
            admin_clan(context, player_id, session_id, action)
        }
        crate::game::SlashCommand::Role { target, role } => {
            admin_role(context, player_id, session_id, target, role)
        }
    }
}

fn admin_economy_money_all(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    amount: i64,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::AdminMoneyAll {
            request: crate::game::AdminMoneyAllRequest {
                player_id,
                session_id,
                amount,
            },
        }],
        broadcasts: Vec::new(),
    }
}

fn admin_economy_give(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    item_id: i32,
    amount: i32,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    let packets = context
        .modify_player(player_id, |ecs, entity| {
            let batch = crate::net::session::wire::PacketBatch::default();
            {
                let mut inventory = ecs.get_mut::<crate::game::player::PlayerInventory>(entity)?;
                *inventory.items.entry(item_id).or_insert(0) = inventory
                    .items
                    .get(&item_id)
                    .copied()
                    .unwrap_or_default()
                    .saturating_add(amount);
                crate::net::session::outbound::inventory_sync::send_inventory(
                    &batch,
                    &mut inventory,
                );
            }
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                .dirty = true;
            Some(batch.into_packets())
        })
        .flatten();
    packets.map_or_else(
        || {
            context.slash_ok_effect(
                session_id,
                player_id,
                "КОМАНДА",
                "Состояние игрока недоступно.",
            )
        },
        |packets| CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets,
            }],
            saves: Vec::new(),
            broadcasts: Vec::new(),
        },
    )
}

fn admin_economy_give_all(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    let money_result = context
        .modify_player(player_id, |ecs, entity| {
            {
                let mut inventory = ecs.get_mut::<crate::game::player::PlayerInventory>(entity)?;
                for item_id in 0..=50 {
                    *inventory.items.entry(item_id).or_insert(0) = inventory
                        .items
                        .get(&item_id)
                        .copied()
                        .unwrap_or_default()
                        .saturating_add(10);
                }
                inventory.minv = false;
                inventory.miniq.clear();
                let mut item_ids = inventory.items.keys().copied().collect::<Vec<_>>();
                item_ids.sort_unstable();
                inventory.miniq.extend(item_ids.into_iter().take(4));
            }
            {
                let mut stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                stats.money = stats.money.saturating_add(1_000_000);
                stats.creds = stats.creds.saturating_add(100_000);
            }
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                .dirty = true;
            let stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
            Some((stats.money, stats.creds))
        })
        .flatten();
    let Some((money, creds)) = money_result else {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "КОМАНДА",
            "Состояние игрока недоступно.",
        );
    };
    let mut all_packets = Vec::new();
    // Send inventory (minv=false first, then minv=true with miniq)
    if let Some(inv_packets) = context
        .modify_player(player_id, |ecs, entity| {
            let mut inv = ecs.get_mut::<crate::game::player::PlayerInventory>(entity)?;
            let batch = crate::net::session::wire::PacketBatch::default();
            inv.minv = false;
            crate::net::session::outbound::inventory_sync::send_inventory(&batch, &mut inv);
            inv.minv = true;
            crate::net::session::outbound::inventory_sync::send_inventory(&batch, &mut inv);
            Some(batch.into_packets())
        })
        .flatten()
    {
        all_packets.extend(inv_packets);
    }
    let money_pkt = crate::protocol::packets::money(money, creds);
    let money_batch = crate::net::session::wire::PacketBatch::default();
    crate::net::session::wire::send_u_packet(&money_batch, money_pkt.0, &money_pkt.1);
    all_packets.extend(money_batch.into_packets());
    CommandEffects {
        events: vec![crate::game::GameEvent::SessionBatch {
            session_id,
            player_id,
            packets: all_packets,
        }],
        saves: Vec::new(),
        broadcasts: Vec::new(),
    }
}

fn admin_economy_money(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    amount: i64,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    let packet = context
        .modify_player(player_id, |ecs, entity| {
            let packet = {
                let mut stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                stats.money = stats.money.saturating_add(amount);
                crate::protocol::packets::money(stats.money, stats.creds)
            };
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                .dirty = true;
            Some(crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            ))
        })
        .flatten();
    packet.map_or_else(
        || {
            context.slash_ok_effect(
                session_id,
                player_id,
                "КОМАНДА",
                "Состояние игрока недоступно.",
            )
        },
        |packet| CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: vec![packet],
            }],
            saves: Vec::new(),
            broadcasts: Vec::new(),
        },
    )
}

fn admin_economy_heal(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    let packet = context
        .modify_player(player_id, |ecs, entity| {
            let packet = {
                let mut stats = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                stats.health = stats.max_health;
                crate::protocol::packets::health(stats.health, stats.max_health)
            };
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?
                .dirty = true;
            Some(crate::net::session::wire::make_u_packet_bytes(
                packet.0, &packet.1,
            ))
        })
        .flatten();
    packet.map_or_else(
        || {
            context.slash_ok_effect(
                session_id,
                player_id,
                "КОМАНДА",
                "Состояние игрока недоступно.",
            )
        },
        |packet| CommandEffects {
            events: vec![crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: vec![packet],
            }],
            saves: Vec::new(),
            broadcasts: Vec::new(),
        },
    )
}

fn admin_skill_help(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    context.slash_ok_effect(
        session_id,
        player_id,
        "Скиллы",
        &crate::game::logic::commands_social::admin_skill_codes_help(),
    )
}

struct AdminSkillArgs {
    target: String,
    code: String,
    level: i32,
    slot: Option<i32>,
    exp: f32,
}

fn admin_skill(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    args: AdminSkillArgs,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    let Some(target_id) = context.resolve_online_player(player_id, &args.target) else {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            &format!("Игрок '{}' не в сети", args.target),
        );
    };
    let Some(skill_type) = crate::game::skills::SkillType::from_code(&args.code) else {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Скилл",
            "Неизвестный wire/DB-код скилла. Примеры: U=геология, M=ход, d=копка, l=HP",
        );
    };
    let batch = crate::net::session::wire::PacketBatch::default();
    let Some((target_name, chosen_slot, row)) =
        crate::game::logic::commands_social::apply_admin_skill_set(
            context, &batch, target_id, skill_type, args.level, args.slot, args.exp,
        )
    else {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "КОМАНДА",
            "Состояние игрока недоступно.",
        );
    };
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::AdminSkill {
            request: crate::game::AdminSkillRequest {
                player_id,
                session_id,
                target_id,
                target_name,
                target_session_id: context.player_session(target_id),
                skill_code: skill_type.code().to_owned(),
                level: args.level,
                slot: chosen_slot,
                exp: args.exp,
                packets: batch.into_packets(),
                row: Box::new(row),
            },
        }],
        broadcasts: Vec::new(),
    }
}

fn admin_kick(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    target: String,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    let Some(target_id) = context.resolve_online_player(player_id, &target) else {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            &format!("Игрок '{target}' не в сети"),
        );
    };
    let (title, message) = if context.kick_player(target_id) {
        ("Кик", format!("Игрок {target} кикнут"))
    } else {
        ("Ошибка", "Не удалось кикнуть игрока".to_owned())
    };
    context.slash_ok_effect(session_id, player_id, title, &message)
}

fn admin_teleport(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    x: i32,
    y: i32,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    if !context.world_valid_coord(x, y) {
        return context.slash_ok_effect(session_id, player_id, "Ошибка", "Координаты вне карты");
    }
    if !context.teleport_player(player_id, x, y) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "КОМАНДА",
            "Состояние игрока недоступно.",
        );
    }
    let teleport = crate::protocol::packets::tp(x, y);
    CommandEffects {
        events: vec![
            crate::game::GameEvent::SessionBatch {
                session_id,
                player_id,
                packets: vec![crate::net::session::wire::make_u_packet_bytes(
                    teleport.0,
                    &teleport.1,
                )],
            },
            crate::game::GameEvent::RefreshChunks {
                session_id,
                player_id,
            },
        ],
        saves: Vec::new(),
        broadcasts: Vec::new(),
    }
}

fn admin_pack(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    action: crate::game::SlashPackCommand,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    let result = match action {
        crate::game::SlashPackCommand::Owner { x, y, owner_id } => context
            .modify_pack(x, y, |ecs, entity| {
                if let Some(mut ownership) =
                    ecs.get_mut::<crate::game::buildings::BuildingOwnership>(entity)
                {
                    ownership.owner_id = crate::game::PlayerId::from(owner_id);
                }
            })
            .map(|()| ("Пак", "Владелец обновлен")),
        crate::game::SlashPackCommand::Clan { x, y, clan_id } => context
            .modify_pack(x, y, |ecs, entity| {
                if let Some(mut ownership) =
                    ecs.get_mut::<crate::game::buildings::BuildingOwnership>(entity)
                {
                    ownership.clan_id = clan_id;
                }
            })
            .map(|()| ("Пак", "Клан обновлен")),
        crate::game::SlashPackCommand::Type { x, y, pack_type } => {
            let extra = match crate::game::logic::buildings::building_extra_for_pack_type(pack_type)
            {
                Ok(extra) => extra,
                Err(error) => {
                    tracing::error!(
                        ?pack_type,
                        ?error,
                        "Missing building config for pack type command"
                    );
                    return context.slash_ok_effect(
                        session_id,
                        player_id,
                        "Ошибка",
                        "Конфиг здания не найден",
                    );
                }
            };
            context
                .modify_pack(x, y, |ecs, entity| {
                    if let Some(mut metadata) =
                        ecs.get_mut::<crate::game::buildings::BuildingMetadata>(entity)
                    {
                        metadata.pack_type = pack_type;
                    }
                    if let Some(mut stats) =
                        ecs.get_mut::<crate::game::buildings::BuildingStats>(entity)
                    {
                        stats.max_charge = extra.max_charge;
                        stats.charge = stats.charge.min(extra.max_charge);
                        stats.max_hp = extra.max_hp;
                        stats.hp = stats.hp.min(extra.max_hp);
                    }
                })
                .map(|()| ("Пак", "Тип обновлен"))
        }
        crate::game::SlashPackCommand::Move { x, y, to_x, to_y } => {
            let Some(old_view) = context.pack_at(x, y) else {
                return context.slash_ok_effect(
                    session_id,
                    player_id,
                    "Ошибка",
                    "Здание не найдено",
                );
            };
            if let Err(message) = context.validate_pack_move(&old_view, to_x, to_y) {
                return context.slash_ok_effect(session_id, player_id, "Ошибка", message);
            }
            context
                .modify_pack(x, y, |ecs, entity| {
                    if let Some(mut position) =
                        ecs.get_mut::<crate::game::buildings::GridPosition>(entity)
                    {
                        position.x = to_x;
                        position.y = to_y;
                    }
                })
                .map(|()| {
                    context.move_pack_index_and_cells(&old_view, to_x, to_y);
                    ("Пак", "Позиция обновлена")
                })
        }
        crate::game::SlashPackCommand::Invalid { message } => {
            return context.slash_ok_effect(session_id, player_id, "Пак", &message);
        }
    };
    match result {
        Ok((title, message)) => context.slash_ok_effect(session_id, player_id, title, message),
        Err(_) => context.slash_ok_effect(session_id, player_id, "Ошибка", "Здание не найдено"),
    }
}

fn admin_clan(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    action: crate::game::ClanAction,
) -> CommandEffects {
    let create_reserved = match &action {
        crate::game::ClanAction::Create { .. } => {
            if let Err(message) = context.reserve_clan_create(player_id) {
                return context.slash_ok_effect(session_id, player_id, "Ошибка", message);
            }
            true
        }
        crate::game::ClanAction::Leave
        | crate::game::ClanAction::Kick { .. }
        | crate::game::ClanAction::AcceptInvite { .. }
        | crate::game::ClanAction::DeclineInvite { .. }
        | crate::game::ClanAction::AcceptRequest { .. }
        | crate::game::ClanAction::DeclineRequest { .. }
        | crate::game::ClanAction::Promote { .. }
        | crate::game::ClanAction::KickById { .. }
        | crate::game::ClanAction::Invite { .. }
        | crate::game::ClanAction::Request { .. } => false,
        crate::game::ClanAction::Invalid { .. } => unreachable!("handled above"),
    };
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::ClanCommand {
            request: crate::game::ClanCommandRequest {
                player_id,
                session_id,
                action,
                create_reserved,
            },
        }],
        broadcasts: Vec::new(),
    }
}

fn admin_role(
    context: &crate::game::logic::kernel_context::KernelContext<'_>,
    player_id: crate::game::PlayerId,
    session_id: crate::game::SessionId,
    target: String,
    role: openmines_storage::players::Role,
) -> CommandEffects {
    if !context.is_admin(player_id) {
        return context.slash_ok_effect(
            session_id,
            player_id,
            "Ошибка",
            "Нет прав на админ-команду",
        );
    }
    CommandEffects {
        events: Vec::new(),
        saves: vec![crate::game::SaveCommand::AdminRole {
            request: crate::game::AdminRoleRequest {
                player_id,
                session_id,
                target_name: target,
                role,
            },
        }],
        broadcasts: Vec::new(),
    }
}
