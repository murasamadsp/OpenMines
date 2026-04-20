#![allow(
    dead_code,
    clippy::enum_variant_names,
    clippy::elidable_lifetime_names,
    clippy::cast_precision_loss,
    clippy::option_if_let_else,
    clippy::match_same_arms,
    clippy::suboptimal_flops,
    clippy::map_unwrap_or
)]

use crate::db::SkillState;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SkillType {
    AntiSlime = 0,
    AntiBlock = 1,
    AdjacentExtraction = 2,
    Geology = 3,
    MineBlue = 4,
    MineGreen = 5,
    Destruction = 6,
    Annihilation = 7,
    Crystallography = 8,
    Deconstruction = 9,
    AntiGun = 10,
    BuildRed = 11,
    Digging = 12,
    Health = 13,
    MineGeneral = 14,
    MineRed = 15,
    BuildGreen = 16,
    BuildQuadro = 17,
    Detection = 18,
    Movement = 19,
    BuildYellow = 20,
    Compression = 21,
    Fridge = 22,
    MineCyan = 23,
    RoadMovement = 24,
    Upgrade = 25,
    Deactivation = 26,
    HyperPacking = 27,
    MineViolet = 28,
    Packing = 29,
    PackingBlue = 30,
    PackingCyan = 31,
    PackingViolet = 32,
    Discount = 33,
    Sort = 34,
    Turbo = 35,
    DeMagnetizing = 36,
    MineWhite = 37,
    PackingRed = 38,
    PackingWhite = 39,
    PackingGreen = 40,
    Extraction = 41,
    Repair = 42,
    ExpertMining = 43,
    Washing = 44,
    Fracturing = 45,
    NanoPacking = 46,
    BuildStructure = 47,
    BuildRoad = 48,
    BuildUniversal = 49,
    BuildWar = 50,
    Architecture = 51,
    TotalDestruction = 52,
    UltraWhite = 53,
    Jewlery = 54,
    Induction = 55,
    MineSlime = 56,
    MineDeep = 57,
    GluonPacking = 58,
}

impl SkillType {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AntiSlime => "a",
            Self::AntiBlock => "k",
            Self::AdjacentExtraction => "j",
            Self::Geology => "U",
            Self::MineBlue => "B",
            Self::MineGreen => "G",
            Self::Destruction => "D",
            Self::Annihilation => "x",
            Self::Crystallography => "y",
            Self::Deconstruction => "z",
            Self::AntiGun => "u",
            Self::BuildRed => "E",
            Self::Digging => "d",
            Self::Health => "l",
            Self::MineGeneral => "m",
            Self::MineRed => "R",
            Self::BuildGreen => "L",
            Self::BuildQuadro => "Q",
            Self::Detection => "q",
            Self::Movement => "M",
            Self::BuildYellow => "Y",
            Self::Compression => "P",
            Self::Fridge => "F",
            Self::MineCyan => "C",
            Self::RoadMovement => "t",
            Self::Upgrade => "*U",
            Self::Deactivation => "Z",
            Self::HyperPacking => "h",
            Self::MineViolet => "V",
            Self::Packing => "p",
            Self::PackingBlue => "b",
            Self::PackingCyan => "c",
            Self::PackingViolet => "v",
            Self::Discount => "*M",
            Self::Sort => "J",
            Self::Turbo => "S",
            Self::DeMagnetizing => "X",
            Self::MineWhite => "W",
            Self::PackingRed => "r",
            Self::PackingWhite => "w",
            Self::PackingGreen => "g",
            Self::Extraction => "o",
            Self::Repair => "e",
            Self::ExpertMining => "*D",
            Self::Washing => "i",
            Self::Fracturing => "f",
            Self::NanoPacking => "H",
            Self::BuildStructure => "O",
            Self::BuildRoad => "A",
            Self::BuildUniversal => "*B",
            Self::BuildWar => "*L",
            Self::Architecture => "*A",
            Self::TotalDestruction => "*T",
            Self::UltraWhite => "*u",
            Self::Jewlery => "*J",
            Self::Induction => "*I",
            Self::MineSlime => "*a",
            Self::MineDeep => "*d",
            Self::GluonPacking => "*g",
        }
    }

    #[must_use]
    pub fn from_code(s: &str) -> Option<Self> {
        match s {
            "a" => Some(Self::AntiSlime),
            "k" => Some(Self::AntiBlock),
            "j" => Some(Self::AdjacentExtraction),
            "U" => Some(Self::Geology),
            "B" => Some(Self::MineBlue),
            "G" => Some(Self::MineGreen),
            "D" => Some(Self::Destruction),
            "x" => Some(Self::Annihilation),
            "y" => Some(Self::Crystallography),
            "z" => Some(Self::Deconstruction),
            "u" => Some(Self::AntiGun),
            "E" => Some(Self::BuildRed),
            "d" => Some(Self::Digging),
            "l" => Some(Self::Health),
            "m" => Some(Self::MineGeneral),
            "R" => Some(Self::MineRed),
            "L" => Some(Self::BuildGreen),
            "Q" => Some(Self::BuildQuadro),
            "q" => Some(Self::Detection),
            "M" => Some(Self::Movement),
            "Y" => Some(Self::BuildYellow),
            "P" => Some(Self::Compression),
            "F" => Some(Self::Fridge),
            "C" => Some(Self::MineCyan),
            "t" => Some(Self::RoadMovement),
            "*U" => Some(Self::Upgrade),
            "Z" => Some(Self::Deactivation),
            "h" => Some(Self::HyperPacking),
            "V" => Some(Self::MineViolet),
            "p" => Some(Self::Packing),
            "b" => Some(Self::PackingBlue),
            "c" => Some(Self::PackingCyan),
            "v" => Some(Self::PackingViolet),
            "*M" => Some(Self::Discount),
            "J" => Some(Self::Sort),
            "S" => Some(Self::Turbo),
            "X" => Some(Self::DeMagnetizing),
            "W" => Some(Self::MineWhite),
            "r" => Some(Self::PackingRed),
            "w" => Some(Self::PackingWhite),
            "g" => Some(Self::PackingGreen),
            "o" => Some(Self::Extraction),
            "e" => Some(Self::Repair),
            "*D" => Some(Self::ExpertMining),
            "i" => Some(Self::Washing),
            "f" => Some(Self::Fracturing),
            "H" => Some(Self::NanoPacking),
            "O" => Some(Self::BuildStructure),
            "A" => Some(Self::BuildRoad),
            "*B" => Some(Self::BuildUniversal),
            "*L" => Some(Self::BuildWar),
            "*A" => Some(Self::Architecture),
            "*T" => Some(Self::TotalDestruction),
            "*u" => Some(Self::UltraWhite),
            "*J" => Some(Self::Jewlery),
            "*I" => Some(Self::Induction),
            "*a" => Some(Self::MineSlime),
            "*d" => Some(Self::MineDeep),
            "*g" => Some(Self::GluonPacking),
            _ => None,
        }
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

pub trait OnDig {
    fn on_dig(&self, current_val: f32) -> f32;
}

pub trait OnDigCrys {
    fn on_dig_crys(&self, current_val: f32) -> f32;
}

pub trait OnMove {
    fn on_move(&self, current_val: f32) -> f32;
    fn on_move_road(&self, current_val: f32) -> f32;
}

pub trait OnBld {
    fn on_bld(&self, skill: SkillType, current_cost: f32) -> f32;
    fn on_bld_hp(&self, skill: SkillType, current_hp: f32) -> f32;
}

pub trait OnHealth {
    fn on_health_max(&self, current_max: i32) -> i32;
    fn on_health_regen(&self, current_regen: f32) -> f32;
}

pub trait OnHurt {
    fn on_hurt(&self, current_damage: f32) -> f32;
}

pub trait OnPackCrys {
    fn on_pack_crys_capacity(&self, current_capacity: i64) -> i64;
}

pub trait OnExp {
    fn on_exp(&self, current_val: f32) -> f32;
}

pub struct PlayerSkills<'a> {
    pub skills: &'a HashMap<String, SkillState>,
}

impl<'a> OnDig for PlayerSkills<'a> {
    fn on_dig(&self, _current_val: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::Digging)
    }
}

impl<'a> OnDigCrys for PlayerSkills<'a> {
    fn on_dig_crys(&self, _current_val: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::MineGeneral)
    }
}

impl<'a> OnMove for PlayerSkills<'a> {
    fn on_move(&self, _current_val: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::Movement)
    }
    fn on_move_road(&self, _current_val: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::RoadMovement)
    }
}

impl<'a> OnBld for PlayerSkills<'a> {
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
                .get(skill.code())
                .map_or(1.0, |s| s.level as f32),
            _ => 1.0,
        }
    }
}

impl<'a> OnHealth for PlayerSkills<'a> {
    fn on_health_max(&self, _current_max: i32) -> i32 {
        #[allow(clippy::cast_possible_truncation)]
        let val = get_player_skill_effect(self.skills, SkillType::Health) as i32;
        val
    }
    fn on_health_regen(&self, _current_regen: f32) -> f32 {
        get_player_skill_effect(self.skills, SkillType::Repair)
    }
}

impl<'a> OnHurt for PlayerSkills<'a> {
    fn on_hurt(&self, current_damage: f32) -> f32 {
        let anti_gun = get_player_skill_effect(self.skills, SkillType::AntiGun);
        current_damage * (1.0 - anti_gun / 100.0)
    }
}

impl<'a> OnPackCrys for PlayerSkills<'a> {
    fn on_pack_crys_capacity(&self, _current_capacity: i64) -> i64 {
        #[allow(clippy::cast_possible_truncation)]
        let val = get_player_skill_effect(self.skills, SkillType::Packing) as i64;
        val
    }
}

impl<'a> OnExp for PlayerSkills<'a> {
    fn on_exp(&self, current_val: f32) -> f32 {
        if let Some(upgr_state) = self.skills.get(SkillType::Upgrade.code()) {
            current_val * skill_effect(SkillType::Upgrade, upgr_state.level)
        } else {
            current_val
        }
    }
}

/// Calculate the gameplay effect value for a skill at a given level.
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::too_many_lines)]
pub fn skill_effect(skill: SkillType, level: i32) -> f32 {
    let x = level as f32;
    match skill {
        SkillType::Digging => x.mul_add(10.0, 100.0),
        SkillType::Movement => (500.0 - x * 5.0).max(200.0),
        SkillType::MineGeneral => {
            if x <= 0.0 {
                0.08
            } else {
                0.08 + x.log10() * x.sqrt() / 4.0
            }
        }
        SkillType::Health => x.mul_add(10.0, 100.0),
        SkillType::BuildRoad => x.mul_add(-0.2_f32, 5.0).max(1.0),
        SkillType::Induction => x.mul_add(0.2, 100.0),
        SkillType::Packing => x.mul_add(20.0, 100.0),
        SkillType::AntiGun => {
            if x <= 0.0 {
                1.0
            } else {
                let val = (1.0 + x - x.log10() * x.powf(0.9) / 2.0 - x * 0.098).round();
                val.min(92.0)
            }
        }
        SkillType::Fridge
        | SkillType::RoadMovement
        | SkillType::BuildGreen
        | SkillType::BuildYellow
        | SkillType::BuildRed
        | SkillType::BuildStructure
        | SkillType::BuildWar
        | SkillType::Repair => x,
        _ => x,
    }
}

/// Experience needed to level up from the current level.
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::missing_const_for_fn)]
pub fn exp_needed(_skill: SkillType, level: i32) -> f32 {
    100.0 * (level as f32)
}

/// Get the effect value for a player's skill, defaulting to level 0 if not present.
pub fn get_player_skill_effect(skills: &HashMap<String, SkillState>, skill: SkillType) -> f32 {
    skills
        .get(skill.code())
        .map_or_else(|| skill_effect(skill, 0), |s| skill_effect(skill, s.level))
}

/// Add exp to a skill. Returns true if the skill leveled up.
pub fn add_skill_exp(skills: &mut HashMap<String, SkillState>, code: &str, amount: f32) -> bool {
    let skill_type = SkillType::from_code(code);
    // Сначала читаем модификаторы из `skills` иммутабельно, чтобы дальше спокойно взять entry mut.
    let upgrade_mult = skills
        .get(SkillType::Upgrade.code())
        .map(|s| skill_effect(SkillType::Upgrade, s.level))
        .unwrap_or(1.0);
    let entry = skills
        .entry(code.to_string())
        .or_insert(SkillState { level: 1, exp: 0.0 });

    let total_amount = amount * upgrade_mult;

    entry.exp += total_amount;
    if let Some(st) = skill_type {
        let needed = exp_needed(st, entry.level);
        if needed > 0.0 && entry.exp >= needed {
            entry.exp -= needed;
            entry.level += 1;
            return true;
        }
    }
    false
}

/// Convert skills into outbound packets payload (`(skill_code, percent)`), preserving legacy rounding.
pub fn skill_progress_payload(skills: &HashMap<String, SkillState>) -> Vec<(String, i32)> {
    skills
        .iter()
        .map(|(code, s)| {
            let skill_type = SkillType::from_code(code);
            let needed = skill_type.map_or(1.0, |st| exp_needed(st, s.level));
            let pct = if needed > 0.0 {
                #[allow(clippy::cast_possible_truncation)]
                let ratio = (f64::from(s.exp) * 100.0 / f64::from(needed)).round() as i32;
                ratio.clamp(0, 100)
            } else {
                100
            };
            (code.clone(), pct)
        })
        .collect()
}

pub fn get_skill_requirements(skill: SkillType) -> Option<HashMap<SkillType, i32>> {
    let mut reqs = HashMap::new();
    match skill {
        SkillType::MineGeneral => {
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

        SkillType::BuildRoad => {
            reqs.insert(SkillType::Digging, 5);
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

pub fn can_install_skill(player_skills: &HashMap<String, SkillState>, skill: SkillType) -> bool {
    if player_skills.contains_key(skill.code()) {
        return false;
    }
    if let Some(reqs) = get_skill_requirements(skill) {
        for (req_skill, req_lvl) in reqs {
            if let Some(s) = player_skills.get(req_skill.code()) {
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
