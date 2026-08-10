//! Definitions of all `PlayerCommand`, `GameEvent`, and `SaveCommand` contracts.
//! These enums decouple the network session tasks, the ECS game loop thread,
//! and the asynchronous database persistence writer task.

use bytes::Bytes;
use openmines_storage::buildings::BuildingExtra;
use openmines_storage::players::{PlayerRow, Role};
use std::time::Instant;

use crate::game::actors::player::PlayerId;
use crate::game::structures::buildings::{PackType, PackView};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionId(u64);

impl SessionId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for SessionId {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CommandSeq(u64);

impl CommandSeq {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SimTick(u64);

impl SimTick {
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct InventoryBuildingPlacement {
    pub selected_item: i32,
    pub type_code: String,
    pub pack_type: PackType,
    pub x: i32,
    pub y: i32,
    pub owner_id: PlayerId,
    pub clan_id: i32,
    pub extra: BuildingExtra,
}

#[derive(Debug, Clone)]
pub struct PaidBuildingPlacement {
    pub type_code: String,
    pub pack_type: PackType,
    pub x: i32,
    pub y: i32,
    pub owner_id: PlayerId,
    pub owner_clan_id: i32,
    pub building_clan_id: i32,
    pub cost: i64,
    pub extra: BuildingExtra,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BuildingDeleteOperationId(u64);

impl BuildingDeleteOperationId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

impl From<CommandSeq> for BuildingDeleteOperationId {
    fn from(sequence: CommandSeq) -> Self {
        Self::new(sequence.get())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuildingDeleteOrigin {
    pub session_id: SessionId,
    pub player_id: PlayerId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildingDeleteCause {
    PlayerRequest(BuildingDeleteOrigin),
    Damage { trigger_player_id: Option<PlayerId> },
}

impl BuildingDeleteCause {
    pub const fn origin(self) -> Option<BuildingDeleteOrigin> {
        match self {
            Self::PlayerRequest(origin) => Some(origin),
            Self::Damage { .. } => None,
        }
    }

    pub const fn trigger_player_id(self) -> Option<PlayerId> {
        match self {
            Self::PlayerRequest(origin) => Some(origin.player_id),
            Self::Damage {
                trigger_player_id, ..
            } => trigger_player_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemovePack {
    pub x: i32,
    pub y: i32,
    pub cause: BuildingDeleteCause,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuildingIdentity {
    pub building_id: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct BuildingDeleteRequest {
    pub operation_id: BuildingDeleteOperationId,
    pub expected: BuildingIdentity,
    pub view: PackView,
    pub cause: BuildingDeleteCause,
    pub box_write: Option<openmines_storage::BoxWrite>,
    pub inventory_drop_item: Option<i32>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GuiButtonKind {
    Close,
    Building,
    Pack,
    Storage,
    Craft,
    Teleport,
    Respawn,
    Gun,
    Market,
    Settings,
    Programmer,
    Clan,
    Auction,
    Up,
    Other,
}

impl GuiButtonKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Close => "gui.close",
            Self::Building => "gui.building",
            Self::Pack => "gui.pack",
            Self::Storage => "gui.storage",
            Self::Craft => "gui.craft",
            Self::Teleport => "gui.teleport",
            Self::Respawn => "gui.respawn",
            Self::Gun => "gui.gun",
            Self::Market => "gui.market",
            Self::Settings => "gui.settings",
            Self::Programmer => "gui.programmer",
            Self::Clan => "gui.clan",
            Self::Auction => "gui.auction",
            Self::Up => "gui.up",
            Self::Other => "gui.other",
        }
    }
}

#[derive(Debug, Clone)]
pub enum GuiCommand {
    OpenPack { x: i32, y: i32 },
    Button { kind: GuiButtonKind, raw: String },
}

impl GuiCommand {
    pub fn parse(button: String) -> Self {
        if let Some(rest) = button.strip_prefix("pack_op:open:") {
            let mut parts = rest.split(':');
            if let (Some(x), Some(y), None) = (parts.next(), parts.next(), parts.next())
                && let (Ok(x), Ok(y)) = (x.parse::<i32>(), y.parse::<i32>())
            {
                return Self::OpenPack { x, y };
            }
        }

        let kind = classify_gui_button(&button);
        Self::Button { kind, raw: button }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::OpenPack { .. } => "gui.pack.open",
            Self::Button { kind, .. } => kind.label(),
        }
    }
}

fn classify_gui_button(button: &str) -> GuiButtonKind {
    if matches!(button, "exit" | "exit:0" | "close") {
        GuiButtonKind::Close
    } else if button.starts_with("bld_place:") || button == "open_buildings" {
        GuiButtonKind::Building
    } else if button.starts_with("pack_op:") || button.starts_with("pack_save:") {
        GuiButtonKind::Pack
    } else if button.starts_with("transfer:") {
        GuiButtonKind::Storage
    } else if button.starts_with("craft_") {
        GuiButtonKind::Craft
    } else if button.starts_with("tp:") {
        GuiButtonKind::Teleport
    } else if button.starts_with("resp_") {
        GuiButtonKind::Respawn
    } else if button.starts_with("gun_") {
        GuiButtonKind::Gun
    } else if matches!(button, "sellcrys" | "buycrys" | "sellall" | "getprofit")
        || button.starts_with("sell:")
        || button.starts_with("buy:")
    {
        GuiButtonKind::Market
    } else if button.starts_with("save:") {
        GuiButtonKind::Settings
    } else if button == "prog"
        || button == "createprog"
        || button.starts_with("openprog:")
        || button.starts_with("createprog:")
        || button.starts_with("rename:")
    {
        GuiButtonKind::Programmer
    } else if button.starts_with("clan") {
        GuiButtonKind::Clan
    } else if button == "auc"
        || button.starts_with("choose:")
        || button.starts_with("openorder:")
        || button.starts_with("auc")
    {
        GuiButtonKind::Auction
    } else if button == "upgrade"
        || button == "buyslot"
        || button.starts_with("skill:")
        || button.starts_with("delete:")
        || button.starts_with("install:")
    {
        GuiButtonKind::Up
    } else {
        GuiButtonKind::Other
    }
}

#[derive(Debug, Clone)]
pub struct TeleportGuiView {
    pub source: crate::game::WorldPos,
    pub charge: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub destinations: Vec<crate::game::WorldPos>,
    pub map_tiles: Vec<Option<bool>>,
}

#[derive(Debug, Clone)]
pub struct SpotGuiView {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct StorageGuiView {
    pub x: i32,
    pub y: i32,
    pub crystal_lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum GuiView {
    Close,
    Teleport(TeleportGuiView),
    Spot(SpotGuiView),
    Storage(StorageGuiView),
}

#[derive(Debug, Clone)]
pub enum GameCommand {
    Player(PlayerCommand),
}

impl GameCommand {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Player(pc) => pc.name(),
        }
    }

    #[must_use]
    pub fn persistence_kind(&self) -> Option<SaveKind> {
        match self {
            Self::Player(pc) => pc.persistence_kind(),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PlayerCommand {
    /// Initial connection handshake and registration.
    Connect {
        row: Box<openmines_storage::players::PlayerRow>,
    },
    /// Clean disconnect of the player session.
    Disconnect,
    /// Player movement request.
    Move {
        time: u32,
        x: i32,
        y: i32,
        direction: i32,
        programmatic: bool,
    },
    /// Cell digging action request.
    Dig { direction: i32, programmatic: bool },
    /// Construction or block placement request.
    Build {
        direction: i32,
        block_type: String,
        programmatic: bool,
    },
    /// Geology scan action request.
    Geology { programmatic: bool },
    /// Health self-healing request.
    Heal { programmatic: bool },
    /// Parsed GUI action from one concrete authenticated session.
    Gui { command: GuiCommand },
    /// Local area chat message.
    LocalChat { message: String },
    /// Parsed slash command from local or channel chat.
    Slash { command: SlashCommand },
    /// Global channel chat message.
    ChannelChat { payload: Bytes },
    /// Request to resynchronize chat history.
    ChatResync { payload: Bytes },
    /// Chat navigation/channel menu interaction.
    ChatMenu { payload: Bytes },
    /// Join or select chat channel.
    ChatChoose { payload: Bytes },
    /// Update individual chat settings.
    ChatSettings { payload: Bytes },
    /// Send a private chat message to a user.
    ChatPrivate { payload: Bytes },
    /// Request nicknames for a list of player IDs.
    Whois { ids: Vec<i32> },
    /// Toggle automatic digging status.
    ToggleAutoDig,
    /// Toggle player aggression status.
    ToggleAggression,
    /// Select item index in player inventory.
    InventoryChoose { payload: Bytes },
    /// Use currently selected inventory item.
    InventoryUse,
    /// Toggle inventory GUI visibility.
    InventoryToggle,
    /// Open a nearby box / chest.
    OpenBox,
    /// Claim daily connection reward.
    ClaimBonus,
    /// Save client settings payload.
    SettingsSave { payload: Bytes },
    /// Trigger admin command GUI or panels.
    AdminAction,
    /// Respawn player after death.
    Respawn,
    /// Open the programmer program editing GUI.
    OpenProgrammer,
    /// Request list of building structures owned by the player.
    RequestMyBuildings,
    /// Open clan management GUI.
    OpenClan,
    /// Programmator program lifecycle action (save, delete, restart, rename, copy).
    ProgramAction { event: String, payload: Bytes },
    /// Clear deleted programmer runtime state after DB ownership/delete succeeded.
    ApplyDeletedProgram { program_id: i32 },
    /// Commit an inventory building placement after DB insert succeeded.
    ApplyInventoryBuildingPlaced {
        placement: InventoryBuildingPlacement,
        db_id: i32,
    },
    /// Commit a paid GUI building placement after DB insert succeeded.
    ApplyPaidBuildingPlaced {
        placement: PaidBuildingPlacement,
        db_id: i32,
    },
    /// Refund money for a paid GUI building placement after DB insert failed.
    RefundPaidBuildingPlacement { cost: i64 },
    /// Authoritative request to delete one building through persistence admission.
    RemovePack { remove: RemovePack },
    /// Apply a GUI program open/create after DB ownership/selection succeeded.
    ApplyProgramEditorOpen {
        program_id: i32,
        program_name: String,
        source: String,
    },
    /// Apply a GUI program rename after DB rename succeeded.
    ApplyProgramEditorRename {
        program_id: i32,
        program_name: String,
        source: String,
    },
    /// Sell crystals at market.
    MarketSell {
        sliders: [i64; 6],
        building_x: i32,
        building_y: i32,
    },
    /// Sell all crystals at market.
    MarketSellAll { building_x: i32, building_y: i32 },
    /// Buy crystals at market.
    MarketBuy {
        sliders: [i64; 6],
        building_x: i32,
        building_y: i32,
    },
    /// Withdraw profit from market building.
    MarketGetProfit { building_x: i32, building_y: i32 },
    /// Known TY event that does not mutate gameplay state.
    KnownNoopTy { event: String, payload: Bytes },
}

#[derive(Debug, Clone)]
pub enum SlashCommand {
    Give {
        item_id: i32,
        amount: i32,
    },
    GiveAll,
    Money {
        amount: i64,
    },
    MoneyAll {
        amount: i64,
    },
    Skill {
        target: String,
        code: String,
        level: i32,
        slot: Option<i32>,
        exp: f32,
    },
    SkillHelp,
    Teleport {
        x: i32,
        y: i32,
    },
    Heal,
    Kick {
        target: String,
    },
    Role {
        target: String,
        role: Role,
    },
    Clan {
        action: ClanAction,
    },
    Pack {
        action: SlashPackCommand,
    },
    Help,
    Unknown {
        command: String,
    },
    Invalid {
        title: String,
        message: String,
    },
}

impl SlashCommand {
    #[must_use]
    pub const fn persistence_kind(&self) -> Option<SaveKind> {
        match self {
            Self::MoneyAll { .. } => Some(SaveKind::AdminMoneyAll),
            Self::Role { .. } => Some(SaveKind::AdminRole),
            Self::Skill { .. } => Some(SaveKind::AdminSkill),
            Self::Clan { .. } => Some(SaveKind::ClanCommand),
            Self::Give { .. }
            | Self::GiveAll
            | Self::Money { .. }
            | Self::SkillHelp
            | Self::Teleport { .. }
            | Self::Heal
            | Self::Kick { .. }
            | Self::Pack { .. }
            | Self::Help
            | Self::Unknown { .. }
            | Self::Invalid { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ClanAction {
    Create { name: String, tag: String },
    Leave,
    Kick { target: String },
    AcceptInvite { clan_id: i32 },
    DeclineInvite { clan_id: i32 },
    AcceptRequest { target_id: PlayerId },
    DeclineRequest { target_id: PlayerId },
    Promote { target_id: PlayerId },
    KickById { target_id: PlayerId },
    Invite { target_id: PlayerId },
    Request { clan_id: i32 },
    Invalid { message: String },
}

#[derive(Debug, Clone)]
pub enum SlashPackCommand {
    Owner {
        x: i32,
        y: i32,
        owner_id: i32,
    },
    Clan {
        x: i32,
        y: i32,
        clan_id: i32,
    },
    Move {
        x: i32,
        y: i32,
        to_x: i32,
        to_y: i32,
    },
    Type {
        x: i32,
        y: i32,
        pack_type: PackType,
    },
    Invalid {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandIngressClass {
    Lifecycle,
    Gameplay,
    Internal,
}

impl CommandIngressClass {
    #[must_use]
    pub const fn metric_name(self) -> &'static str {
        match self {
            Self::Lifecycle => "lifecycle",
            Self::Gameplay => "gameplay",
            Self::Internal => "internal",
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Lifecycle => 0,
            Self::Gameplay => 1,
            Self::Internal => 2,
        }
    }
}

impl PlayerCommand {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Connect { .. } => "connect",
            Self::Disconnect { .. } => "disconnect",
            Self::Move { .. } => "move",
            Self::Dig { .. } => "dig",
            Self::Build { .. } => "build",
            Self::Geology { .. } => "geology",
            Self::Heal { .. } => "heal",
            Self::Gui { command, .. } => command.label(),
            Self::LocalChat { .. } => "local_chat",
            Self::Slash { .. } => "slash",
            Self::ChannelChat { .. } => "channel_chat",
            Self::ChatResync { .. } => "chat_resync",
            Self::ChatMenu { .. } => "chat_menu",
            Self::ChatChoose { .. } => "chat_choose",
            Self::ChatSettings { .. } => "chat_settings",
            Self::ChatPrivate { .. } => "chat_private",
            Self::Whois { .. } => "whois",
            Self::ToggleAutoDig { .. } => "toggle_auto_dig",
            Self::ToggleAggression { .. } => "toggle_aggression",
            Self::InventoryChoose { .. } => "inventory_choose",
            Self::InventoryUse { .. } => "inventory_use",
            Self::InventoryToggle { .. } => "inventory_toggle",
            Self::OpenBox { .. } => "open_box",
            Self::ClaimBonus { .. } => "claim_bonus",
            Self::SettingsSave { .. } => "settings_save",
            Self::AdminAction { .. } => "admin_action",
            Self::Respawn { .. } => "respawn",
            Self::OpenProgrammer { .. } => "open_programmer",
            Self::RequestMyBuildings { .. } => "request_my_buildings",
            Self::OpenClan { .. } => "open_clan",
            Self::ProgramAction { .. } => "program_action",
            Self::ApplyDeletedProgram { .. } => "apply_deleted_program",
            Self::ApplyInventoryBuildingPlaced { .. } => "apply_inventory_building_placed",
            Self::ApplyPaidBuildingPlaced { .. } => "apply_paid_building_placed",
            Self::RefundPaidBuildingPlacement { .. } => "refund_paid_building_placement",
            Self::RemovePack { .. } => "remove_pack",
            Self::ApplyProgramEditorOpen { .. } => "apply_program_editor_open",
            Self::ApplyProgramEditorRename { .. } => "apply_program_editor_rename",
            Self::MarketSell { .. } => "market_sell",
            Self::MarketSellAll { .. } => "market_sell_all",
            Self::MarketBuy { .. } => "market_buy",
            Self::MarketGetProfit { .. } => "market_get_profit",
            Self::KnownNoopTy { .. } => "known_noop_ty",
        }
    }

    pub fn persistence_kind(&self) -> Option<SaveKind> {
        match self {
            Self::Disconnect { .. } | Self::ClaimBonus { .. } => Some(SaveKind::Player),
            Self::RemovePack { .. } => Some(SaveKind::BuildingDelete),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw.starts_with("createprog:") => Some(SaveKind::ProgramCreate),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw == "prog" => Some(SaveKind::ProgramMenu),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw.starts_with("openprog:") => Some(SaveKind::ProgramOpen),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw.starts_with("rename:") => Some(SaveKind::ProgramRename),
            Self::ProgramAction { event, payload } if event == "PROG" => {
                crate::game::programmator::ProgrammatorState::decode_prog_packet(payload).map(
                    |(program_id, _)| {
                        if program_id <= 0 {
                            SaveKind::ProgramMenu
                        } else {
                            SaveKind::Program
                        }
                    },
                )
            }
            Self::ProgramAction { event, .. } if event == "PCOP" => Some(SaveKind::ProgramCopy),
            Self::ProgramAction { event, payload } if event == "PDEL" => {
                std::str::from_utf8(payload)
                    .ok()
                    .and_then(|raw| raw.trim().parse::<i32>().ok())
                    .filter(|program_id| *program_id > 0)
                    .map(|_| SaveKind::ProgramDelete)
            }
            Self::OpenProgrammer => Some(SaveKind::ProgramMenu),
            Self::RequestMyBuildings => Some(SaveKind::BuildingMenu),
            Self::OpenClan
            // `OpenPack` is resolved against the live world during apply. Reserve the
            // clan-menu slot conservatively so a clans pack can never create durable
            // work after admission. Non-clan packs release this unused permit.
            | Self::Gui {
                command: GuiCommand::OpenPack { .. },
            } => Some(SaveKind::ClanMenu),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if matches!(
                raw.as_str(),
                "clan_menu"
                    | "clan_back"
                    | "clan_members"
                    | "clan_invite_list"
                    | "clan_invites_view"
                    | "clan_requests"
            ) || raw.starts_with("clan_view:") =>
            {
                Some(SaveKind::ClanMenu)
            }
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw == "auc" => Some(SaveKind::AuctionGrid),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw.starts_with("choose:") => Some(SaveKind::AuctionItemOrders),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw.starts_with("openorder:") => Some(SaveKind::AuctionOrder),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw.starts_with("aucsetnum:") => Some(SaveKind::AuctionOrderCreate),
            Self::Gui {
                command: GuiCommand::Button { raw, .. },
            } if raw.starts_with("aucminbet:") || raw.starts_with("aucbet:") => {
                Some(SaveKind::AuctionBet)
            }
            Self::ChatSettings { .. } => Some(SaveKind::ChatColorCycle),
            Self::ChatResync { .. } | Self::ChatChoose { .. } => Some(SaveKind::ChatResync),
            Self::ChatMenu { .. } => Some(SaveKind::ChatMenu),
            Self::ChatPrivate { .. } => Some(SaveKind::ChatPrivate),
            Self::Whois { .. } => Some(SaveKind::Whois),
            Self::Slash { command } => command.persistence_kind(),
            Self::LocalChat { message } => {
                crate::game::logic::commands_social::parse_slash_command(message.trim())
                    .persistence_kind()
            }
            Self::ChannelChat { payload } => {
                crate::game::logic::commands_social::parse_slash_command(
                    &crate::game::logic::chat::extract_channel_message_text(payload),
                )
                .persistence_kind()
            }
            _ => None,
        }
    }

    #[must_use]
    pub const fn ingress_class(&self) -> CommandIngressClass {
        match self {
            Self::Connect { .. } | Self::Disconnect { .. } => CommandIngressClass::Lifecycle,
            Self::ApplyDeletedProgram { .. }
            | Self::ApplyInventoryBuildingPlaced { .. }
            | Self::ApplyPaidBuildingPlaced { .. }
            | Self::RefundPaidBuildingPlacement { .. }
            | Self::ApplyProgramEditorOpen { .. }
            | Self::ApplyProgramEditorRename { .. } => CommandIngressClass::Internal,
            _ => CommandIngressClass::Gameplay,
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueuedGameCommand {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub ingress_class: Option<CommandIngressClass>,
    pub sequence: CommandSeq,
    pub received_at: Instant,
    pub enqueued_at: Instant,
    pub command: GameCommand,
}

#[derive(Debug, Default)]
pub struct CommandEffects {
    pub events: Vec<GameEvent>,
    pub saves: Vec<SaveCommand>,
    pub broadcasts: Vec<crate::game::BroadcastEffect>,
}

impl CommandEffects {
    pub fn append(&mut self, mut other: Self) {
        self.events.append(&mut other.events);
        self.saves.append(&mut other.saves);
        self.broadcasts.append(&mut other.broadcasts);
    }
}

/// Data snapshot for presentation-owned Player.Init encoding, extracted during simulation tick.
#[derive(Debug, Clone)]
pub struct PlayerInitView {
    pub player: Box<PlayerRow>,
    pub geo_label: String,
    pub max_health: i32,
    pub skills: crate::game::actors::player::PlayerSkillsComp,
    pub inventory: crate::game::player::PlayerInventory,
    pub chunk_x: u32,
    pub chunk_y: u32,
    pub dir: u8,
    pub skin: u8,
    pub clan_id_u16: u16,
    pub chat_tag: String,
    pub chat_name: String,
    pub chat_history: Vec<openmines_protocol::chat::ChatMessage>,
    pub prog_running: bool,
    pub hand_mode_active: bool,
    pub initial_visible_chunks: Vec<(u32, u32)>,
}

/// Outbound work produced by authoritative command application.
#[derive(Debug, Clone)]
pub enum GameEvent {
    /// Hydrated login view for presentation-owned Player.Init encoding.
    PlayerInit {
        session_id: SessionId,
        view: Box<PlayerInitView>,
    },
    SessionBatch {
        session_id: SessionId,
        player_id: PlayerId,
        packets: Vec<Vec<u8>>,
    },
    RefreshChunks {
        session_id: SessionId,
        player_id: PlayerId,
    },
    Fanout {
        recipients: Vec<SessionId>,
        data: Vec<u8>,
    },
    MovementFanout {
        player_id: PlayerId,
        recipients: Vec<SessionId>,
        data: Vec<u8>,
    },
    ChatFanout {
        route: crate::game::logic::chat::ChannelChatRoute,
        message: openmines_protocol::chat::ChatMessage,
    },
    /// Ordered world updates. Presentation owns encoding and session wakeups,
    /// while this sequence retains the legacy barriers between effect kinds.
    WorldEffects {
        effects: Vec<crate::game::BroadcastEffect>,
    },
    GuiView {
        session_id: SessionId,
        player_id: PlayerId,
        view: GuiView,
    },
}

impl GameEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::PlayerInit { .. } => "player_init",
            Self::SessionBatch { .. } => "session_batch",
            Self::RefreshChunks { .. } => "refresh_chunks",
            Self::Fanout { .. } => "fanout",
            Self::MovementFanout { .. } => "movement_fanout",
            Self::ChatFanout { .. } => "chat_fanout",
            Self::WorldEffects { .. } => "world_effects",
            Self::GuiView { .. } => "gui_view",
        }
    }
}

/// All database write transactions sent from the game thread to the persistence worker.
#[derive(Debug, Clone)]
pub enum SaveCommand {
    Player {
        row: Box<PlayerRow>,
    },
    Building {
        row: Box<openmines_storage::buildings::BuildingRow>,
    },
    Box {
        write: openmines_storage::BoxWrite,
    },
    ProgramCreate {
        request: ProgramCreateRequest,
    },
    Program {
        request: ProgramSaveRequest,
    },
    ProgramMenu {
        request: ProgramMenuRequest,
    },
    ProgramOpen {
        request: ProgramOpenRequest,
    },
    ProgramRename {
        request: ProgramRenameRequest,
    },
    ProgramDelete {
        request: ProgramDeleteRequest,
    },
    #[allow(dead_code)]
    ProgramCopy {
        request: ProgramCopyRequest,
    },
    BuildingMenu {
        request: BuildingMenuRequest,
    },
    AuctionGrid {
        request: AuctionGridRequest,
    },
    AuctionItemOrders {
        request: AuctionItemOrdersRequest,
    },
    AuctionOrder {
        request: AuctionOrderRequest,
    },
    AuctionOrderCreate {
        request: AuctionOrderCreateRequest,
    },
    AuctionBet {
        request: AuctionBetRequest,
    },
    BuildingDelete {
        request: BuildingDeleteRequest,
    },
    ChatAppend {
        request: ChatAppendRequest,
    },
    ChatColorCycle {
        request: ChatColorCycleRequest,
    },
    ChatResync {
        request: ChatResyncRequest,
    },
    ChatMenu {
        request: ChatMenuRequest,
    },
    ChatPrivate {
        request: ChatPrivateRequest,
    },
    Whois {
        request: WhoisRequest,
    },
    ClanMenu {
        request: ClanMenuRequest,
    },
    AdminMoneyAll {
        request: AdminMoneyAllRequest,
    },
    AdminRole {
        request: AdminRoleRequest,
    },
    AdminSkill {
        request: AdminSkillRequest,
    },
    ClanCommand {
        request: ClanCommandRequest,
    },
}

#[derive(Debug, Clone)]
pub struct AdminMoneyAllRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub amount: i64,
}

#[derive(Debug, Clone)]
pub struct AdminRoleRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub target_name: String,
    pub role: Role,
}

#[derive(Debug, Clone)]
pub struct AdminSkillRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub target_id: PlayerId,
    pub target_name: String,
    pub target_session_id: Option<SessionId>,
    pub skill_code: String,
    pub level: i32,
    pub slot: i32,
    pub exp: f32,
    pub packets: Vec<Vec<u8>>,
    pub row: Box<PlayerRow>,
}

#[derive(Debug, Clone)]
pub struct ClanCommandRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub action: ClanAction,
    pub create_reserved: bool,
}

#[derive(Debug, Clone)]
pub struct ChatAppendRequest {
    pub id: i64,
    pub tag: String,
    pub nickname: String,
    pub text: String,
    pub player_id: i32,
    pub color: i32,
}

#[derive(Debug, Clone)]
pub struct ChatColorCycleRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
}

#[derive(Debug, Clone)]
pub struct ChatResyncRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub channel_tag: String,
    pub last_id: i64,
}

#[derive(Debug, Clone)]
pub struct ChatMenuRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
}

#[derive(Debug, Clone)]
pub struct ChatPrivateRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub target_uid: PlayerId,
}

#[derive(Debug, Clone)]
pub struct WhoisRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    /// Original wire order, including repeated and non-existent IDs.
    pub ids: Vec<i32>,
    /// Names extracted from the authoritative online ECS snapshot before dispatch.
    pub online_names: Vec<(i32, String)>,
}

#[derive(Debug, Clone)]
pub struct ClanMenuRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub player_clan_id: Option<i32>,
    pub action: ClanMenuAction,
    /// Eligible online invite targets extracted from ECS before the DB read.
    pub invite_candidates: Vec<(i32, String)>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClanMenuAction {
    Main,
    Preview { clan_id: i32 },
    Members,
    InviteList,
    Invites,
    Requests,
}

impl SaveCommand {
    pub const fn kind(&self) -> SaveKind {
        match self {
            Self::Player { .. } => SaveKind::Player,
            Self::Building { .. } => SaveKind::Building,
            Self::Box { .. } => SaveKind::Box,
            Self::ProgramCreate { .. } => SaveKind::ProgramCreate,
            Self::Program { .. } => SaveKind::Program,
            Self::ProgramMenu { .. } => SaveKind::ProgramMenu,
            Self::ProgramOpen { .. } => SaveKind::ProgramOpen,
            Self::ProgramRename { .. } => SaveKind::ProgramRename,
            Self::ProgramDelete { .. } => SaveKind::ProgramDelete,
            Self::ProgramCopy { .. } => SaveKind::ProgramCopy,
            Self::BuildingMenu { .. } => SaveKind::BuildingMenu,
            Self::AuctionGrid { .. } => SaveKind::AuctionGrid,
            Self::AuctionItemOrders { .. } => SaveKind::AuctionItemOrders,
            Self::AuctionOrder { .. } => SaveKind::AuctionOrder,
            Self::AuctionOrderCreate { .. } => SaveKind::AuctionOrderCreate,
            Self::AuctionBet { .. } => SaveKind::AuctionBet,
            Self::BuildingDelete { .. } => SaveKind::BuildingDelete,
            Self::ChatAppend { .. } => SaveKind::ChatAppend,
            Self::ChatColorCycle { .. } => SaveKind::ChatColorCycle,
            Self::ChatResync { .. } => SaveKind::ChatResync,
            Self::ChatMenu { .. } => SaveKind::ChatMenu,
            Self::ChatPrivate { .. } => SaveKind::ChatPrivate,
            Self::Whois { .. } => SaveKind::Whois,
            Self::ClanMenu { .. } => SaveKind::ClanMenu,
            Self::AdminMoneyAll { .. } => SaveKind::AdminMoneyAll,
            Self::AdminRole { .. } => SaveKind::AdminRole,
            Self::AdminSkill { .. } => SaveKind::AdminSkill,
            Self::ClanCommand { .. } => SaveKind::ClanCommand,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgramCreateRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ProgramSaveRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub program_id: i32,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ProgramMenuRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
}

#[derive(Debug, Clone)]
pub struct ProgramOpenRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub program: i32,
}

#[derive(Debug, Clone)]
pub struct ProgramRenameRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub program_id: i32,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ProgramDeleteRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub program_id: i32,
    pub clear_selected: bool,
}

#[derive(Debug, Clone)]
pub struct ProgramCopyRequest {
    pub player: PlayerId,
    pub session: SessionId,
    pub program: i32,
}

#[derive(Debug, Clone)]
pub struct BuildingMenuRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
}

#[derive(Debug, Clone)]
pub struct AuctionGridRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub building_x: i32,
    pub building_y: i32,
}

#[derive(Debug, Clone)]
pub struct AuctionItemOrdersRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub building_x: i32,
    pub building_y: i32,
    pub item_id: i32,
}

#[derive(Debug, Clone)]
pub struct AuctionOrderRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub building_x: i32,
    pub building_y: i32,
    pub order_id: i32,
}

#[derive(Debug, Clone)]
pub struct AuctionOrderCreateRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub building_x: i32,
    pub building_y: i32,
    pub item_id: i32,
    pub num: i32,
    pub cost: i64,
}

#[derive(Debug, Clone)]
pub struct AuctionBetRequest {
    pub player_id: PlayerId,
    pub session_id: SessionId,
    pub building_x: i32,
    pub building_y: i32,
    pub order_id: i32,
    pub requested_amount: Option<i64>,
    pub bidder_money: i64,
}

#[derive(Debug)]
pub enum PersistenceCompletion {
    ProgramCreated {
        request: ProgramCreateRequest,
        result: ProgramCreateResult,
    },
    ProgramSaved {
        request: ProgramSaveRequest,
        result: ProgramSaveResult,
    },
    ProgramMenuLoaded {
        request: ProgramMenuRequest,
        result: ProgramMenuResult,
    },
    ProgramOpened {
        request: ProgramOpenRequest,
        result: ProgramOpenResult,
    },
    ProgramRenamed {
        request: ProgramRenameRequest,
        result: ProgramRenameResult,
    },
    ProgramDeleted {
        request: ProgramDeleteRequest,
        result: ProgramDeleteResult,
    },
    ProgramCopied {
        request: ProgramCopyRequest,
        result: ProgramCopyResult,
    },
    BuildingMenuLoaded {
        request: BuildingMenuRequest,
        result: BuildingMenuResult,
    },
    AuctionGridLoaded {
        request: AuctionGridRequest,
        result: AuctionGridResult,
    },
    AuctionItemOrdersLoaded {
        request: AuctionItemOrdersRequest,
        result: AuctionItemOrdersResult,
    },
    AuctionOrderLoaded {
        request: AuctionOrderRequest,
        result: AuctionOrderResult,
    },
    AuctionOrderCreated {
        request: AuctionOrderCreateRequest,
        result: AuctionOrderCreateResult,
    },
    AuctionBetCompleted {
        request: AuctionBetRequest,
        result: AuctionBetResult,
    },
    BuildingDeleted {
        request: BuildingDeleteRequest,
        result: BuildingDeleteResult,
    },
    ChatColorCycled {
        request: ChatColorCycleRequest,
        result: ChatColorCycleResult,
    },
    ChatResynced {
        request: ChatResyncRequest,
        result: ChatResyncResult,
    },
    ChatMenuLoaded {
        request: ChatMenuRequest,
        result: ChatMenuResult,
    },
    ChatPrivateOpened {
        request: ChatPrivateRequest,
        result: ChatPrivateResult,
    },
    WhoisLoaded {
        request: WhoisRequest,
        result: WhoisResult,
    },
    ClanMenuLoaded {
        request: ClanMenuRequest,
        result: ClanMenuResult,
    },
    AdminMoneyAllApplied {
        request: AdminMoneyAllRequest,
        result: AdminMoneyAllResult,
    },
    AdminRoleApplied {
        request: AdminRoleRequest,
        result: AdminRoleResult,
    },
    AdminSkillApplied {
        request: AdminSkillRequest,
        result: AdminSkillResult,
    },
    ClanCommandApplied {
        request: ClanCommandRequest,
        result: ClanCommandResult,
    },
}

#[derive(Debug)]
pub enum ProgramCreateResult {
    Created { program_id: i32 },
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum ProgramSaveResult {
    Saved { program_name: String },
    Rejected,
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum ProgramMenuResult {
    Loaded {
        programs: Vec<crate::db::ProgramRow>,
    },
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum ProgramOpenResult {
    Opened { program: crate::db::ProgramRow },
    Rejected,
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum ProgramRenameResult {
    Renamed { program: crate::db::ProgramRow },
    Rejected,
    PermanentFailure { message: String },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ProgramDeleteResult {
    Deleted,
    Rejected,
    PermanentFailure,
}

#[derive(Debug)]
pub enum ProgramCopyResult {
    Copied,
    Rejected,
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum BuildingMenuResult {
    Loaded {
        buildings: Vec<crate::db::buildings::BuildingRow>,
    },
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum AuctionGridResult {
    Loaded { counts: Vec<(i32, i64, i64)> },
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum AuctionItemOrdersResult {
    Loaded {
        orders: Vec<crate::db::orders::OrderRow>,
    },
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum AuctionOrderResult {
    Loaded {
        order: crate::db::orders::OrderRow,
        buyer_name: Option<String>,
    },
    NotFound,
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum AuctionOrderCreateResult {
    Created,
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum AuctionBetResult {
    Won {
        amount: i64,
        previous_buyer_id: i32,
        previous_cost: i64,
        order: crate::db::orders::OrderRow,
        buyer_name: Option<String>,
    },
    LostRace,
    Rejected,
    NotFound,
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum ChatColorCycleResult {
    Cycled { color: i32 },
    Rejected,
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum ChatResyncResult {
    Success {
        channel_name: String,
        messages: Vec<openmines_protocol::chat::ChatMessage>,
    },
    #[allow(dead_code)]
    AccessDenied,
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum ChatMenuResult {
    Success {
        channels: Vec<(String, bool, String, String)>,
    },
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum ChatPrivateResult {
    Success {
        target_name: String,
        channel_tag: String,
        messages: Vec<openmines_protocol::chat::ChatMessage>,
    },
    TargetNotFound,
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum WhoisResult {
    Loaded { names: Vec<(i32, String)> },
    PermanentFailure { message: String },
}

#[derive(Debug, Clone)]
pub struct ClanMenuListEntry {
    pub id: i32,
    pub name: String,
    pub abr: String,
    pub member_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClanMemberEntry {
    pub player_id: i32,
    pub name: String,
    pub rank: i32,
}

#[derive(Debug)]
pub enum ClanMenuResult {
    Browse {
        invites: Vec<(i32, String)>,
        clans: Vec<ClanMenuListEntry>,
    },
    #[allow(dead_code)]
    Info {
        clan: ClanMenuListEntry,
        owner_name: String,
        player_rank: i32,
        request_count: Option<usize>,
        can_leave: bool,
    },
    Preview {
        clan: ClanMenuListEntry,
        owner_name: String,
        can_request_join: bool,
    },
    Members {
        members: Vec<ClanMemberEntry>,
        player_rank: i32,
    },
    InviteList {
        allowed: bool,
        candidates: Vec<(i32, String)>,
    },
    Invites {
        invites: Vec<(i32, String)>,
    },
    Requests {
        allowed: bool,
        requests: Vec<(i32, String)>,
    },
    NotFound,
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum AdminMoneyAllResult {
    Applied { affected_players: u64 },
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum AdminRoleResult {
    Applied {
        target_id: PlayerId,
        target_name: String,
    },
    TargetNotFound {
        target_name: String,
    },
    PermanentFailure {
        message: String,
    },
}

#[derive(Debug)]
pub enum AdminSkillResult {
    Saved,
    PermanentFailure { message: String },
}

#[derive(Debug)]
pub enum ClanCommandResult {
    Created { clan_id: i32 },
    Left { clan_id: i32, disbanded: bool },
    Kicked { clan_id: i32, target_id: PlayerId },
    Joined { clan_id: i32 },
    InviteDeclined,
    RequestAccepted { clan_id: i32, target_id: PlayerId },
    RequestDeclined,
    Promoted { clan_id: i32, target_id: PlayerId },
    Invited { target_id: PlayerId },
    RequestSent,
    Rejected { title: String, message: String },
    PermanentFailure { message: String },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BuildingDeleteResult {
    Deleted { cleared_resp_bindings: u64 },
    IdentityMismatch,
    PermanentFailure { message: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveKind {
    Player,
    Building,
    Box,
    Program,
    ProgramCreate,
    ProgramMenu,
    ProgramOpen,
    ProgramRename,
    ProgramDelete,
    ProgramCopy,
    BuildingMenu,
    AuctionGrid,
    AuctionItemOrders,
    AuctionOrder,
    AuctionOrderCreate,
    AuctionBet,
    BuildingDelete,
    ChatAppend,
    ChatColorCycle,
    ChatResync,
    ChatMenu,
    ChatPrivate,
    Whois,
    ClanMenu,
    AdminMoneyAll,
    AdminRole,
    AdminSkill,
    ClanCommand,
}

impl SaveKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Player => "save_player",
            Self::Building => "save_building",
            Self::Box => "save_box",
            Self::Program => "save_program",
            Self::ProgramCreate => "create_program",
            Self::ProgramMenu => "program_menu",
            Self::ProgramOpen => "program_open",
            Self::ProgramRename => "program_rename",
            Self::ProgramDelete => "program_delete",
            Self::ProgramCopy => "program_copy",
            Self::BuildingMenu => "building_menu",
            Self::AuctionGrid => "auction_grid",
            Self::AuctionItemOrders => "auction_item_orders",
            Self::AuctionOrder => "auction_order",
            Self::AuctionOrderCreate => "auction_order_create",
            Self::AuctionBet => "auction_bet",
            Self::BuildingDelete => "delete_building",
            Self::ChatAppend => "save_chat",
            Self::ChatColorCycle => "cycle_chat_color",
            Self::ChatResync => "chat_resync",
            Self::ChatMenu => "chat_menu",
            Self::ChatPrivate => "chat_private",
            Self::Whois => "whois",
            Self::ClanMenu => "clan_menu",
            Self::AdminMoneyAll => "admin_money_all",
            Self::AdminRole => "admin_role",
            Self::AdminSkill => "admin_skill",
            Self::ClanCommand => "clan_command",
        }
    }
}

#[cfg(test)]
pub mod tests;
