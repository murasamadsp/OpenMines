//! Definitions of all `PlayerCommand`, `GameEvent`, and `SaveCommand` contracts.
//! These enums decouple the network session tasks, the ECS game loop thread,
//! and the asynchronous database persistence writer task.

use bytes::Bytes;
use openmines_storage::buildings::BuildingExtra;
use openmines_storage::players::PlayerRow;
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
pub enum GuiView {
    Close,
    Teleport(TeleportGuiView),
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
    /// Known TY event that does not mutate gameplay state.
    KnownNoopTy { event: String, payload: Bytes },
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
            Self::ProgramAction { event, .. } if event == "PROG" => Some(SaveKind::Program),
            Self::ChatSettings { .. } => Some(SaveKind::ChatColorCycle),
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
        route: crate::net::session::social::chat::ChannelChatRoute,
        message: openmines_protocol::chat::ChatMessage,
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
            Self::Fanout { .. } => "fanout",
            Self::MovementFanout { .. } => "movement_fanout",
            Self::ChatFanout { .. } => "chat_fanout",
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
    BuildingDelete {
        request: BuildingDeleteRequest,
    },
    ChatAppend {
        request: ChatAppendRequest,
    },
    ChatColorCycle {
        request: ChatColorCycleRequest,
    },
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

impl SaveCommand {
    pub const fn kind(&self) -> SaveKind {
        match self {
            Self::Player { .. } => SaveKind::Player,
            Self::Building { .. } => SaveKind::Building,
            Self::Box { .. } => SaveKind::Box,
            Self::ProgramCreate { .. } => SaveKind::ProgramCreate,
            Self::Program { .. } => SaveKind::Program,
            Self::BuildingDelete { .. } => SaveKind::BuildingDelete,
            Self::ChatAppend { .. } => SaveKind::ChatAppend,
            Self::ChatColorCycle { .. } => SaveKind::ChatColorCycle,
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
    BuildingDeleted {
        request: BuildingDeleteRequest,
        result: BuildingDeleteResult,
    },
    ChatColorCycled {
        request: ChatColorCycleRequest,
        result: ChatColorCycleResult,
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
pub enum ChatColorCycleResult {
    Cycled { color: i32 },
    Rejected,
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
    BuildingDelete,
    ChatAppend,
    ChatColorCycle,
}

impl SaveKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Player => "save_player",
            Self::Building => "save_building",
            Self::Box => "save_box",
            Self::Program => "save_program",
            Self::ProgramCreate => "create_program",
            Self::BuildingDelete => "delete_building",
            Self::ChatAppend => "save_chat",
            Self::ChatColorCycle => "cycle_chat_color",
        }
    }
}
