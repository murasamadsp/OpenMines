use bevy_ecs::prelude::Resource;

use super::super::logic::crafting;
use super::super::structures::buildings::{BuildingCrafting, PackType};

/// Запись персистенции бокса: (координата, `Some`=upsert | `None`=delete).
/// Пакет в HB-overlay здания: поля именованы для читаемости (IR-3).
#[derive(Clone, Copy, Debug)]
pub struct PackOverlay {
    /// Код типа здания (`PackType::code()`).
    pub code: u8,
    /// X-координата здания (сетевой u16, `rem_euclid(65536)`).
    pub x: u16,
    /// Y-координата здания (сетевой u16).
    pub y: u16,
    /// Клановый ID (`clan_id.clamp(0,255) as u8`).
    pub clan: u8,
    /// HB `O` entry off-byte. For charge-based packs: `charge > 0`; for Craft:
    /// `1 + recipe.result.id`, plus 50 when ready.
    pub off: u8,
}

pub fn pack_overlay_off(
    pack_type: PackType,
    charge: i32,
    craft: Option<&BuildingCrafting>,
    now: i64,
) -> u8 {
    if pack_type != PackType::Craft {
        return u8::from(charge > 0);
    }
    let Some(recipe_id) = craft.and_then(|c| c.recipe_id) else {
        return 0;
    };
    let Some(recipe) = crafting::recipe_by_id(recipe_id) else {
        return 0;
    };
    let ready_bonus = if craft.is_some_and(|c| c.end_ts > 0 && now >= c.end_ts) {
        50
    } else {
        0
    };
    u8::try_from(1 + recipe.result.id + ready_bonus)
        .expect("craft recipe overlay item id must fit HB O off byte")
}

/// Координаты зданий, которым нужен HB O re-broadcast после обнуления charge (C# `ResendPack`).
#[derive(Resource, Default)]
pub struct PackResendQueue(pub Vec<(i32, i32)>);
