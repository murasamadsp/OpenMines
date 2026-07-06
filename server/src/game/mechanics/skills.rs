use crate::db::SkillSlots;
use std::collections::HashMap;

/// Коды (`#[strum(serialize)]`) — единственный источник истины для wire (@S) и
/// БД (skills JSON keyed by code). `code()`/`from_code()` выведены из них через
/// strum, что исключает рассинхрон прямого/обратного маппинга.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    strum::IntoStaticStr,
    strum::EnumString,
    strum::EnumIter,
)]
pub enum SkillType {
    #[strum(serialize = "a")]
    AntiSlime = 0,
    #[strum(serialize = "k")]
    AntiBlock = 1,
    #[strum(serialize = "j")]
    AdjacentExtraction = 2,
    #[strum(serialize = "U")]
    Geology = 3,
    #[strum(serialize = "B")]
    MineBlue = 4,
    #[strum(serialize = "G")]
    MineGreen = 5,
    #[strum(serialize = "D")]
    Destruction = 6,
    #[strum(serialize = "x")]
    Annihilation = 7,
    #[strum(serialize = "y")]
    Crystallography = 8,
    #[strum(serialize = "z")]
    Deconstruction = 9,
    #[strum(serialize = "u")]
    AntiGun = 10,
    #[strum(serialize = "E")]
    BuildRed = 11,
    #[strum(serialize = "d")]
    Digging = 12,
    #[strum(serialize = "l")]
    Health = 13,
    #[strum(serialize = "m")]
    MineGeneral = 14,
    #[strum(serialize = "R")]
    MineRed = 15,
    #[strum(serialize = "L")]
    BuildGreen = 16,
    #[strum(serialize = "Q")]
    BuildQuadro = 17,
    #[strum(serialize = "q")]
    Detection = 18,
    #[strum(serialize = "M")]
    Movement = 19,
    #[strum(serialize = "Y")]
    BuildYellow = 20,
    #[strum(serialize = "P")]
    Compression = 21,
    #[strum(serialize = "F")]
    Fridge = 22,
    #[strum(serialize = "C")]
    MineCyan = 23,
    #[strum(serialize = "t")]
    RoadMovement = 24,
    #[strum(serialize = "*U")]
    Upgrade = 25,
    #[strum(serialize = "Z")]
    Deactivation = 26,
    #[strum(serialize = "h")]
    HyperPacking = 27,
    #[strum(serialize = "V")]
    MineViolet = 28,
    #[strum(serialize = "p")]
    Packing = 29,
    #[strum(serialize = "b")]
    PackingBlue = 30,
    #[strum(serialize = "c")]
    PackingCyan = 31,
    #[strum(serialize = "v")]
    PackingViolet = 32,
    #[strum(serialize = "*M")]
    Discount = 33,
    #[strum(serialize = "J")]
    Sort = 34,
    #[strum(serialize = "S")]
    Turbo = 35,
    #[strum(serialize = "X")]
    DeMagnetizing = 36,
    #[strum(serialize = "W")]
    MineWhite = 37,
    #[strum(serialize = "r")]
    PackingRed = 38,
    #[strum(serialize = "w")]
    PackingWhite = 39,
    #[strum(serialize = "g")]
    PackingGreen = 40,
    #[strum(serialize = "o")]
    Extraction = 41,
    #[strum(serialize = "e")]
    Repair = 42,
    #[strum(serialize = "*D")]
    ExpertMining = 43,
    #[strum(serialize = "i")]
    Washing = 44,
    #[strum(serialize = "f")]
    Fracturing = 45,
    #[strum(serialize = "H")]
    NanoPacking = 46,
    #[strum(serialize = "O")]
    BuildStructure = 47,
    #[strum(serialize = "A")]
    BuildRoad = 48,
    #[strum(serialize = "*B")]
    BuildUniversal = 49,
    #[strum(serialize = "*L")]
    BuildWar = 50,
    #[strum(serialize = "*A")]
    Architecture = 51,
    #[strum(serialize = "*T")]
    TotalDestruction = 52,
    #[strum(serialize = "*u")]
    UltraWhite = 53,
    #[strum(serialize = "*J")]
    Jewlery = 54,
    #[strum(serialize = "*I")]
    Induction = 55,
    #[strum(serialize = "*a")]
    MineSlime = 56,
    #[strum(serialize = "*d")]
    MineDeep = 57,
    #[strum(serialize = "*g")]
    GluonPacking = 58,
}

impl SkillType {
    #[must_use]
    /// Wire/БД-код навыка (выведен из `#[strum(serialize)]`).
    pub fn code(self) -> &'static str {
        self.into()
    }

    #[must_use]
    /// Навык по wire/БД-коду (выведен из `#[strum(serialize)]`).
    pub fn from_code(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    #[must_use]
    pub const fn effect_type(self) -> SkillEffectType {
        match self {
            Self::Digging => SkillEffectType::OnDig,
            Self::MineGeneral
            | Self::MineBlue
            | Self::MineGreen
            | Self::MineRed
            | Self::MineCyan
            | Self::MineViolet
            | Self::MineWhite
            | Self::MineSlime
            | Self::MineDeep
            | Self::UltraWhite
            | Self::Jewlery => SkillEffectType::OnDigCrys,
            Self::Movement | Self::RoadMovement | Self::Fridge | Self::Turbo => {
                SkillEffectType::OnMove
            }
            Self::BuildGreen
            | Self::BuildRoad
            | Self::BuildYellow
            | Self::BuildRed
            | Self::BuildQuadro
            | Self::BuildStructure
            | Self::BuildUniversal
            | Self::BuildWar
            | Self::Architecture => SkillEffectType::OnBld,
            Self::Health | Self::Repair => SkillEffectType::OnHealth,
            Self::AntiGun | Self::AntiSlime | Self::AntiBlock | Self::Induction => {
                SkillEffectType::OnHurt
            }
            Self::Packing
            | Self::PackingBlue
            | Self::PackingCyan
            | Self::PackingGreen
            | Self::PackingRed
            | Self::PackingViolet
            | Self::PackingWhite
            | Self::Compression
            | Self::HyperPacking
            | Self::NanoPacking
            | Self::GluonPacking => SkillEffectType::OnPackCrys,
            _ => SkillEffectType::OnExp,
        }
    }

    #[must_use]
    pub const fn category(self) -> SkillCategory {
        match self {
            Self::Digging
            | Self::Destruction
            | Self::Annihilation
            | Self::Deconstruction
            | Self::TotalDestruction => SkillCategory::Digging,
            Self::MineGeneral
            | Self::MineBlue
            | Self::MineGreen
            | Self::MineRed
            | Self::MineCyan
            | Self::MineViolet
            | Self::MineWhite
            | Self::MineSlime
            | Self::MineDeep
            | Self::UltraWhite
            | Self::Jewlery
            | Self::Geology
            | Self::Crystallography
            | Self::Detection
            | Self::Extraction
            | Self::ExpertMining
            | Self::Washing
            | Self::Fracturing
            | Self::AdjacentExtraction => SkillCategory::Mining,
            Self::BuildGreen
            | Self::BuildRoad
            | Self::BuildYellow
            | Self::BuildRed
            | Self::BuildQuadro
            | Self::BuildStructure
            | Self::BuildUniversal
            | Self::BuildWar
            | Self::Architecture => SkillCategory::Building,
            Self::Movement
            | Self::RoadMovement
            | Self::Fridge
            | Self::Turbo
            | Self::Deactivation => SkillCategory::Movement,
            Self::Health
            | Self::Repair
            | Self::AntiGun
            | Self::AntiSlime
            | Self::AntiBlock
            | Self::Induction
            | Self::DeMagnetizing => SkillCategory::HP,
            Self::Packing
            | Self::PackingBlue
            | Self::PackingCyan
            | Self::PackingGreen
            | Self::PackingRed
            | Self::PackingViolet
            | Self::PackingWhite
            | Self::Compression
            | Self::HyperPacking
            | Self::NanoPacking
            | Self::GluonPacking
            | Self::Sort
            | Self::Discount
            | Self::Upgrade => SkillCategory::Packing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCategory {
    Digging,
    Mining,
    Building,
    Movement,
    HP,
    Packing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // Prefixed with 'On' to match C# reference architecture naming conventions
pub enum SkillEffectType {
    OnDig,
    OnExp,
    OnMove,
    OnPackCrys,
    OnHurt,
    OnUp,
    OnBld,
    OnDigCrys,
    OnHealth,
}

#[allow(dead_code)] // Will be used when digging skill hooks are integrated
pub trait OnDig {
    fn on_dig(&self, current_val: f32) -> f32;
}

#[allow(dead_code)] // Will be used when digging crystal skill hooks are integrated
pub trait OnDigCrys {
    fn on_dig_crys(&self, current_val: f32) -> f32;
}

#[allow(dead_code)] // Will be used when movement speed modifiers are wired into packet handlers
pub trait OnMove {
    fn on_move(&self, current_val: f32) -> f32;
    fn on_move_road(&self, current_val: f32) -> f32;
}

#[allow(dead_code)] // Will be used when building construction cost/HP modifiers are integrated
pub trait OnBld {
    fn on_bld(&self, skill: SkillType, current_cost: f32) -> f32;
    fn on_bld_hp(&self, skill: SkillType, current_hp: f32) -> f32;
}

#[allow(dead_code)] // Will be used when player health regeneration ticking is implemented
pub trait OnHealth {
    fn on_health_max(&self, current_max: i32) -> i32;
    fn on_health_regen(&self, current_regen: f32) -> f32;
}

pub trait OnHurt {
    fn on_hurt(&self, current_damage: f32) -> f32;
}

#[allow(dead_code)] // Will be used when pack capacity enhancements are checked during crystal pickups
pub trait OnPackCrys {
    fn on_pack_crys_capacity(&self, current_capacity: i64) -> i64;
}

#[allow(dead_code)] // Will be used when experience scaling modifiers are implemented
pub trait OnExp {
    fn on_exp(&self, current_val: f32) -> f32;
}

pub struct PlayerSkills<'a> {
    pub skills: &'a SkillSlots,
}

fn level_to_f32(level: i32) -> f32 {
    level.to_string().parse::<f32>().unwrap_or(0.0)
}

impl OnDig for PlayerSkills<'_> {
    fn on_dig(&self, _current_val: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::Digging)
    }
}

impl OnDigCrys for PlayerSkills<'_> {
    fn on_dig_crys(&self, _current_val: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::MineGeneral)
    }
}

impl OnMove for PlayerSkills<'_> {
    fn on_move(&self, _current_val: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::Movement)
    }
    fn on_move_road(&self, _current_val: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::RoadMovement)
    }
}

impl OnBld for PlayerSkills<'_> {
    fn on_bld(&self, skill: SkillType, _current_cost: f32) -> f32 {
        get_player_skill_effect(self.skills, skill)
    }
    fn on_bld_hp(&self, skill: SkillType, _current_hp: f32) -> f32 {
        match skill {
            SkillType::BuildGreen
            | SkillType::BuildYellow
            | SkillType::BuildRed
            | SkillType::BuildWar => self
                .skills
                .find(skill.code())
                .map_or(1.0, |s| level_to_f32(s.level)),
            _ => 1.0,
        }
    }
}

impl OnHealth for PlayerSkills<'_> {
    fn on_health_max(&self, _current_max: i32) -> i32 {
        #[allow(clippy::cast_possible_truncation)]
        let val = get_player_skill_effect(self.skills, SkillType::Health) as i32;
        val
    }
    fn on_health_regen(&self, _current_regen: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::Repair)
    }
}

impl OnHurt for PlayerSkills<'_> {
    fn on_hurt(&self, current_damage: f32) -> f32 {
        let anti_gun = get_player_skill_effect(self.skills, SkillType::AntiGun);
        // C# Player.Hurt: `eff = (int)(num * Effect/100); num -= eff` — усекается
        // СНИЖЕНИЕ урона (а не итог), всегда в пользу чуть большего урона.
        // Прежний `dmg*(1-ag/100)` + round в combat давал расхождение ≤1 HP.
        let reduction = (current_damage * anti_gun / 100.0).trunc();
        current_damage - reduction
    }
}

impl OnPackCrys for PlayerSkills<'_> {
    fn on_pack_crys_capacity(&self, _current_capacity: i64) -> i64 {
        #[allow(clippy::cast_possible_truncation)]
        let val = get_player_skill_effect(self.skills, SkillType::Packing) as i64;
        val
    }
}

impl OnExp for PlayerSkills<'_> {
    fn on_exp(&self, current_val: f32) -> f32 {
        self.skills
            .find(SkillType::Upgrade.code())
            .map_or(current_val, |upgr_state| {
                current_val * skill_effect(SkillType::Upgrade, upgr_state.level)
            })
    }
}

/// Calculate the gameplay effect value for a skill at a given level.
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::too_many_lines)]
pub fn skill_effect(skill: SkillType, level: i32) -> f32 {
    let x = level as f32;
    match skill {
        SkillType::Digging => x.mul_add(10.0, 100.0),
        // 1:1 ref (`server_reference/.../PlayerSkills.cs`, SkillType.Movement):
        // effectfunc = (x) => 70f - x * 0.05f > 30f ? 70f - x * 0.05f : 30f
        SkillType::Movement => x.mul_add(-0.05, 70.0).max(30.0),
        SkillType::MineGeneral => {
            if x <= 0.0 {
                0.08
            } else {
                0.08 + x.log10() * x.sqrt() / 4.0
            }
        }
        SkillType::Health => x.mul_add(3.0, 100.0),
        SkillType::BuildRoad => x.mul_add(-0.2_f32, 5.0).max(1.0),
        SkillType::Induction => x.mul_add(0.2, 100.0),
        SkillType::Packing => x.mul_add(20.0, 100.0),
        SkillType::AntiGun => {
            if x <= 0.0 {
                1.0
            } else {
                let val = x
                    .mul_add(-0.098, 1.0 + x - x.log10() * x.powf(0.9) / 2.0)
                    .round();
                val.min(92.0)
            }
        }
        // D23: C# effectfunc = (x) => 1 for BuildGreen/Yellow/Red (cost is always 1 crystal).
        // D23: C# effectfunc = (x) => 1 for all Build* types, Fridge, RoadMovement.
        SkillType::BuildGreen
        | SkillType::BuildYellow
        | SkillType::BuildRed
        | SkillType::BuildStructure
        | SkillType::BuildWar
        | SkillType::Fridge
        | SkillType::RoadMovement => 1.0,
        _ => x,
    }
}

/// Experience needed to level up from the current level.
#[allow(clippy::cast_precision_loss)]
pub const fn exp_needed(skill: SkillType, _level: i32) -> f32 {
    match skill {
        SkillType::AntiGun => 0.0,
        _ => 1.0,
    }
}

/// Get the effect value for a player's skill, defaulting to level 0 if not present.
pub fn get_player_skill_effect(skills: &SkillSlots, skill: SkillType) -> f32 {
    skills
        .find(skill.code())
        .map_or_else(|| skill_effect(skill, 0), |s| skill_effect(skill, s.level))
}

/// Add exp to a skill IF установлен в слот. C# `Skill.AddExp`: вызывается только
/// для установленных скиллов (`UseSkill` итерирует `skills.Values`), НЕ делает
/// auto-level-up (level-up — только через Up GUI). Возвращает `true`, если скилл
/// установлен и exp добавлен (вызывающие тогда шлют @S); `false`, если скилла нет
/// в слотах (тогда @S не нужен — 1:1 C#, неустановленный скилл опыт не копит).
pub fn add_skill_exp(skills: &mut SkillSlots, code: &str, amount: f32) -> bool {
    let upgrade_mult = skills
        .find(SkillType::Upgrade.code())
        .map_or(1.0, |s| skill_effect(SkillType::Upgrade, s.level));

    if let Some(entry) = skills.find_mut(code) {
        entry.exp = amount.mul_add(upgrade_mult, entry.exp);
        true
    } else {
        false
    }
}

/// Convert skills into outbound packets payload (`(skill_code, percent)`), preserving legacy rounding.
#[must_use]
pub fn skill_progress_payload(skills: &SkillSlots) -> Vec<(String, i32)> {
    skills
        .skills
        .values()
        .map(|s| {
            let skill_type = SkillType::from_code(&s.code);
            let needed = skill_type.map_or(1.0, |st| exp_needed(st, s.level));
            // C# `Skill.AddExp`: pct = `(int)(exp*100/Expiriense)` — усечение, БЕЗ
            // клампа. Клиент `MiniSkill` различает >=100 (стрелка «ап») и >=200
            // (удвоенная полоса); прежний `clamp(0,100)` делал >=200 недостижимым.
            // Guard деления на ноль (`needed>0`) сохранён — AntiGun имеет expfunc=0.
            let pct = if needed > 0.0 {
                #[allow(clippy::cast_possible_truncation)]
                {
                    (f64::from(s.exp) * 100.0 / f64::from(needed)) as i32
                }
            } else {
                100
            };
            (s.code.clone(), pct)
        })
        .collect()
}

pub fn get_skill_requirements(skill: SkillType) -> Option<HashMap<SkillType, i32>> {
    let mut reqs = HashMap::new();
    match skill {
        SkillType::MineGeneral | SkillType::BuildRoad => {
            reqs.insert(SkillType::Digging, 5);
        }
        SkillType::MineBlue => {
            reqs.insert(SkillType::MineGeneral, 5);
        }
        SkillType::MineGreen => {
            reqs.insert(SkillType::MineBlue, 10);
        }
        SkillType::MineRed => {
            reqs.insert(SkillType::MineGreen, 10);
        }
        SkillType::MineCyan => {
            reqs.insert(SkillType::MineRed, 15);
        }
        SkillType::MineViolet => {
            reqs.insert(SkillType::MineCyan, 15);
        }
        SkillType::MineWhite => {
            reqs.insert(SkillType::MineViolet, 20);
        }

        SkillType::PackingBlue => {
            reqs.insert(SkillType::Packing, 5);
        }
        SkillType::PackingGreen => {
            reqs.insert(SkillType::PackingBlue, 10);
        }

        SkillType::RoadMovement => {
            reqs.insert(SkillType::Movement, 10);
        }

        SkillType::BuildGreen => {
            reqs.insert(SkillType::BuildRoad, 5);
        }
        SkillType::BuildYellow => {
            reqs.insert(SkillType::BuildGreen, 10);
        }
        SkillType::BuildRed => {
            reqs.insert(SkillType::BuildYellow, 10);
        }
        SkillType::BuildQuadro => {
            reqs.insert(SkillType::BuildRed, 15);
        }
        SkillType::BuildStructure => {
            reqs.insert(SkillType::BuildQuadro, 15);
        }
        SkillType::BuildUniversal => {
            reqs.insert(SkillType::BuildStructure, 20);
        }

        _ => return None,
    }
    Some(reqs)
}

#[allow(dead_code)] // Will be used when skill purchase/installation GUI events are wired up in session handlers
pub fn can_install_skill(player_skills: &SkillSlots, skill: SkillType) -> bool {
    if player_skills.find(skill.code()).is_some() {
        return false;
    }
    if let Some(reqs) = get_skill_requirements(skill) {
        for (req_skill, req_lvl) in reqs {
            if let Some(s) = player_skills.find(req_skill.code()) {
                if s.level < req_lvl {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::SkillType;
    use strum::IntoEnumIterator as _;

    #[test]
    fn code_from_code_roundtrip_all_variants() {
        // Залочить bijection code()↔from_code() для всех вариантов после перехода
        // на strum: коды персистятся в БД (skills JSON) и на проводе (@S).
        for skill in SkillType::iter() {
            assert_eq!(SkillType::from_code(skill.code()), Some(skill));
        }
    }

    #[test]
    fn from_code_unknown_is_none() {
        assert_eq!(SkillType::from_code("?"), None);
        assert_eq!(SkillType::from_code(""), None);
    }
}
