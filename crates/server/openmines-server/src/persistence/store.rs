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
use std::future::Future;
use std::sync::Arc;

#[derive(Debug)]
pub enum PersistenceStoreFailure {
    Transient(anyhow::Error),
    Permanent(anyhow::Error),
}

pub trait PersistenceStore: Clone + Send + Sync + 'static {
    fn save_players_batch(
        &self,
        players: &[crate::db::PlayerRow],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn save_buildings_batch(
        &self,
        buildings: &[crate::db::buildings::BuildingRow],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn save_resp_profit_batch(
        &self,
        transfers: &[(crate::db::PlayerRow, crate::db::buildings::BuildingRow)],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn save_boxes_batch(
        &self,
        writes: &[crate::db::BoxWrite],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn save_chat_messages_batch(
        &self,
        messages: &[crate::game::ChatAppendRequest],
    ) -> impl Future<Output = anyhow::Result<()>> + Send;

    fn save_program(
        &self,
        request: &crate::game::ProgramSaveRequest,
    ) -> impl Future<Output = Result<Option<crate::db::ProgramRow>, PersistenceStoreFailure>> + Send;

    fn create_program(
        &self,
        request: &crate::game::ProgramCreateRequest,
    ) -> impl Future<Output = Result<i32, PersistenceStoreFailure>> + Send;

    fn delete_building(
        &self,
        write: &crate::db::BuildingDeleteWrite,
    ) -> impl Future<Output = Result<crate::db::BuildingDeleteOutcome, PersistenceStoreFailure>> + Send;

    fn cycle_chat_color(
        &self,
        request: &crate::game::ChatColorCycleRequest,
    ) -> impl Future<Output = Result<Option<i32>, PersistenceStoreFailure>> + Send;

    fn admin_money_all(
        &self,
        request: &crate::game::AdminMoneyAllRequest,
    ) -> impl Future<Output = Result<u64, PersistenceStoreFailure>> + Send;

    fn admin_role(
        &self,
        request: &crate::game::AdminRoleRequest,
    ) -> impl Future<Output = Result<crate::game::AdminRoleResult, PersistenceStoreFailure>> + Send;

    fn admin_skill(
        &self,
        request: &crate::game::AdminSkillRequest,
    ) -> impl Future<Output = Result<(), PersistenceStoreFailure>> + Send;

    fn clan_command(
        &self,
        request: &crate::game::ClanCommandRequest,
    ) -> impl Future<Output = Result<crate::game::ClanCommandResult, PersistenceStoreFailure>> + Send;

    fn chat_resync(
        &self,
        request: &crate::game::ChatResyncRequest,
    ) -> impl Future<Output = Result<crate::game::ChatResyncResult, PersistenceStoreFailure>> + Send;

    fn chat_menu(
        &self,
        request: &crate::game::ChatMenuRequest,
    ) -> impl Future<Output = Result<crate::game::ChatMenuResult, PersistenceStoreFailure>> + Send;

    fn chat_private(
        &self,
        request: &crate::game::ChatPrivateRequest,
    ) -> impl Future<Output = Result<crate::game::ChatPrivateResult, PersistenceStoreFailure>> + Send;

    fn whois(
        &self,
        request: &crate::game::WhoisRequest,
    ) -> impl Future<Output = Result<crate::game::WhoisResult, PersistenceStoreFailure>> + Send;

    fn clan_menu(
        &self,
        request: &crate::game::ClanMenuRequest,
    ) -> impl Future<Output = Result<crate::game::ClanMenuResult, PersistenceStoreFailure>> + Send;

    fn program_menu(
        &self,
        request: &crate::game::ProgramMenuRequest,
    ) -> impl Future<Output = Result<Vec<crate::db::ProgramRow>, PersistenceStoreFailure>> + Send;

    fn program_open(
        &self,
        request: &crate::game::ProgramOpenRequest,
    ) -> impl Future<Output = Result<crate::game::ProgramOpenResult, PersistenceStoreFailure>> + Send;

    fn program_rename(
        &self,
        request: &crate::game::ProgramRenameRequest,
    ) -> impl Future<Output = Result<crate::game::ProgramRenameResult, PersistenceStoreFailure>> + Send;

    fn program_delete(
        &self,
        request: &crate::game::ProgramDeleteRequest,
    ) -> impl Future<Output = Result<crate::game::ProgramDeleteResult, PersistenceStoreFailure>> + Send;

    fn copy_program(
        &self,
        request: &crate::game::ProgramCopyRequest,
    ) -> impl Future<Output = Result<bool, PersistenceStoreFailure>> + Send;

    fn building_menu(
        &self,
        request: &crate::game::BuildingMenuRequest,
    ) -> impl Future<Output = Result<Vec<crate::db::BuildingRow>, PersistenceStoreFailure>> + Send;

    fn auction_grid(
        &self,
        request: &crate::game::AuctionGridRequest,
    ) -> impl Future<Output = Result<crate::game::AuctionGridResult, PersistenceStoreFailure>> + Send;

    fn auction_item_orders(
        &self,
        request: &crate::game::AuctionItemOrdersRequest,
    ) -> impl Future<Output = Result<crate::game::AuctionItemOrdersResult, PersistenceStoreFailure>> + Send;

    fn auction_order(
        &self,
        request: &crate::game::AuctionOrderRequest,
    ) -> impl Future<Output = Result<crate::game::AuctionOrderResult, PersistenceStoreFailure>> + Send;

    fn auction_order_create(
        &self,
        request: &crate::game::AuctionOrderCreateRequest,
    ) -> impl Future<Output = Result<crate::game::AuctionOrderCreateResult, PersistenceStoreFailure>>
    + Send;

    fn auction_bet(
        &self,
        request: &crate::game::AuctionBetRequest,
    ) -> impl Future<Output = Result<crate::game::AuctionBetResult, PersistenceStoreFailure>> + Send;
}

impl PersistenceStore for Arc<crate::db::Database> {
    async fn save_players_batch(&self, players: &[crate::db::PlayerRow]) -> anyhow::Result<()> {
        crate::db::Database::save_players_batch(self, players).await
    }

    async fn save_buildings_batch(
        &self,
        buildings: &[crate::db::BuildingRow],
    ) -> anyhow::Result<()> {
        crate::db::Database::save_buildings_batch(self, buildings).await
    }

    async fn save_resp_profit_batch(
        &self,
        transfers: &[(crate::db::PlayerRow, crate::db::buildings::BuildingRow)],
    ) -> anyhow::Result<()> {
        crate::db::Database::save_resp_profit_batch(self, transfers).await
    }

    async fn save_boxes_batch(&self, writes: &[crate::db::BoxWrite]) -> anyhow::Result<()> {
        crate::db::Database::save_boxes_batch(self, writes).await
    }

    async fn save_chat_messages_batch(
        &self,
        messages: &[crate::game::ChatAppendRequest],
    ) -> anyhow::Result<()> {
        for msg in messages {
            self.add_chat_message(
                msg.id,
                &msg.tag,
                &msg.nickname,
                &msg.text,
                msg.player_id,
                msg.color,
            )
            .await?;
        }
        Ok(())
    }

    async fn save_program(
        &self,
        request: &crate::game::ProgramSaveRequest,
    ) -> Result<Option<crate::db::ProgramRow>, PersistenceStoreFailure> {
        self.save_select_program(
            request.player_id.as_i32(),
            request.program_id,
            &request.source,
        )
        .await
        .map_err(|error| PersistenceStoreFailure::Transient(error.into()))
        .and_then(|result| match result {
            openmines_storage::programs::SaveSelectProgramResult::Saved(program) => {
                Ok(Some(program))
            }
            openmines_storage::programs::SaveSelectProgramResult::ProgramUnavailable => Ok(None),
            openmines_storage::programs::SaveSelectProgramResult::PlayerUnavailable => {
                Err(PersistenceStoreFailure::Permanent(anyhow::anyhow!(
                    "player {} is unavailable for program selection",
                    request.player_id
                )))
            }
        })
    }

    async fn create_program(
        &self,
        request: &crate::game::ProgramCreateRequest,
    ) -> Result<i32, PersistenceStoreFailure> {
        let program_id = self
            .insert_program(request.player_id.into(), &request.name, "")
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        let _ = self
            .set_selected_program(request.player_id.into(), Some(program_id))
            .await;
        Ok(program_id)
    }

    async fn delete_building(
        &self,
        write: &crate::db::BuildingDeleteWrite,
    ) -> Result<crate::db::BuildingDeleteOutcome, PersistenceStoreFailure> {
        self.apply_building_delete(write)
            .await
            .map_err(PersistenceStoreFailure::Transient)
    }

    async fn cycle_chat_color(
        &self,
        request: &crate::game::ChatColorCycleRequest,
    ) -> Result<Option<i32>, PersistenceStoreFailure> {
        self.cycle_chat_color_if_present(request.player_id.as_i32())
            .await
            .map_err(PersistenceStoreFailure::Transient)
    }

    async fn admin_money_all(
        &self,
        request: &crate::game::AdminMoneyAllRequest,
    ) -> Result<u64, PersistenceStoreFailure> {
        let affected = self
            .add_money_to_all(request.amount)
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        Ok(affected as u64)
    }

    async fn admin_role(
        &self,
        request: &crate::game::AdminRoleRequest,
    ) -> Result<crate::game::AdminRoleResult, PersistenceStoreFailure> {
        let target = match self.get_player_by_name(&request.target_name).await {
            Ok(Some(player)) => player,
            Ok(None) => {
                return Ok(crate::game::AdminRoleResult::TargetNotFound {
                    target_name: request.target_name.clone(),
                });
            }
            Err(error) => {
                return Ok(crate::game::AdminRoleResult::PermanentFailure {
                    message: error.to_string(),
                });
            }
        };
        match self.set_player_role(target.id, request.role).await {
            Ok(true) => Ok(crate::game::AdminRoleResult::Applied {
                target_id: crate::game::PlayerId::from(target.id),
                target_name: target.name,
            }),
            Ok(false) => Ok(crate::game::AdminRoleResult::TargetNotFound {
                target_name: request.target_name.clone(),
            }),
            Err(error) => Ok(crate::game::AdminRoleResult::PermanentFailure {
                message: error.to_string(),
            }),
        }
    }

    async fn admin_skill(
        &self,
        request: &crate::game::AdminSkillRequest,
    ) -> Result<(), PersistenceStoreFailure> {
        self.save_player(&request.row)
            .await
            .map_err(PersistenceStoreFailure::Transient)
    }

    async fn clan_command(
        &self,
        request: &crate::game::ClanCommandRequest,
    ) -> Result<crate::game::ClanCommandResult, PersistenceStoreFailure> {
        use crate::game::ClanAction;
        let result = match &request.action {
            ClanAction::Create { name, tag } => {
                let clan_id = match self.pick_clan_id().await {
                    Ok(Some(id)) => id,
                    Ok(None) => {
                        return Ok(crate::game::ClanCommandResult::PermanentFailure {
                            message: "no available clan id".into(),
                        });
                    }
                    Err(e) => {
                        return Ok(crate::game::ClanCommandResult::PermanentFailure {
                            message: e.to_string(),
                        });
                    }
                };
                self.create_clan(clan_id, name, tag, request.player_id.as_i32())
                    .await
                    .map(|()| crate::game::ClanCommandResult::Created { clan_id })
                    .map_err(|e| e.to_string())
            }
            ClanAction::Leave => self
                .leave_clan(request.player_id.as_i32())
                .await
                .map(|()| crate::game::ClanCommandResult::Left {
                    clan_id: 0,
                    disbanded: false,
                })
                .map_err(|e| e.to_string()),
            ClanAction::Kick { target } => {
                let target_id = match self.get_player_by_name(target).await {
                    Ok(Some(p)) => p.id,
                    _ => {
                        return Ok(crate::game::ClanCommandResult::Kicked {
                            clan_id: 0,
                            target_id: crate::game::PlayerId::from(0),
                        });
                    }
                };
                self.kick_from_clan(target_id)
                    .await
                    .map(|()| crate::game::ClanCommandResult::Kicked {
                        clan_id: 0,
                        target_id: crate::game::PlayerId::from(target_id),
                    })
                    .map_err(|e| e.to_string())
            }
            ClanAction::KickById { target_id } => self
                .kick_from_clan(target_id.as_i32())
                .await
                .map(|()| crate::game::ClanCommandResult::Kicked {
                    clan_id: 0,
                    target_id: *target_id,
                })
                .map_err(|e| e.to_string()),
            ClanAction::AcceptInvite { clan_id } => self
                .accept_clan_invite(*clan_id, request.player_id.as_i32())
                .await
                .map(|()| crate::game::ClanCommandResult::Joined { clan_id: *clan_id })
                .map_err(|e| e.to_string()),
            ClanAction::DeclineInvite { clan_id } => self
                .decline_clan_invite(*clan_id, request.player_id.as_i32())
                .await
                .map(|()| crate::game::ClanCommandResult::InviteDeclined)
                .map_err(|e| e.to_string()),
            ClanAction::AcceptRequest { target_id } => self
                .accept_clan_request(request.player_id.as_i32(), target_id.as_i32())
                .await
                .map(|()| crate::game::ClanCommandResult::RequestAccepted {
                    clan_id: request.player_id.as_i32(),
                    target_id: *target_id,
                })
                .map_err(|e| e.to_string()),
            ClanAction::DeclineRequest { target_id } => self
                .decline_clan_request(request.player_id.as_i32(), target_id.as_i32())
                .await
                .map(|()| crate::game::ClanCommandResult::RequestDeclined)
                .map_err(|e| e.to_string()),
            ClanAction::Promote { target_id } => self
                .set_clan_rank(
                    target_id.as_i32(),
                    request.player_id.as_i32(),
                    openmines_storage::clans::ClanRank::Officer,
                )
                .await
                .map(|()| crate::game::ClanCommandResult::Promoted {
                    clan_id: request.player_id.as_i32(),
                    target_id: *target_id,
                })
                .map_err(|e| e.to_string()),
            ClanAction::Invite { target_id } => self
                .add_clan_invite(request.player_id.as_i32(), target_id.as_i32())
                .await
                .map(|()| crate::game::ClanCommandResult::Invited {
                    target_id: *target_id,
                })
                .map_err(|e| e.to_string()),
            ClanAction::Request { clan_id } => self
                .add_clan_request(*clan_id, request.player_id.as_i32())
                .await
                .map(|()| crate::game::ClanCommandResult::RequestSent)
                .map_err(|e| e.to_string()),
            ClanAction::Invalid { message } => Ok(crate::game::ClanCommandResult::Rejected {
                title: "Ошибка".to_string(),
                message: message.clone(),
            }),
        };
        match result {
            Ok(r) => Ok(r),
            Err(message) => Ok(crate::game::ClanCommandResult::PermanentFailure { message }),
        }
    }

    async fn chat_resync(
        &self,
        request: &crate::game::ChatResyncRequest,
    ) -> Result<crate::game::ChatResyncResult, PersistenceStoreFailure> {
        let messages = self
            .get_recent_chat_messages(&request.channel_tag, 200)
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        let channel_name = request.channel_tag.clone();
        let messages = messages
            .into_iter()
            .filter(|m| m.0 > request.last_id)
            .map(|m| openmines_protocol::chat::ChatMessage {
                id: m.0,
                time: m.3,
                clan_id: m.6,
                user_id: m.4,
                nickname: m.1,
                text: m.2,
                color: m.5,
            })
            .collect();
        Ok(crate::game::ChatResyncResult::Success {
            channel_name,
            messages,
        })
    }

    async fn chat_menu(
        &self,
        _request: &crate::game::ChatMenuRequest,
    ) -> Result<crate::game::ChatMenuResult, PersistenceStoreFailure> {
        Ok(crate::game::ChatMenuResult::Success {
            channels: Vec::new(),
        })
    }

    async fn chat_private(
        &self,
        request: &crate::game::ChatPrivateRequest,
    ) -> Result<crate::game::ChatPrivateResult, PersistenceStoreFailure> {
        let tags = self
            .private_chat_tags(request.target_uid.as_i32())
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        let Some(channel_tag) = tags.into_iter().next() else {
            return Ok(crate::game::ChatPrivateResult::TargetNotFound);
        };
        let messages = self
            .get_recent_chat_messages(&channel_tag, 200)
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        let target_name = messages.first().map(|m| m.1.clone()).unwrap_or_default();
        let messages = messages
            .into_iter()
            .map(|m| openmines_protocol::chat::ChatMessage {
                id: m.0,
                time: m.3,
                clan_id: m.6,
                user_id: m.4,
                nickname: m.1,
                text: m.2,
                color: m.5,
            })
            .collect();
        Ok(crate::game::ChatPrivateResult::Success {
            target_name,
            channel_tag,
            messages,
        })
    }

    async fn whois(
        &self,
        request: &crate::game::WhoisRequest,
    ) -> Result<crate::game::WhoisResult, PersistenceStoreFailure> {
        let mut names = Vec::with_capacity(request.online_names.len());
        for (id, name) in &request.online_names {
            let _ = id;
            names.push((*id, name.clone()));
        }
        for id in &request.ids {
            if let Ok(Some(player)) = self.get_player_by_id(*id).await
                && !names.iter().any(|(nid, _)| nid == id)
            {
                names.push((*id, player.name));
            }
        }
        Ok(crate::game::WhoisResult::Loaded { names })
    }

    async fn clan_menu(
        &self,
        request: &crate::game::ClanMenuRequest,
    ) -> Result<crate::game::ClanMenuResult, PersistenceStoreFailure> {
        use crate::game::ClanMenuAction;
        let result = match request.action {
            ClanMenuAction::Main => self
                .list_clans()
                .await
                .map(|clans| {
                    let invites = Vec::new();
                    let clans = clans
                        .into_iter()
                        .map(|c| crate::game::ClanMenuListEntry {
                            id: c.id,
                            name: c.name,
                            abr: c.abr,
                            member_count: 0,
                        })
                        .collect();
                    crate::game::ClanMenuResult::Browse { invites, clans }
                })
                .map_err(|e| e.to_string()),
            ClanMenuAction::Preview { clan_id } => self
                .get_clan(clan_id)
                .await
                .map(|opt| match opt {
                    Some(c) => crate::game::ClanMenuResult::Preview {
                        clan: crate::game::ClanMenuListEntry {
                            id: c.id,
                            name: c.name,
                            abr: c.abr,
                            member_count: 0,
                        },
                        owner_name: String::new(),
                        can_request_join: true,
                    },
                    None => crate::game::ClanMenuResult::NotFound,
                })
                .map_err(|e| e.to_string()),
            ClanMenuAction::Members => {
                let clan_id = request.player_clan_id.unwrap_or(0);
                self.get_clan_members(clan_id)
                    .await
                    .map(|members| {
                        let members = members
                            .into_iter()
                            .map(|(id, name, rank)| crate::game::ClanMemberEntry {
                                player_id: id,
                                name,
                                rank,
                            })
                            .collect();
                        crate::game::ClanMenuResult::Members {
                            members,
                            player_rank: 0,
                        }
                    })
                    .map_err(|e| e.to_string())
            }
            ClanMenuAction::InviteList => {
                let allowed = true;
                let candidates = request.invite_candidates.clone();
                Ok(crate::game::ClanMenuResult::InviteList {
                    allowed,
                    candidates,
                })
            }
            ClanMenuAction::Invites => self
                .get_player_invites(request.player_id.as_i32())
                .await
                .map(|invites| {
                    let invites = invites.into_iter().collect();
                    crate::game::ClanMenuResult::Invites { invites }
                })
                .map_err(|e| e.to_string()),
            ClanMenuAction::Requests => {
                let clan_id = request.player_clan_id.unwrap_or(0);
                self.get_clan_requests(clan_id)
                    .await
                    .map(|requests| {
                        let requests = requests.into_iter().collect();
                        crate::game::ClanMenuResult::Requests {
                            allowed: true,
                            requests,
                        }
                    })
                    .map_err(|e| e.to_string())
            }
        };
        match result {
            Ok(r) => Ok(r),
            Err(message) => Ok(crate::game::ClanMenuResult::PermanentFailure { message }),
        }
    }

    async fn program_menu(
        &self,
        request: &crate::game::ProgramMenuRequest,
    ) -> Result<Vec<crate::db::ProgramRow>, PersistenceStoreFailure> {
        self.list_programs(request.player_id.as_i32())
            .await
            .map_err(PersistenceStoreFailure::Transient)
    }

    async fn program_open(
        &self,
        request: &crate::game::ProgramOpenRequest,
    ) -> Result<crate::game::ProgramOpenResult, PersistenceStoreFailure> {
        let Some(program) = self
            .get_program(request.program)
            .await
            .map_err(PersistenceStoreFailure::Transient)?
        else {
            return Ok(crate::game::ProgramOpenResult::Rejected);
        };
        if program.player_id != request.player_id.as_i32() {
            return Ok(crate::game::ProgramOpenResult::Rejected);
        }
        self.set_selected_program(request.player_id.into(), Some(program.id))
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        Ok(crate::game::ProgramOpenResult::Opened { program })
    }

    async fn program_rename(
        &self,
        request: &crate::game::ProgramRenameRequest,
    ) -> Result<crate::game::ProgramRenameResult, PersistenceStoreFailure> {
        let Some(program) = self
            .get_program(request.program_id)
            .await
            .map_err(PersistenceStoreFailure::Transient)?
        else {
            return Ok(crate::game::ProgramRenameResult::Rejected);
        };
        if program.player_id != request.player_id.as_i32() {
            return Ok(crate::game::ProgramRenameResult::Rejected);
        }
        self.rename_program(request.program_id, &request.name)
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        Ok(crate::game::ProgramRenameResult::Renamed {
            program: crate::db::ProgramRow {
                name: request.name.clone(),
                ..program
            },
        })
    }

    async fn program_delete(
        &self,
        request: &crate::game::ProgramDeleteRequest,
    ) -> Result<crate::game::ProgramDeleteResult, PersistenceStoreFailure> {
        let deleted = self
            .delete_program_owned(request.player_id.into(), request.program_id)
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        if !deleted {
            return Ok(crate::game::ProgramDeleteResult::Rejected);
        }
        if request.clear_selected
            && let Err(error) = self
                .set_selected_program(request.player_id.into(), None)
                .await
        {
            tracing::error!(
                player_id = %request.player_id,
                program_id = request.program_id,
                error = ?error,
                "Selected program clear failed after delete"
            );
        }
        Ok(crate::game::ProgramDeleteResult::Deleted)
    }

    async fn copy_program(
        &self,
        request: &crate::game::ProgramCopyRequest,
    ) -> Result<bool, PersistenceStoreFailure> {
        let source = match self.get_program(request.program).await {
            Ok(Some(program)) if program.player_id == request.player.as_i32() => {
                (program.name, program.code)
            }
            Ok(Some(_) | None) => return Ok(false),
            Err(error) => {
                return Err(PersistenceStoreFailure::Transient(error));
            }
        };
        let (source_name, source_code) = source;
        let copy_name = format!("{source_name} (copy)");
        match self
            .insert_program(request.player.as_i32(), &copy_name, &source_code)
            .await
        {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn building_menu(
        &self,
        request: &crate::game::BuildingMenuRequest,
    ) -> Result<Vec<crate::db::BuildingRow>, PersistenceStoreFailure> {
        self.load_buildings_by_owner(request.player_id.as_i32())
            .await
            .map_err(PersistenceStoreFailure::Transient)
    }

    async fn auction_grid(
        &self,
        _request: &crate::game::AuctionGridRequest,
    ) -> Result<crate::game::AuctionGridResult, PersistenceStoreFailure> {
        self.order_counts_by_item()
            .await
            .map(|counts| crate::game::AuctionGridResult::Loaded { counts })
            .map_err(PersistenceStoreFailure::Transient)
    }

    async fn auction_item_orders(
        &self,
        request: &crate::game::AuctionItemOrdersRequest,
    ) -> Result<crate::game::AuctionItemOrdersResult, PersistenceStoreFailure> {
        self.list_orders_by_item(request.item_id)
            .await
            .map(|orders| crate::game::AuctionItemOrdersResult::Loaded { orders })
            .map_err(PersistenceStoreFailure::Transient)
    }

    async fn auction_order(
        &self,
        request: &crate::game::AuctionOrderRequest,
    ) -> Result<crate::game::AuctionOrderResult, PersistenceStoreFailure> {
        let Some(order) = self
            .get_order(request.order_id)
            .await
            .map_err(PersistenceStoreFailure::Transient)?
        else {
            return Ok(crate::game::AuctionOrderResult::NotFound);
        };
        let buyer_name = if order.buyer_id > 0 {
            let Some(player) = self
                .get_player_by_id(order.buyer_id)
                .await
                .map_err(PersistenceStoreFailure::Transient)?
            else {
                return Ok(crate::game::AuctionOrderResult::PermanentFailure {
                    message: "Данные ордера повреждены.".to_owned(),
                });
            };
            Some(player.name)
        } else {
            None
        };
        Ok(crate::game::AuctionOrderResult::Loaded { order, buyer_name })
    }

    async fn auction_order_create(
        &self,
        request: &crate::game::AuctionOrderCreateRequest,
    ) -> Result<crate::game::AuctionOrderCreateResult, PersistenceStoreFailure> {
        self.create_order(
            request.player_id.into(),
            request.item_id,
            request.num,
            request.cost,
        )
        .await
        .map(|_| crate::game::AuctionOrderCreateResult::Created)
        .map_err(PersistenceStoreFailure::Transient)
    }

    async fn auction_bet(
        &self,
        request: &crate::game::AuctionBetRequest,
    ) -> Result<crate::game::AuctionBetResult, PersistenceStoreFailure> {
        let Some(order) = self
            .get_order(request.order_id)
            .await
            .map_err(PersistenceStoreFailure::Transient)?
        else {
            return Ok(crate::game::AuctionBetResult::NotFound);
        };
        let previous_buyer_id = order.buyer_id;
        let previous_cost = order.cost;
        let amount = request.requested_amount.unwrap_or_else(|| {
            crate::game::logic::auction_gui::min_bid(order.cost, order.buyer_id > 0)
        });
        let required = crate::game::logic::auction_gui::min_bid(order.cost, order.buyer_id > 0);
        if required > amount || request.bidder_money < amount {
            return Ok(crate::game::AuctionBetResult::Rejected);
        }
        let buyer_name = self
            .get_player_by_id(request.player_id.into())
            .await
            .map_err(PersistenceStoreFailure::Transient)?
            .map(|player| player.name);
        let bet_time = crate::tasks::auction::now_unix();
        let won = self
            .try_update_order_bet_cas(
                request.order_id,
                amount,
                request.player_id.into(),
                bet_time,
                order.buyer_id,
                order.cost,
            )
            .await
            .map_err(PersistenceStoreFailure::Transient)?;
        if won == 0 {
            return Ok(crate::game::AuctionBetResult::LostRace);
        }
        if order.buyer_id != 0
            && let Err(error) = self.add_player_money(order.buyer_id, order.cost).await
        {
            let rollback = self
                .try_update_order_bet_cas(
                    request.order_id,
                    order.cost,
                    order.buyer_id,
                    order.bet_time,
                    request.player_id.into(),
                    amount,
                )
                .await;
            if let Err(rollback_error) = rollback {
                tracing::error!(
                    order_id = request.order_id,
                    error = ?rollback_error,
                    "Auction bet order rollback failed after buyer refund failure"
                );
            }
            return Err(PersistenceStoreFailure::Permanent(error));
        }
        let mut updated_order = order;
        updated_order.cost = amount;
        updated_order.buyer_id = request.player_id.into();
        updated_order.bet_time = bet_time;
        Ok(crate::game::AuctionBetResult::Won {
            amount,
            previous_buyer_id,
            previous_cost,
            order: updated_order,
            buyer_name,
        })
    }
}
