use crate::game::logic::numeric::saturating_trunc_f32_to_i32;
use crate::game::skills::OnHealth;
use crate::game::{GameState, PlayerId};
use std::sync::Arc;

#[derive(Debug, Eq, PartialEq)]
pub enum HealResult {
    Applied {
        health: i32,
        max_health: i32,
        crystals: [i64; 6],
        x: i32,
        y: i32,
        skill_packet: Option<(&'static str, Vec<u8>)>,
    },
    SilentNoop,
    MissingState(&'static str),
    MissingEntity,
}

pub fn apply_heal(state: &Arc<GameState>, pid: PlayerId, programmatic: bool) -> HealResult {
    let ctx = crate::game::ExpContext::from_state(state);
    state
        .modify_player(pid, |ecs, entity| {
            let Some(prog) = ecs.get::<crate::game::programmator::ProgrammatorState>(entity) else {
                tracing::error!(player_id = %pid, component = "ProgrammatorState", "Player component missing for heal");
                return Some(HealResult::MissingState("ProgrammatorState"));
            };
            if !programmatic && !prog.is_manual_control_allowed() {
                return Some(HealResult::SilentNoop);
            }

            let Some(player_stats) = ecs.get::<crate::game::player::PlayerStats>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for heal");
                return Some(HealResult::MissingState("PlayerStats"));
            };
            let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerPosition", "Player component missing for heal");
                return Some(HealResult::MissingState("PlayerPosition"));
            };
            let Some(skills) = ecs.get::<crate::game::player::PlayerSkillsComp>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing for heal");
                return Some(HealResult::MissingState("PlayerSkillsComp"));
            };

            let heal_amount = saturating_trunc_f32_to_i32(crate::game::skills::PlayerSkills {
                skills: &skills.states,
            }
            .on_health_regen(0.0));

            if heal_amount <= 0
                || player_stats.health >= player_stats.max_health
                || player_stats.crystals
                    [crate::game::logic::crystals::CrystalKind::Red.index()]
                    < 1
            {
                return Some(HealResult::SilentNoop);
            }

            let max_health = player_stats.max_health;
            let x = pos.x;
            let y = pos.y;
            let (health, crystals) = {
                let mut changed_stats = ecs
                    .get_mut::<crate::game::player::PlayerStats>(entity)
                    .expect("PlayerStats checked before heal mutation");
                changed_stats.crystals
                    [crate::game::logic::crystals::CrystalKind::Red.index()] -= 1;
                changed_stats.health = changed_stats
                    .health
                    .saturating_add(heal_amount)
                    .min(max_health);
                (changed_stats.health, changed_stats.crystals)
            };

            let mut skills = ecs
                .get_mut::<crate::game::player::PlayerSkillsComp>(entity)
                .expect("PlayerSkillsComp checked before heal skill exp");
            let skill_packet = ctx.add_skill_exp(&mut skills.states, "e", 1.0);

            Some(HealResult::Applied {
                health,
                max_health,
                crystals,
                x,
                y,
                skill_packet,
            })
        })
        .flatten()
        .unwrap_or(HealResult::MissingEntity)
}
