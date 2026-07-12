use crate::game::PackType;

const ITEM_NAMES: [&str; 51] = [
    "TP",
    "Resp",
    "UP",
    "Market",
    "Clans",
    "boom",
    "prot",
    "raz",
    "Cred",
    "Rembot",
    "geopack",
    "CyanAlive",
    "RedAlive",
    "VioletAlive",
    "BlackAlive",
    "WhiteAlive",
    "BlueAlive",
    "VulcRadar",
    "AliveRadar",
    "BotRadar",
    "TPR",
    "Konstr Bot",
    "Boy gay",
    "Zalupa Zalupa",
    "Crafter",
    "BoomShop",
    "Gun",
    "Gate",
    "Dizz",
    "Storage",
    "PackRadar",
    "x3 up",
    "freeup",
    "mine x4",
    "Gypno",
    "poli",
    "nano bot",
    "accum",
    "transgender",
    "Comp",
    "c190",
    "Fed",
    "BlackRock",
    "RedRock",
    "AntiMage",
    "EMO",
    "RainbowAlive",
    "spot",
    "NC",
    "Money",
    "Оперативные Порно Покемоны.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingItemSpec {
    pub item_id: i32,
    pub pack_type: PackType,
    pub placement_offset: i32,
    pub database_code: &'static str,
    pub drops_on_destroy: bool,
}

const BUILDING_ITEMS: [BuildingItemSpec; 9] = [
    BuildingItemSpec::new(0, PackType::Teleport, 2, "T", true),
    BuildingItemSpec::new(1, PackType::Resp, 2, "R", true),
    BuildingItemSpec::new(2, PackType::Up, 2, "U", true),
    BuildingItemSpec::new(3, PackType::Market, 2, "M", true),
    BuildingItemSpec::new(4, PackType::Clans, 2, "D", false),
    BuildingItemSpec::new(24, PackType::Craft, 2, "F", true),
    BuildingItemSpec::new(26, PackType::Gun, 2, "G", true),
    BuildingItemSpec::new(27, PackType::Gate, 1, "N", false),
    BuildingItemSpec::new(29, PackType::Storage, 2, "L", true),
];

impl BuildingItemSpec {
    const fn new(
        item_id: i32,
        pack_type: PackType,
        placement_offset: i32,
        database_code: &'static str,
        drops_on_destroy: bool,
    ) -> Self {
        Self {
            item_id,
            pack_type,
            placement_offset,
            database_code,
            drops_on_destroy,
        }
    }
}

pub fn item_name(item_id: i32) -> &'static str {
    usize::try_from(item_id)
        .ok()
        .and_then(|index| ITEM_NAMES.get(index))
        .copied()
        .unwrap_or("")
}

pub fn building_item(item_id: i32) -> Option<BuildingItemSpec> {
    BUILDING_ITEMS
        .iter()
        .copied()
        .find(|spec| spec.item_id == item_id)
}

pub fn building_item_for_pack(pack_type: PackType) -> Option<BuildingItemSpec> {
    BUILDING_ITEMS
        .iter()
        .copied()
        .find(|spec| spec.pack_type == pack_type)
}

pub fn destroyed_building_drop(pack_type: PackType) -> Option<i32> {
    building_item_for_pack(pack_type)
        .filter(|spec| spec.drops_on_destroy)
        .map(|spec| spec.item_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn building_catalog_roundtrips_supported_items() {
        for spec in BUILDING_ITEMS {
            assert_eq!(building_item(spec.item_id), Some(spec));
            assert_eq!(building_item_for_pack(spec.pack_type), Some(spec));
        }
    }

    #[test]
    fn destroy_drop_policy_keeps_legacy_exclusions() {
        assert_eq!(destroyed_building_drop(PackType::Teleport), Some(0));
        assert_eq!(destroyed_building_drop(PackType::Storage), Some(29));
        assert_eq!(destroyed_building_drop(PackType::Gate), None);
        assert_eq!(destroyed_building_drop(PackType::Clans), None);
    }
}
