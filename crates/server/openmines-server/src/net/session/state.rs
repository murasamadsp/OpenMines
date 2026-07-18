use crate::game::PlayerId;

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum AuthState {
    PreAuth,
    GuiAuth(GuiAuthStep),
    Authenticated { player_id: PlayerId },
}

impl AuthState {
    pub const fn player_id(&self) -> Option<PlayerId> {
        match self {
            Self::Authenticated { player_id } => Some(*player_id),
            Self::PreAuth | Self::GuiAuth(_) => None,
        }
    }
}

/// Sub-state of the GUI auth flow (registration / login through client GUI).
/// 1:1 with C# `Auth` class state machine.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum GuiAuthStep {
    /// Default window: "Новый акк" / "ok" (nick input).
    MainMenu,
    /// Player found by nick, waiting for password input.
    LoginPassword { nick: String },
    /// Creating new account — waiting for nick input.
    RegisterNick,
    /// Creating new account — nick accepted, waiting for password.
    RegisterPassword { nick: String },
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum HeartbeatGate {
    WaitingForAuthResponse,
    AuthResponseQueued,
    Enabled,
}

impl HeartbeatGate {
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub const fn mark_auth_response_queued(&mut self) {
        *self = Self::AuthResponseQueued;
    }

    pub fn enable_if_auth_response_flushed(&mut self) -> bool {
        if *self == Self::AuthResponseQueued {
            *self = Self::Enabled;
            true
        } else {
            false
        }
    }
}
