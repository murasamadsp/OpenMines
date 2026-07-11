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

#[derive(Debug, Clone)]
pub struct BuildingRemoval {
    pub view: PackView,
    pub trigger_pid: Option<PlayerId>,
    pub storage_crystals: Option<[i64; 6]>,
}

/// All incoming action commands from client sessions to the game tick loop.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PlayerCommand {
    /// Initial connection handshake and registration.
    Connect {
        row: Box<openmines_storage::players::PlayerRow>,
        session_id: SessionId,
    },
    /// Clean disconnect of the player session.
    Disconnect {
        player_id: PlayerId,
        session_id: SessionId,
    },
    /// Player movement request.
    Move {
        player_id: PlayerId,
        session_id: SessionId,
        time: u32,
        x: i32,
        y: i32,
        direction: i32,
        programmatic: bool,
    },
    /// Cell digging action request.
    Dig {
        player_id: PlayerId,
        direction: i32,
        programmatic: bool,
    },
    /// Construction or block placement request.
    Build {
        player_id: PlayerId,
        direction: i32,
        block_type: String,
        programmatic: bool,
    },
    /// Geology scan action request.
    Geology {
        player_id: PlayerId,
        programmatic: bool,
    },
    /// Health self-healing request.
    Heal {
        player_id: PlayerId,
        programmatic: bool,
    },
    /// GUI button click action.
    GuiButton { player_id: PlayerId, button: String },
    /// Local area chat message.
    LocalChat {
        player_id: PlayerId,
        message: String,
    },
    /// Global channel chat message.
    ChannelChat { player_id: PlayerId, payload: Bytes },
    /// Request to resynchronize chat history.
    ChatResync { player_id: PlayerId, payload: Bytes },
    /// Chat navigation/channel menu interaction.
    ChatMenu { player_id: PlayerId, payload: Bytes },
    /// Join or select chat channel.
    ChatChoose { player_id: PlayerId, payload: Bytes },
    /// Update individual chat settings.
    ChatSettings { player_id: PlayerId, payload: Bytes },
    /// Send a private chat message to a user.
    ChatPrivate { player_id: PlayerId, payload: Bytes },
    /// Request nicknames for a list of player IDs.
    Whois { player_id: PlayerId, ids: Vec<i32> },
    /// Toggle automatic digging status.
    ToggleAutoDig { player_id: PlayerId },
    /// Toggle player aggression status.
    ToggleAggression { player_id: PlayerId },
    /// Select item index in player inventory.
    InventoryChoose { player_id: PlayerId, payload: Bytes },
    /// Use currently selected inventory item.
    InventoryUse { player_id: PlayerId },
    /// Toggle inventory GUI visibility.
    InventoryToggle { player_id: PlayerId },
    /// Open a nearby box / chest.
    OpenBox { player_id: PlayerId },
    /// Claim daily connection reward.
    ClaimBonus { player_id: PlayerId },
    /// Save client settings payload.
    SettingsSave { player_id: PlayerId, payload: Bytes },
    /// Trigger admin command GUI or panels.
    AdminAction { player_id: PlayerId },
    /// Respawn player after death.
    Respawn { player_id: PlayerId },
    /// Open the programmer program editing GUI.
    OpenProgrammer { player_id: PlayerId },
    /// Request list of building structures owned by the player.
    RequestMyBuildings { player_id: PlayerId },
    /// Open clan management GUI.
    OpenClan { player_id: PlayerId },
    /// Programmator program lifecycle action (save, delete, restart, rename, copy).
    ProgramAction {
        player_id: PlayerId,
        session_id: SessionId,
        event: String,
        payload: Bytes,
    },
    /// Clear deleted programmer runtime state after DB ownership/delete succeeded.
    ApplyDeletedProgram {
        player_id: PlayerId,
        program_id: i32,
    },
    /// Commit an inventory building placement after DB insert succeeded.
    ApplyInventoryBuildingPlaced {
        session_id: SessionId,
        placement: InventoryBuildingPlacement,
        db_id: i32,
    },
    /// Commit a paid GUI building placement after DB insert succeeded.
    ApplyPaidBuildingPlaced {
        session_id: SessionId,
        placement: PaidBuildingPlacement,
        db_id: i32,
    },
    /// Refund money for a paid GUI building placement after DB insert failed.
    RefundPaidBuildingPlacement {
        session_id: SessionId,
        player_id: PlayerId,
        cost: i64,
    },
    /// Commit a building removal after DB delete succeeded.
    ApplyRemovedBuilding { removal: BuildingRemoval },
    /// Apply a GUI program open/create after DB ownership/selection succeeded.
    ApplyProgramEditorOpen {
        session_id: SessionId,
        player_id: PlayerId,
        program_id: i32,
        program_name: String,
        source: String,
    },
    /// Apply a GUI program rename after DB rename succeeded.
    ApplyProgramEditorRename {
        session_id: SessionId,
        player_id: PlayerId,
        program_id: i32,
        program_name: String,
        source: String,
    },
    /// Known TY event that does not mutate gameplay state.
    KnownNoopTy {
        player_id: PlayerId,
        event: String,
        payload: Bytes,
    },
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
            Self::GuiButton { .. } => "gui_button",
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
            Self::ApplyRemovedBuilding { .. } => "apply_removed_building",
            Self::ApplyProgramEditorOpen { .. } => "apply_program_editor_open",
            Self::ApplyProgramEditorRename { .. } => "apply_program_editor_rename",
            Self::KnownNoopTy { .. } => "known_noop_ty",
        }
    }

    pub fn persistence_kind(&self) -> Option<SaveKind> {
        match self {
            Self::Disconnect { .. } | Self::ClaimBonus { .. } => Some(SaveKind::Player),
            Self::ApplyRemovedBuilding { .. } => Some(SaveKind::Box),
            Self::ProgramAction { event, .. } if event == "PROG" => Some(SaveKind::Program),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueuedPlayerCommand {
    pub sequence: CommandSeq,
    pub received_at: Instant,
    pub enqueued_at: Instant,
    pub command: PlayerCommand,
}

#[derive(Debug, Default)]
pub struct CommandEffects {
    pub events: Vec<GameEvent>,
    pub saves: Vec<SaveCommand>,
}

impl CommandEffects {
    pub fn append(&mut self, mut other: Self) {
        self.events.append(&mut other.events);
        self.saves.append(&mut other.saves);
    }
}

/// Outbound work produced by authoritative command application.
#[derive(Debug, Clone)]
pub enum GameEvent {
    SessionBatch {
        session_id: SessionId,
        player_id: PlayerId,
        packets: Vec<Vec<u8>>,
    },
    Fanout {
        recipients: Vec<SessionId>,
        data: Vec<u8>,
    },
}

impl GameEvent {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::SessionBatch { .. } => "session_batch",
            Self::Fanout { .. } => "fanout",
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
    Program {
        request: ProgramSaveRequest,
    },
}

impl SaveCommand {
    pub const fn kind(&self) -> SaveKind {
        match self {
            Self::Player { .. } => SaveKind::Player,
            Self::Building { .. } => SaveKind::Building,
            Self::Box { .. } => SaveKind::Box,
            Self::Program { .. } => SaveKind::Program,
        }
    }
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
    ProgramSaved {
        request: ProgramSaveRequest,
        result: ProgramSaveResult,
    },
}

#[derive(Debug)]
pub enum ProgramSaveResult {
    Saved { program_name: String },
    Rejected,
    PermanentFailure { message: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveKind {
    Player,
    Building,
    Box,
    Program,
}

impl SaveKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Player => "save_player",
            Self::Building => "save_building",
            Self::Box => "save_box",
            Self::Program => "save_program",
        }
    }
}
