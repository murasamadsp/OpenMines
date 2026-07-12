use crate::{
    BuildingExtra, BuildingRow, ChatRow, ClanRank, ClanRow, Database, PlayerRow, ProgramRow, Role,
};
use anyhow::Result;

// TODO: DatabaseProvider trait methods will be used when the abstraction layer is fully wired for dependency injection
#[allow(dead_code)]
pub(crate) trait DatabaseProvider: Send + Sync {
    // buildings
    async fn load_all_buildings(&self) -> Result<Vec<BuildingRow>>;
    async fn insert_building(
        &self,
        type_code: &str,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<i32>;
    #[allow(dead_code)]
    async fn delete_all_buildings(&self) -> Result<u64>;
    async fn update_building_extra(&self, id: i32, extra: &BuildingExtra) -> Result<()>;
    #[allow(clippy::too_many_arguments)]
    async fn update_building_state(
        &self,
        id: i32,
        type_code: u8,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<()>;

    // chats
    async fn add_chat_message(
        &self,
        tag: &str,
        name: &str,
        msg: &str,
        player_id: i32,
    ) -> Result<(i64, i32)>;
    async fn get_recent_chat_messages(&self, tag: &str, limit: usize) -> Result<Vec<ChatRow>>;

    // clans
    async fn create_clan(&self, id: i32, name: &str, abr: &str, owner_id: i32) -> Result<()>;
    async fn get_clan(&self, id: i32) -> Result<Option<ClanRow>>;
    async fn get_clan_members(&self, clan_id: i32) -> Result<Vec<(i32, String, i32)>>;
    async fn list_clans(&self) -> Result<Vec<ClanRow>>;
    async fn delete_clan(&self, id: i32) -> Result<()>;
    async fn add_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()>;
    async fn get_clan_requests(&self, clan_id: i32) -> Result<Vec<(i32, String)>>;
    async fn accept_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()>;
    async fn decline_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()>;
    async fn add_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()>;
    async fn get_player_invites(&self, player_id: i32) -> Result<Vec<(i32, String)>>;
    async fn accept_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()>;
    async fn decline_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()>;
    async fn set_clan_rank(&self, player_id: i32, clan_id: i32, rank: ClanRank) -> Result<()>;
    async fn leave_clan(&self, player_id: i32) -> Result<()>;
    async fn kick_from_clan(&self, player_id: i32) -> Result<()>;
    async fn get_used_clan_ids(&self) -> Result<Vec<i32>>;

    // players
    async fn create_player(&self, name: &str, passwd: &str, hash: &str) -> Result<PlayerRow>;
    #[allow(dead_code)]
    async fn set_player_role(&self, player_id: i32, role: Role) -> Result<bool>;
    async fn get_player_by_id(&self, id: i32) -> Result<Option<PlayerRow>>;
    async fn get_player_by_name(&self, name: &str) -> Result<Option<PlayerRow>>;
    async fn save_player(&self, p: &PlayerRow) -> Result<()>;
    async fn player_name_exists(&self, name: &str) -> Result<bool>;
    async fn update_player_passwd(&self, player_id: i32, passwd: &str) -> Result<()>;
    async fn update_player_resp(
        &self,
        player_id: i32,
        x: Option<i32>,
        y: Option<i32>,
    ) -> Result<()>;
    async fn add_money_to_all(&self, amount: i64) -> Result<usize>;

    // programs
    #[allow(dead_code)]
    async fn list_programs(&self, player_id: i32) -> Result<Vec<ProgramRow>>;
    #[allow(dead_code)]
    async fn get_program(&self, id: i32) -> Result<Option<ProgramRow>>;
    #[allow(dead_code)]
    async fn insert_program(&self, player_id: i32, name: &str, code: &str) -> Result<i32>;
    #[allow(dead_code)]
    async fn update_program(&self, id: i32, code: &str) -> Result<()>;
    #[allow(dead_code)]
    async fn rename_program(&self, id: i32, new_name: &str) -> Result<()>;
    #[allow(dead_code)]
    async fn delete_program(&self, id: i32) -> Result<()>;
}

impl DatabaseProvider for Database {
    async fn load_all_buildings(&self) -> Result<Vec<BuildingRow>> {
        self.load_all_buildings().await
    }
    async fn insert_building(
        &self,
        type_code: &str,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<i32> {
        self.insert_building(type_code, x, y, owner_id, clan_id, extra)
            .await
    }
    async fn delete_all_buildings(&self) -> Result<u64> {
        self.delete_all_buildings().await
    }
    async fn update_building_extra(&self, id: i32, extra: &BuildingExtra) -> Result<()> {
        self.update_building_extra(id, extra).await
    }
    #[allow(clippy::too_many_arguments)]
    async fn update_building_state(
        &self,
        id: i32,
        type_code: u8,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<()> {
        self.update_building_state(id, type_code, x, y, owner_id, clan_id, extra)
            .await
    }

    async fn add_chat_message(
        &self,
        tag: &str,
        name: &str,
        msg: &str,
        player_id: i32,
    ) -> Result<(i64, i32)> {
        self.add_chat_message(tag, name, msg, player_id).await
    }
    async fn get_recent_chat_messages(&self, tag: &str, limit: usize) -> Result<Vec<ChatRow>> {
        self.get_recent_chat_messages(tag, limit).await
    }

    async fn create_clan(&self, id: i32, name: &str, abr: &str, owner_id: i32) -> Result<()> {
        self.create_clan(id, name, abr, owner_id).await
    }
    async fn get_clan(&self, id: i32) -> Result<Option<ClanRow>> {
        self.get_clan(id).await
    }
    async fn get_clan_members(&self, clan_id: i32) -> Result<Vec<(i32, String, i32)>> {
        self.get_clan_members(clan_id).await
    }
    async fn list_clans(&self) -> Result<Vec<ClanRow>> {
        self.list_clans().await
    }
    async fn delete_clan(&self, id: i32) -> Result<()> {
        self.delete_clan(id).await
    }
    async fn add_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.add_clan_request(clan_id, player_id).await
    }
    async fn get_clan_requests(&self, clan_id: i32) -> Result<Vec<(i32, String)>> {
        self.get_clan_requests(clan_id).await
    }
    async fn accept_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.accept_clan_request(clan_id, player_id).await
    }
    async fn decline_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.decline_clan_request(clan_id, player_id).await
    }
    async fn add_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.add_clan_invite(clan_id, player_id).await
    }
    async fn get_player_invites(&self, player_id: i32) -> Result<Vec<(i32, String)>> {
        self.get_player_invites(player_id).await
    }
    async fn accept_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.accept_clan_invite(clan_id, player_id).await
    }
    async fn decline_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.decline_clan_invite(clan_id, player_id).await
    }
    async fn set_clan_rank(&self, player_id: i32, clan_id: i32, rank: ClanRank) -> Result<()> {
        self.set_clan_rank(player_id, clan_id, rank).await
    }
    async fn leave_clan(&self, player_id: i32) -> Result<()> {
        self.leave_clan(player_id).await
    }
    async fn kick_from_clan(&self, player_id: i32) -> Result<()> {
        self.kick_from_clan(player_id).await
    }
    async fn get_used_clan_ids(&self) -> Result<Vec<i32>> {
        self.get_used_clan_ids().await
    }

    async fn create_player(&self, name: &str, passwd: &str, hash: &str) -> Result<PlayerRow> {
        self.create_player(name, passwd, hash).await
    }
    async fn set_player_role(&self, player_id: i32, role: Role) -> Result<bool> {
        self.set_player_role(player_id, role).await
    }
    async fn get_player_by_id(&self, id: i32) -> Result<Option<PlayerRow>> {
        self.get_player_by_id(id).await
    }
    async fn get_player_by_name(&self, name: &str) -> Result<Option<PlayerRow>> {
        self.get_player_by_name(name).await
    }
    async fn save_player(&self, p: &PlayerRow) -> Result<()> {
        self.save_player(p).await
    }
    async fn player_name_exists(&self, name: &str) -> Result<bool> {
        self.player_name_exists(name).await
    }
    async fn update_player_passwd(&self, player_id: i32, passwd: &str) -> Result<()> {
        self.update_player_passwd(player_id, passwd).await
    }
    async fn update_player_resp(
        &self,
        player_id: i32,
        x: Option<i32>,
        y: Option<i32>,
    ) -> Result<()> {
        self.update_player_resp(player_id, x, y).await
    }
    async fn add_money_to_all(&self, amount: i64) -> Result<usize> {
        self.add_money_to_all(amount).await
    }

    async fn list_programs(&self, player_id: i32) -> Result<Vec<ProgramRow>> {
        self.list_programs(player_id).await
    }
    async fn get_program(&self, id: i32) -> Result<Option<ProgramRow>> {
        self.get_program(id).await
    }
    async fn insert_program(&self, player_id: i32, name: &str, code: &str) -> Result<i32> {
        self.insert_program(player_id, name, code).await
    }
    async fn update_program(&self, id: i32, code: &str) -> Result<()> {
        self.update_program(id, code).await
    }
    async fn rename_program(&self, id: i32, new_name: &str) -> Result<()> {
        self.rename_program(id, new_name).await
    }
    async fn delete_program(&self, id: i32) -> Result<()> {
        self.delete_program(id).await
    }
}
