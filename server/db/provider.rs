use crate::db::{
    BuildingExtra, BuildingRow, ClanRank, ClanRow, Database, PlayerRow, ProgramRow, Role,
};
use anyhow::Result;

pub trait DatabaseProvider: Send + Sync {
    // buildings
    fn load_all_buildings(&self) -> Result<Vec<BuildingRow>>;
    fn insert_building(
        &self,
        type_code: &str,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<i32>;
    fn delete_building(&self, building_id: i32) -> Result<()>;
    #[allow(dead_code)]
    fn delete_all_buildings(&self) -> Result<u64>;
    fn update_building_extra(&self, id: i32, extra: &BuildingExtra) -> Result<()>;
    #[allow(clippy::too_many_arguments)]
    fn update_building_state(
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
    fn add_chat_message(&self, tag: &str, name: &str, msg: &str) -> Result<()>;
    fn get_recent_chat_messages(
        &self,
        tag: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, i64)>>;

    // clans
    fn create_clan(&self, id: i32, name: &str, abr: &str, owner_id: i32) -> Result<()>;
    fn get_clan(&self, id: i32) -> Result<Option<ClanRow>>;
    fn get_clan_members(&self, clan_id: i32) -> Result<Vec<(i32, String, i32)>>;
    fn list_clans(&self) -> Result<Vec<ClanRow>>;
    fn delete_clan(&self, id: i32) -> Result<()>;
    fn add_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()>;
    fn get_clan_requests(&self, clan_id: i32) -> Result<Vec<(i32, String)>>;
    fn accept_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()>;
    fn decline_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()>;
    fn add_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()>;
    fn get_player_invites(&self, player_id: i32) -> Result<Vec<(i32, String)>>;
    fn accept_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()>;
    fn decline_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()>;
    fn set_clan_rank(&self, player_id: i32, rank: ClanRank) -> Result<()>;
    fn leave_clan(&self, player_id: i32) -> Result<()>;
    fn kick_from_clan(&self, player_id: i32) -> Result<()>;
    fn get_used_clan_ids(&self) -> Result<Vec<i32>>;

    // players
    fn create_player(&self, name: &str, passwd: &str, hash: &str) -> Result<PlayerRow>;
    #[allow(dead_code)]
    fn set_player_role(&self, player_id: i32, role: Role) -> Result<bool>;
    fn get_player_by_id(&self, id: i32) -> Result<Option<PlayerRow>>;
    fn get_player_by_name(&self, name: &str) -> Result<Option<PlayerRow>>;
    fn save_player(&self, p: &PlayerRow) -> Result<()>;
    fn player_name_exists(&self, name: &str) -> Result<bool>;
    fn update_player_resp(&self, player_id: i32, x: Option<i32>, y: Option<i32>) -> Result<()>;
    fn add_money_to_all(&self, amount: i64) -> Result<usize>;

    // programs
    #[allow(dead_code)]
    fn list_programs(&self, player_id: i32) -> Result<Vec<ProgramRow>>;
    #[allow(dead_code)]
    fn get_program(&self, id: i32) -> Result<Option<ProgramRow>>;
    #[allow(dead_code)]
    fn insert_program(&self, player_id: i32, name: &str, code: &str) -> Result<i32>;
    #[allow(dead_code)]
    fn update_program(&self, id: i32, code: &str) -> Result<()>;
    #[allow(dead_code)]
    fn rename_program(&self, id: i32, new_name: &str) -> Result<()>;
    #[allow(dead_code)]
    fn delete_program(&self, id: i32) -> Result<()>;
}

impl DatabaseProvider for Database {
    fn load_all_buildings(&self) -> Result<Vec<BuildingRow>> {
        self.load_all_buildings()
    }
    fn insert_building(
        &self,
        type_code: &str,
        x: i32,
        y: i32,
        owner_id: i32,
        clan_id: i32,
        extra: &BuildingExtra,
    ) -> Result<i32> {
        self.insert_building(type_code, x, y, owner_id, clan_id, extra)
    }
    fn delete_building(&self, building_id: i32) -> Result<()> {
        self.delete_building(building_id)
    }
    fn delete_all_buildings(&self) -> Result<u64> {
        self.delete_all_buildings()
    }
    fn update_building_extra(&self, id: i32, extra: &BuildingExtra) -> Result<()> {
        self.update_building_extra(id, extra)
    }
    #[allow(clippy::too_many_arguments)]
    fn update_building_state(
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
    }

    fn add_chat_message(&self, tag: &str, name: &str, msg: &str) -> Result<()> {
        self.add_chat_message(tag, name, msg)
    }
    fn get_recent_chat_messages(
        &self,
        tag: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, i64)>> {
        self.get_recent_chat_messages(tag, limit)
    }

    fn create_clan(&self, id: i32, name: &str, abr: &str, owner_id: i32) -> Result<()> {
        self.create_clan(id, name, abr, owner_id)
    }
    fn get_clan(&self, id: i32) -> Result<Option<ClanRow>> {
        self.get_clan(id)
    }
    fn get_clan_members(&self, clan_id: i32) -> Result<Vec<(i32, String, i32)>> {
        self.get_clan_members(clan_id)
    }
    fn list_clans(&self) -> Result<Vec<ClanRow>> {
        self.list_clans()
    }
    fn delete_clan(&self, id: i32) -> Result<()> {
        self.delete_clan(id)
    }
    fn add_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.add_clan_request(clan_id, player_id)
    }
    fn get_clan_requests(&self, clan_id: i32) -> Result<Vec<(i32, String)>> {
        self.get_clan_requests(clan_id)
    }
    fn accept_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.accept_clan_request(clan_id, player_id)
    }
    fn decline_clan_request(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.decline_clan_request(clan_id, player_id)
    }
    fn add_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.add_clan_invite(clan_id, player_id)
    }
    fn get_player_invites(&self, player_id: i32) -> Result<Vec<(i32, String)>> {
        self.get_player_invites(player_id)
    }
    fn accept_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.accept_clan_invite(clan_id, player_id)
    }
    fn decline_clan_invite(&self, clan_id: i32, player_id: i32) -> Result<()> {
        self.decline_clan_invite(clan_id, player_id)
    }
    fn set_clan_rank(&self, player_id: i32, rank: ClanRank) -> Result<()> {
        self.set_clan_rank(player_id, rank)
    }
    fn leave_clan(&self, player_id: i32) -> Result<()> {
        self.leave_clan(player_id)
    }
    fn kick_from_clan(&self, player_id: i32) -> Result<()> {
        self.kick_from_clan(player_id)
    }
    fn get_used_clan_ids(&self) -> Result<Vec<i32>> {
        self.get_used_clan_ids()
    }

    fn create_player(&self, name: &str, passwd: &str, hash: &str) -> Result<PlayerRow> {
        self.create_player(name, passwd, hash)
    }
    fn set_player_role(&self, player_id: i32, role: Role) -> Result<bool> {
        self.set_player_role(player_id, role)
    }
    fn get_player_by_id(&self, id: i32) -> Result<Option<PlayerRow>> {
        self.get_player_by_id(id)
    }
    fn get_player_by_name(&self, name: &str) -> Result<Option<PlayerRow>> {
        self.get_player_by_name(name)
    }
    fn save_player(&self, p: &PlayerRow) -> Result<()> {
        self.save_player(p)
    }
    fn player_name_exists(&self, name: &str) -> Result<bool> {
        self.player_name_exists(name)
    }
    fn update_player_resp(&self, player_id: i32, x: Option<i32>, y: Option<i32>) -> Result<()> {
        self.update_player_resp(player_id, x, y)
    }
    fn add_money_to_all(&self, amount: i64) -> Result<usize> {
        self.add_money_to_all(amount)
    }

    fn list_programs(&self, player_id: i32) -> Result<Vec<ProgramRow>> {
        self.list_programs(player_id)
    }
    fn get_program(&self, id: i32) -> Result<Option<ProgramRow>> {
        self.get_program(id)
    }
    fn insert_program(&self, player_id: i32, name: &str, code: &str) -> Result<i32> {
        self.insert_program(player_id, name, code)
    }
    fn update_program(&self, id: i32, code: &str) -> Result<()> {
        self.update_program(id, code)
    }
    fn rename_program(&self, id: i32, new_name: &str) -> Result<()> {
        self.rename_program(id, new_name)
    }
    fn delete_program(&self, id: i32) -> Result<()> {
        self.delete_program(id)
    }
}
