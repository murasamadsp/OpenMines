use crate::db::buildings::BuildingExtra;

use super::super::PlayerId;
use super::super::structures::buildings::PackType;

pub struct BuildingInsertSpec<'a> {
    pub type_code: &'a str,
    pub pack_type: PackType,
    pub x: i32,
    pub y: i32,
    pub owner_id: PlayerId,
    pub clan_id: i32,
    pub extra: &'a BuildingExtra,
}
