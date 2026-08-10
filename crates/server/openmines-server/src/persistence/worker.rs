use super::{
    Arc, BATCH_LIMIT, BUILDING_DELETE_MAX_ATTEMPTS, PersistenceEnvelope, PersistenceStatus,
    PersistenceStore, PersistenceStoreFailure, RETRY_INITIAL_BACKOFF, RETRY_MAX_BACKOFF,
    SaveCommand, SaveKind,
};

#[allow(dead_code)]
pub(super) async fn load_messages_from_db(
    db: &crate::db::Database,
    tag: &str,
    last_id: i64,
) -> Result<Vec<openmines_protocol::chat::ChatMessage>, PersistenceStoreFailure> {
    let rows = db
        .get_recent_chat_messages(tag, openmines_protocol::chat::CHAT_HISTORY_LIMIT)
        .await
        .map_err(PersistenceStoreFailure::Transient)?;
    Ok(rows
        .into_iter()
        .filter(|(id, ..)| *id > last_id)
        .map(|(id, name, text, ts, player_id, color, clan_id)| {
            openmines_protocol::chat::ChatMessage {
                id,
                time: openmines_protocol::chat::dotnet_epoch_minutes(ts),
                clan_id,
                user_id: player_id,
                nickname: name,
                text,
                color,
            }
        })
        .collect())
}

#[allow(dead_code)]
pub(super) async fn get_latest_preview(
    db: &crate::db::Database,
    tag: &str,
) -> Result<String, PersistenceStoreFailure> {
    let rows = db
        .get_recent_chat_messages(tag, 1)
        .await
        .map_err(PersistenceStoreFailure::Transient)?;
    Ok(rows
        .first()
        .map(|(_, n, t, ..)| format!("{n}: {t}"))
        .unwrap_or_default())
}

pub(super) async fn run_worker<S>(
    store: S,
    mut rx: tokio::sync::mpsc::Receiver<PersistenceEnvelope>,
    status: Arc<PersistenceStatus>,
    simulation_waker: crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    let _wake_owner_on_exit = WakeOwnerOnDrop(simulation_waker.clone());
    while let Some(first) = rx.recv().await {
        let mut batch = Vec::with_capacity(BATCH_LIMIT);
        batch.push(first);
        while batch.len() < BATCH_LIMIT {
            let Ok(next) = rx.try_recv() else {
                break;
            };
            batch.push(next);
        }

        simulation_waker.wake();
        persist_batch(&store, &mut batch, &simulation_waker).await;
        status.mark_completed(batch.len());
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(0.0);
    }
    crate::metrics::PERSISTENCE_QUEUE_DEPTH.set(0);
}

struct WakeOwnerOnDrop(crate::simulation_waker::SimulationWaker);

impl Drop for WakeOwnerOnDrop {
    fn drop(&mut self) {
        self.0.wake();
    }
}

async fn persist_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    let mut start = 0usize;
    while start < batch.len() {
        let kind = batch[start].command.kind();
        let end = batch[start..]
            .iter()
            .position(|envelope| envelope.command.kind() != kind)
            .map_or(batch.len(), |offset| start + offset);
        match kind {
            SaveKind::ProgramCreate => {
                persist_program_create_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ProgramMenu => {
                persist_program_menu_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ProgramOpen => {
                persist_program_open_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ProgramRename => {
                persist_program_rename_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ProgramDelete => {
                persist_program_delete_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ProgramCopy => {
                persist_program_copy_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::BuildingMenu => {
                persist_building_menu_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::AuctionGrid => {
                persist_auction_grid_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::AuctionItemOrders => {
                persist_auction_item_orders_batch(store, &mut batch[start..end], simulation_waker)
                    .await;
            }
            SaveKind::AuctionOrder => {
                persist_auction_order_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::AuctionOrderCreate => {
                persist_auction_order_create_batch(store, &mut batch[start..end], simulation_waker)
                    .await;
            }
            SaveKind::AuctionBet => {
                persist_auction_bet_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::Program => {
                persist_program_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::BuildingDelete => {
                persist_building_delete_batch(store, &mut batch[start..end], simulation_waker)
                    .await;
            }
            SaveKind::ChatColorCycle => {
                persist_chat_color_cycle_batch(store, &mut batch[start..end], simulation_waker)
                    .await;
            }
            SaveKind::ChatResync => {
                persist_chat_resync_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ChatMenu => {
                persist_chat_menu_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ChatPrivate => {
                persist_chat_private_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::Whois => {
                persist_whois_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ClanMenu => {
                persist_clan_menu_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::AdminMoneyAll => {
                persist_admin_money_all_batch(store, &mut batch[start..end], simulation_waker)
                    .await;
            }
            SaveKind::AdminRole => {
                persist_admin_role_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::AdminSkill => {
                persist_admin_skill_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::ClanCommand => {
                persist_clan_command_batch(store, &mut batch[start..end], simulation_waker).await;
            }
            SaveKind::Player | SaveKind::Building | SaveKind::Box | SaveKind::ChatAppend => {
                persist_compatible_batch(store, kind, &batch[start..end]).await;
            }
        }
        start = end;
    }
}

async fn persist_admin_money_all_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::AdminMoneyAll { request } = &envelope.command else {
            unreachable!("compatible admin-moneyall batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.admin_money_all(&request).await {
                Ok(affected_players) => {
                    break crate::game::AdminMoneyAllResult::Applied { affected_players };
                }
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::AdminMoneyAllResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::AdminMoneyAll.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Admin moneyall persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("admin moneyall command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::AdminMoneyAllApplied { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::AdminMoneyAll.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_admin_role_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::AdminRole { request } = &envelope.command else {
            unreachable!("compatible admin-role batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.admin_role(&request).await {
                Ok(result) => break result,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::AdminRoleResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::AdminRole.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Admin role persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("admin role command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::AdminRoleApplied { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::AdminRole.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_admin_skill_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::AdminSkill { request } = &envelope.command else {
            unreachable!("compatible admin-skill batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.admin_skill(&request).await {
                Ok(()) => break crate::game::AdminSkillResult::Saved,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::AdminSkillResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::AdminSkill.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Admin skill persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("admin skill command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::AdminSkillApplied { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::AdminSkill.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

macro_rules! persist_action_batch {
    ($name:ident, $command:ident, $kind:expr, $method:ident, $permanent:path, $completion:ident, $label:literal) => {
        async fn $name<S>(store: &S, batch: &mut [PersistenceEnvelope], simulation_waker: &crate::simulation_waker::SimulationWaker) where S: PersistenceStore {
            for envelope in batch {
                let SaveCommand::$command { request } = &envelope.command else { unreachable!(concat!("compatible ", $label, " batch")); };
                let request = request.clone();
                let oldest = envelope.enqueued_at;
                let mut attempt = 0u64;
                let mut backoff = RETRY_INITIAL_BACKOFF;
                let result = loop {
                    crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
                    match store.$method(&request).await {
                        Ok(result) => break result,
                        Err(PersistenceStoreFailure::Permanent(error)) => break { $permanent { message: error.to_string() } },
                        Err(PersistenceStoreFailure::Transient(error)) => {
                            attempt = attempt.saturating_add(1);
                            crate::metrics::PERSISTENCE_COMMANDS_TOTAL.with_label_values(&[$kind.name(), "retry"]).inc();
                            tracing::warn!(attempt, ?backoff, error = ?error, player_id = %request.player_id, $label);
                            tokio::time::sleep(backoff).await;
                            backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                        }
                    }
                };
                envelope.completion.take().expect(concat!($label, " command must reserve completion capacity")).send(crate::game::PersistenceCompletion::$completion { request, result });
                simulation_waker.wake();
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL.with_label_values(&[$kind.name(), "persisted"]).inc();
                crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
            }
        }
    };
}

persist_action_batch!(
    persist_clan_command_batch,
    ClanCommand,
    SaveKind::ClanCommand,
    clan_command,
    crate::game::ClanCommandResult::PermanentFailure,
    ClanCommandApplied,
    "Clan command persistence failed transiently; retrying"
);

persist_action_batch!(
    persist_auction_item_orders_batch,
    AuctionItemOrders,
    SaveKind::AuctionItemOrders,
    auction_item_orders,
    crate::game::AuctionItemOrdersResult::PermanentFailure,
    AuctionItemOrdersLoaded,
    "Auction item orders persistence failed transiently; retrying"
);

persist_action_batch!(
    persist_auction_order_batch,
    AuctionOrder,
    SaveKind::AuctionOrder,
    auction_order,
    crate::game::AuctionOrderResult::PermanentFailure,
    AuctionOrderLoaded,
    "Auction order persistence failed transiently; retrying"
);

persist_action_batch!(
    persist_auction_order_create_batch,
    AuctionOrderCreate,
    SaveKind::AuctionOrderCreate,
    auction_order_create,
    crate::game::AuctionOrderCreateResult::PermanentFailure,
    AuctionOrderCreated,
    "Auction order creation persistence failed transiently; retrying"
);

persist_action_batch!(
    persist_auction_bet_batch,
    AuctionBet,
    SaveKind::AuctionBet,
    auction_bet,
    crate::game::AuctionBetResult::PermanentFailure,
    AuctionBetCompleted,
    "Auction bet persistence failed transiently; retrying"
);

persist_action_batch!(
    persist_auction_grid_batch,
    AuctionGrid,
    SaveKind::AuctionGrid,
    auction_grid,
    crate::game::AuctionGridResult::PermanentFailure,
    AuctionGridLoaded,
    "Auction grid persistence failed transiently; retrying"
);
async fn persist_chat_color_cycle_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ChatColorCycle { request } = &envelope.command else {
            unreachable!("compatible chat color cycle batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.cycle_chat_color(&request).await {
                Ok(Some(color)) => break crate::game::ChatColorCycleResult::Cycled { color },
                Ok(None) => break crate::game::ChatColorCycleResult::Rejected,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ChatColorCycleResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::ChatColorCycle.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Chat color persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("chat color cycle command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ChatColorCycled { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ChatColorCycle.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_chat_resync_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ChatResync { request } = &envelope.command else {
            unreachable!("compatible chat resync batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.chat_resync(&request).await {
                Ok(res) => break res,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ChatResyncResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::ChatResync.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Chat resync persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("chat resync command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ChatResynced { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ChatResync.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_chat_menu_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ChatMenu { request } = &envelope.command else {
            unreachable!("compatible chat menu batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.chat_menu(&request).await {
                Ok(res) => break res,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ChatMenuResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::ChatMenu.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Chat menu persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("chat menu command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ChatMenuLoaded { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ChatMenu.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_chat_private_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ChatPrivate { request } = &envelope.command else {
            unreachable!("compatible chat private batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.chat_private(&request).await {
                Ok(res) => break res,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ChatPrivateResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::ChatPrivate.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Chat private persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("chat private command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ChatPrivateOpened { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ChatPrivate.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

persist_action_batch!(
    persist_whois_batch,
    Whois,
    SaveKind::Whois,
    whois,
    crate::game::WhoisResult::PermanentFailure,
    WhoisLoaded,
    "Whois persistence failed transiently; retrying"
);

persist_action_batch!(
    persist_clan_menu_batch,
    ClanMenu,
    SaveKind::ClanMenu,
    clan_menu,
    crate::game::ClanMenuResult::PermanentFailure,
    ClanMenuLoaded,
    "Clan menu persistence failed transiently; retrying"
);
async fn persist_building_delete_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::BuildingDelete { request } = &envelope.command else {
            unreachable!("compatible building-delete batch");
        };
        let request = request.clone();
        let write = crate::db::BuildingDeleteWrite {
            building_id: request.expected.building_id,
            x: request.expected.x,
            y: request.expected.y,
            clear_resp_bindings: request.view.pack_type == crate::game::PackType::Resp,
            box_write: request.box_write.clone(),
        };
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.delete_building(&write).await {
                Ok(crate::db::BuildingDeleteOutcome::Deleted {
                    cleared_resp_bindings,
                }) => {
                    break crate::game::BuildingDeleteResult::Deleted {
                        cleared_resp_bindings,
                    };
                }
                Ok(crate::db::BuildingDeleteOutcome::IdentityMismatch) => {
                    break crate::game::BuildingDeleteResult::IdentityMismatch;
                }
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::BuildingDeleteResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::BuildingDelete.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        building_id = request.expected.building_id,
                        x = request.expected.x,
                        y = request.expected.y,
                        "Building delete failed transiently; retrying"
                    );
                    if attempt >= BUILDING_DELETE_MAX_ATTEMPTS {
                        break crate::game::BuildingDeleteResult::PermanentFailure {
                            message: error.to_string(),
                        };
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };

        envelope
            .completion
            .take()
            .expect("building-delete command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::BuildingDeleted { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::BuildingDelete.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_program_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::Program { request } = &envelope.command else {
            unreachable!("compatible program batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.save_program(&request).await {
                Ok(Some(program)) => {
                    break crate::game::ProgramSaveResult::Saved {
                        program_name: program.name,
                    };
                }
                Ok(None) => break crate::game::ProgramSaveResult::Rejected,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ProgramSaveResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::Program.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        program_id = request.program_id,
                        "Program persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };

        envelope
            .completion
            .take()
            .expect("program command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ProgramSaved { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::Program.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_program_create_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ProgramCreate { request } = &envelope.command else {
            unreachable!("compatible program create batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.create_program(&request).await {
                Ok(program_id) => {
                    break crate::game::ProgramCreateResult::Created { program_id };
                }
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ProgramCreateResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::Program.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Program create failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };

        envelope
            .completion
            .take()
            .expect("program create command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ProgramCreated { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::Program.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_program_menu_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ProgramMenu { request } = &envelope.command else {
            unreachable!("compatible program-menu batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.program_menu(&request).await {
                Ok(programs) => break crate::game::ProgramMenuResult::Loaded { programs },
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ProgramMenuResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::ProgramMenu.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player_id,
                        "Program menu persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("program menu command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ProgramMenuLoaded { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ProgramMenu.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_program_open_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ProgramOpen { request } = &envelope.command else {
            unreachable!("compatible program-open batch");
        };
        let request = request.clone();
        let result = loop {
            match store.program_open(&request).await {
                Ok(result) => break result,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ProgramOpenResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    tracing::warn!(error = ?error, player_id = %request.player_id, "Program open persistence failed transiently; retrying");
                    tokio::time::sleep(RETRY_INITIAL_BACKOFF).await;
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("program open command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ProgramOpened { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ProgramOpen.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_program_rename_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ProgramRename { request } = &envelope.command else {
            unreachable!("compatible program-rename batch");
        };
        let request = request.clone();
        let result = loop {
            match store.program_rename(&request).await {
                Ok(result) => break result,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ProgramRenameResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    tracing::warn!(error = ?error, player_id = %request.player_id, "Program rename persistence failed transiently; retrying");
                    tokio::time::sleep(RETRY_INITIAL_BACKOFF).await;
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("program rename command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ProgramRenamed { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ProgramRename.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_program_delete_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ProgramDelete { request } = &envelope.command else {
            unreachable!("compatible program-delete batch");
        };
        let request = request.clone();
        let result = loop {
            match store.program_delete(&request).await {
                Ok(result) => break result,
                Err(PersistenceStoreFailure::Permanent(_)) => {
                    break crate::game::ProgramDeleteResult::PermanentFailure;
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    tracing::warn!(
                        error = ?error,
                        player_id = %request.player_id,
                        program_id = request.program_id,
                        "Program delete persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(RETRY_INITIAL_BACKOFF).await;
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("program delete command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ProgramDeleted { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ProgramDelete.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_program_copy_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::ProgramCopy { request } = &envelope.command else {
            unreachable!("compatible program-copy batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.copy_program(&request).await {
                Ok(true) => break crate::game::ProgramCopyResult::Copied,
                Ok(false) => break crate::game::ProgramCopyResult::Rejected,
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::ProgramCopyResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::ProgramCopy.name(), "retry"])
                        .inc();
                    tracing::warn!(
                        attempt,
                        ?backoff,
                        error = ?error,
                        player_id = %request.player,
                        program_id = request.program,
                        "Program copy persistence failed transiently; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("program copy command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::ProgramCopied { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::ProgramCopy.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

async fn persist_building_menu_batch<S>(
    store: &S,
    batch: &mut [PersistenceEnvelope],
    simulation_waker: &crate::simulation_waker::SimulationWaker,
) where
    S: PersistenceStore,
{
    for envelope in batch {
        let SaveCommand::BuildingMenu { request } = &envelope.command else {
            unreachable!("compatible building-menu batch");
        };
        let request = request.clone();
        let oldest = envelope.enqueued_at;
        let mut attempt = 0u64;
        let mut backoff = RETRY_INITIAL_BACKOFF;
        let result = loop {
            crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
            match store.building_menu(&request).await {
                Ok(buildings) => break crate::game::BuildingMenuResult::Loaded { buildings },
                Err(PersistenceStoreFailure::Permanent(error)) => {
                    break crate::game::BuildingMenuResult::PermanentFailure {
                        message: error.to_string(),
                    };
                }
                Err(PersistenceStoreFailure::Transient(error)) => {
                    attempt = attempt.saturating_add(1);
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[SaveKind::BuildingMenu.name(), "retry"])
                        .inc();
                    tracing::warn!(attempt, ?backoff, error = ?error, player_id = %request.player_id, "Building menu persistence failed transiently; retrying");
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
                }
            }
        };
        envelope
            .completion
            .take()
            .expect("building menu command must reserve completion capacity")
            .send(crate::game::PersistenceCompletion::BuildingMenuLoaded { request, result });
        simulation_waker.wake();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[SaveKind::BuildingMenu.name(), "persisted"])
            .inc();
        crate::metrics::PERSISTENCE_BATCH_SIZE.observe(1.0);
    }
}

#[allow(clippy::too_many_lines)]
async fn persist_compatible_batch<S>(store: &S, kind: SaveKind, batch: &[PersistenceEnvelope])
where
    S: PersistenceStore,
{
    let oldest = batch[0].enqueued_at;
    let mut attempt = 0u64;
    let mut backoff = RETRY_INITIAL_BACKOFF;
    loop {
        crate::metrics::PERSISTENCE_OLDEST_AGE_SECONDS.set(oldest.elapsed().as_secs_f64());
        let result = match kind {
            SaveKind::Player => {
                let rows = batch
                    .iter()
                    .map(|envelope| match &envelope.command {
                        SaveCommand::Player { row } => row.as_ref().clone(),
                        SaveCommand::Building { .. }
                        | SaveCommand::Box { .. }
                        | SaveCommand::Program { .. }
                        | SaveCommand::ProgramCreate { .. }
                        | SaveCommand::ProgramMenu { .. }
                        | SaveCommand::ProgramOpen { .. }
                        | SaveCommand::ProgramRename { .. }
                        | SaveCommand::ProgramDelete { .. }
                        | SaveCommand::ProgramCopy { .. }
                        | SaveCommand::BuildingMenu { .. }
                        | SaveCommand::AuctionGrid { .. }
                        | SaveCommand::AuctionItemOrders { .. }
                        | SaveCommand::AuctionOrder { .. }
                        | SaveCommand::AuctionOrderCreate { .. }
                        | SaveCommand::AuctionBet { .. }
                        | SaveCommand::ChatAppend { .. }
                        | SaveCommand::BuildingDelete { .. }
                        | SaveCommand::ChatColorCycle { .. }
                        | SaveCommand::ChatResync { .. }
                        | SaveCommand::ChatMenu { .. }
                        | SaveCommand::ChatPrivate { .. }
                        | SaveCommand::Whois { .. }
                        | SaveCommand::ClanMenu { .. }
                        | SaveCommand::AdminMoneyAll { .. }
                        | SaveCommand::AdminRole { .. }
                        | SaveCommand::AdminSkill { .. }
                        | SaveCommand::ClanCommand { .. } => {
                            unreachable!("compatible player batch")
                        }
                    })
                    .collect::<Vec<_>>();
                store.save_players_batch(&rows).await
            }
            SaveKind::Building => {
                let rows = batch
                    .iter()
                    .map(|envelope| match &envelope.command {
                        SaveCommand::Building { row } => row.as_ref().clone(),
                        SaveCommand::Player { .. }
                        | SaveCommand::Box { .. }
                        | SaveCommand::Program { .. }
                        | SaveCommand::ProgramCreate { .. }
                        | SaveCommand::ProgramMenu { .. }
                        | SaveCommand::ProgramOpen { .. }
                        | SaveCommand::ProgramRename { .. }
                        | SaveCommand::ProgramDelete { .. }
                        | SaveCommand::ProgramCopy { .. }
                        | SaveCommand::BuildingMenu { .. }
                        | SaveCommand::AuctionGrid { .. }
                        | SaveCommand::AuctionItemOrders { .. }
                        | SaveCommand::AuctionOrder { .. }
                        | SaveCommand::AuctionOrderCreate { .. }
                        | SaveCommand::AuctionBet { .. }
                        | SaveCommand::ChatAppend { .. }
                        | SaveCommand::BuildingDelete { .. }
                        | SaveCommand::ChatColorCycle { .. }
                        | SaveCommand::ChatResync { .. }
                        | SaveCommand::ChatMenu { .. }
                        | SaveCommand::ChatPrivate { .. }
                        | SaveCommand::Whois { .. }
                        | SaveCommand::ClanMenu { .. }
                        | SaveCommand::AdminMoneyAll { .. }
                        | SaveCommand::AdminRole { .. }
                        | SaveCommand::AdminSkill { .. }
                        | SaveCommand::ClanCommand { .. } => {
                            unreachable!("compatible building batch")
                        }
                    })
                    .collect::<Vec<_>>();
                store.save_buildings_batch(&rows).await
            }
            SaveKind::Box => {
                let writes = batch
                    .iter()
                    .map(|envelope| match &envelope.command {
                        SaveCommand::Box { write } => write.clone(),
                        SaveCommand::Player { .. }
                        | SaveCommand::Building { .. }
                        | SaveCommand::Program { .. }
                        | SaveCommand::ProgramCreate { .. }
                        | SaveCommand::ProgramMenu { .. }
                        | SaveCommand::ProgramOpen { .. }
                        | SaveCommand::ProgramRename { .. }
                        | SaveCommand::ProgramDelete { .. }
                        | SaveCommand::ProgramCopy { .. }
                        | SaveCommand::BuildingMenu { .. }
                        | SaveCommand::AuctionGrid { .. }
                        | SaveCommand::AuctionItemOrders { .. }
                        | SaveCommand::AuctionOrder { .. }
                        | SaveCommand::AuctionOrderCreate { .. }
                        | SaveCommand::AuctionBet { .. }
                        | SaveCommand::ChatAppend { .. }
                        | SaveCommand::BuildingDelete { .. }
                        | SaveCommand::ChatColorCycle { .. }
                        | SaveCommand::ChatResync { .. }
                        | SaveCommand::ChatMenu { .. }
                        | SaveCommand::ChatPrivate { .. }
                        | SaveCommand::Whois { .. }
                        | SaveCommand::ClanMenu { .. }
                        | SaveCommand::AdminMoneyAll { .. }
                        | SaveCommand::AdminRole { .. }
                        | SaveCommand::AdminSkill { .. }
                        | SaveCommand::ClanCommand { .. } => {
                            unreachable!("compatible box batch")
                        }
                    })
                    .collect::<Vec<_>>();
                store.save_boxes_batch(&writes).await
            }
            SaveKind::ChatAppend => {
                let requests = batch
                    .iter()
                    .map(|envelope| match &envelope.command {
                        SaveCommand::ChatAppend { request } => request.clone(),
                        SaveCommand::Player { .. }
                        | SaveCommand::Building { .. }
                        | SaveCommand::Box { .. }
                        | SaveCommand::Program { .. }
                        | SaveCommand::ProgramCreate { .. }
                        | SaveCommand::ProgramMenu { .. }
                        | SaveCommand::ProgramOpen { .. }
                        | SaveCommand::ProgramRename { .. }
                        | SaveCommand::ProgramDelete { .. }
                        | SaveCommand::ProgramCopy { .. }
                        | SaveCommand::BuildingMenu { .. }
                        | SaveCommand::AuctionGrid { .. }
                        | SaveCommand::AuctionItemOrders { .. }
                        | SaveCommand::AuctionOrder { .. }
                        | SaveCommand::AuctionOrderCreate { .. }
                        | SaveCommand::AuctionBet { .. }
                        | SaveCommand::BuildingDelete { .. }
                        | SaveCommand::ChatColorCycle { .. }
                        | SaveCommand::ChatResync { .. }
                        | SaveCommand::ChatMenu { .. }
                        | SaveCommand::ChatPrivate { .. }
                        | SaveCommand::Whois { .. }
                        | SaveCommand::ClanMenu { .. }
                        | SaveCommand::AdminMoneyAll { .. }
                        | SaveCommand::AdminRole { .. }
                        | SaveCommand::AdminSkill { .. }
                        | SaveCommand::ClanCommand { .. } => {
                            unreachable!("compatible chat batch")
                        }
                    })
                    .collect::<Vec<_>>();
                store.save_chat_messages_batch(&requests).await
            }
            SaveKind::Program
            | SaveKind::ProgramCreate
            | SaveKind::ProgramMenu
            | SaveKind::ProgramOpen
            | SaveKind::ProgramRename
            | SaveKind::ProgramDelete
            | SaveKind::ProgramCopy
            | SaveKind::BuildingMenu
            | SaveKind::AuctionGrid
            | SaveKind::AuctionItemOrders
            | SaveKind::AuctionOrder
            | SaveKind::AuctionOrderCreate
            | SaveKind::AuctionBet
            | SaveKind::BuildingDelete
            | SaveKind::ChatColorCycle
            | SaveKind::ChatResync
            | SaveKind::ChatMenu
            | SaveKind::ChatPrivate
            | SaveKind::Whois
            | SaveKind::ClanMenu
            | SaveKind::AdminMoneyAll
            | SaveKind::AdminRole
            | SaveKind::AdminSkill
            | SaveKind::ClanCommand => {
                unreachable!("completion command routed to compatible batch")
            }
        };
        match result {
            Ok(()) => {
                let batch_size =
                    u32::try_from(batch.len()).expect("persistence batch limit fits u32");
                crate::metrics::PERSISTENCE_BATCH_SIZE.observe(f64::from(batch_size));
                if kind == SaveKind::Player {
                    crate::metrics::PLAYER_SAVE_TOTAL
                        .inc_by(u64::try_from(batch.len()).unwrap_or(u64::MAX));
                }
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                    .with_label_values(&[kind.name(), "persisted"])
                    .inc_by(u64::try_from(batch.len()).unwrap_or(u64::MAX));
                return;
            }
            Err(error) => {
                attempt = attempt.saturating_add(1);
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                    .with_label_values(&[kind.name(), "retry"])
                    .inc();
                tracing::warn!(
                    attempt,
                    ?backoff,
                    error = ?error,
                    batch_size = batch.len(),
                    "Persistence batch failed; retrying without dropping durable commands"
                );
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2).min(RETRY_MAX_BACKOFF);
            }
        }
    }
}
