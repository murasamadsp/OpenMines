/// Роль игрока (`players.role`). Новые уровни — только новыми числами, старые не переиспользовать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Role {
    Player = 0,
    Moderator = 1,
    Admin = 2,
}

impl Role {
    #[must_use]
    pub const fn from_db(v: i32) -> Self {
        match v {
            2 => Self::Admin,
            1 => Self::Moderator,
            _ => Self::Player,
        }
    }

    #[must_use]
    pub const fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }

    /// Модератор или админ (задел под отдельные команды).
    #[must_use]
    pub const fn is_moderator_effective(self) -> bool {
        matches!(self, Self::Moderator | Self::Admin)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum ClanRank {
    None = 0,
    Member = 10,
    Officer = 50,
    Leader = 100,
}

impl ClanRank {
    #[must_use]
    pub const fn from_db(v: i32) -> Self {
        match v {
            100 => Self::Leader,
            50 => Self::Officer,
            10 => Self::Member,
            _ => Self::None,
        }
    }
}
