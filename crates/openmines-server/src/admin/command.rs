use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AdminCommandName {
    Give,
    GiveAll,
    Money,
    MoneyAll,
    #[serde(rename = "tp")]
    Teleport,
    Heal,
    Kill,
    Skill,
    Kick,
    Role,
    Clan,
    Pack,
    Announce,
    Online,
    Find,
    Info,
    Save,
    Schedule,
    Shutdown,
    Help,
}

impl AdminCommandName {
    #[must_use]
    pub fn from_slash(token: &str) -> Option<Self> {
        match token {
            "/give" => Some(Self::Give),
            "/giveall" => Some(Self::GiveAll),
            "/money" => Some(Self::Money),
            "/moneyall" => Some(Self::MoneyAll),
            "/tp" => Some(Self::Teleport),
            "/heal" => Some(Self::Heal),
            "/skill" => Some(Self::Skill),
            "/kick" => Some(Self::Kick),
            "/role" => Some(Self::Role),
            "/clan" => Some(Self::Clan),
            "/pack" => Some(Self::Pack),
            "/admin" | "/adminhelp" => Some(Self::Help),
            _ => None,
        }
    }

    #[must_use]
    pub fn from_console(token: &str) -> Option<Self> {
        match token {
            "give" => Some(Self::Give),
            "money" => Some(Self::Money),
            "tp" => Some(Self::Teleport),
            "heal" => Some(Self::Heal),
            "kill" => Some(Self::Kill),
            "kick" => Some(Self::Kick),
            "role" => Some(Self::Role),
            "announce" => Some(Self::Announce),
            "online" => Some(Self::Online),
            "find" => Some(Self::Find),
            "info" => Some(Self::Info),
            "save" => Some(Self::Save),
            "schedule" => Some(Self::Schedule),
            "stop" | "shutdown" => Some(Self::Shutdown),
            "help" | "?" => Some(Self::Help),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AdminCommandName;

    #[test]
    fn surface_parsers_share_command_identity_and_keep_aliases_local() {
        assert_eq!(
            AdminCommandName::from_slash("/money"),
            Some(AdminCommandName::Money)
        );
        assert_eq!(
            AdminCommandName::from_console("money"),
            Some(AdminCommandName::Money)
        );
        assert_eq!(
            AdminCommandName::from_slash("/adminhelp"),
            Some(AdminCommandName::Help)
        );
        assert_eq!(
            AdminCommandName::from_console("stop"),
            Some(AdminCommandName::Shutdown)
        );
        assert_eq!(AdminCommandName::from_console("giveall"), None);
        assert_eq!(AdminCommandName::from_slash("/shutdown"), None);
    }
}
