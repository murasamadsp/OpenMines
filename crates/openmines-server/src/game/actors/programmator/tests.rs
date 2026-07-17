#[cfg(test)]
mod tests {
    use crate::db::SkillSlots;
    use crate::game::player::{
        PlayerConnection, PlayerMetadata, PlayerPosition, PlayerSettings, PlayerSkillsComp,
        PlayerStats,
    };
    use crate::game::{ProgrammatorAction, ProgrammatorQueue};
    use crate::world::WorldProvider;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::super::helpers::{
        ExecResult, WritableStateContext, execute_writable_state, speed_pause,
    };
    use super::super::system::{execute_action, next_programmator_deadline};
    use super::super::types::{
        ActionType, LastVariables, PAction, PFunction, ProgrammatorState, get_action_type,
    };

    #[test]
    fn program_step_without_delay_is_due_immediately() {
        let now = Instant::now();
        assert_eq!(next_programmator_deadline(now, None), now);
        assert_eq!(
            next_programmator_deadline(now, Some(Duration::from_millis(25))),
            now + Duration::from_millis(25)
        );
    }

    #[tokio::test]
    async fn due_system_requeues_program_after_no_delay_action() {
        let test = crate::test_support::ServerTestHarness::new(
            "programmator_due_requeue",
            "programmator-due-player",
        )
        .await;
        let (_tx, _rx) = test.connect_with_outbox(1);
        let player_id = crate::game::PlayerId(test.player.id);
        let due_at = Instant::now();
        let entity = test
            .state
            .modify_player(player_id, |ecs, entity| {
                let mut program = PFunction::new();
                program.actions.push(PAction {
                    action_type: ActionType::None,
                    label: String::new(),
                    num: 0,
                });
                let mut state = ecs.get_mut::<ProgrammatorState>(entity)?;
                state.running = true;
                state.current_prog.clear();
                state.current_prog.insert(String::new(), program);
                state.function_order = vec![String::new()];
                state.current_function.clear();
                state.delay = due_at;
                Some(entity)
            })
            .flatten()
            .expect("connected player has programmator state");
        test.state.schedule_programmator(entity, due_at);

        let due = test.state.take_due_programmators(Instant::now());
        assert_eq!(due, vec![(entity, due_at)]);
        let schedule = test
            .state
            .schedules
            .iter()
            .find(|schedule| schedule.name == "programmator")
            .expect("programmator schedule");
        let mut ecs = test.state.ecs.write();
        ecs.resource_mut::<crate::game::ProgrammatorDueBatch>().0 = due;
        schedule.schedule.write().run(&mut ecs);
        drop(ecs);

        assert!(test.state.next_programmator_due_at().is_some());
        let due = test.state.take_due_programmators(Instant::now());
        assert_eq!(due.len(), 1);
        let mut ecs = test.state.ecs.write();
        ecs.resource_mut::<crate::game::ProgrammatorDueBatch>().0 = due;
        schedule.schedule.write().run(&mut ecs);
        drop(ecs);

        assert!(test.state.next_programmator_due_at().is_some());
        assert!(
            test.state
                .query_player_opt(player_id, |ecs, entity| {
                    ecs.get::<ProgrammatorState>(entity)
                        .map(|state| state.running)
                })
                .unwrap_or(false)
        );
    }

    fn empty_skills() -> PlayerSkillsComp {
        PlayerSkillsComp {
            states: SkillSlots {
                skills: HashMap::new(),
                total_slots: 20,
            },
        }
    }

    fn test_world(name: &str) -> crate::world::World {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir: PathBuf =
            std::env::temp_dir().join(format!("openmines_programmator_{name}_{suffix}"));
        std::fs::create_dir_all(&dir).unwrap();
        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        crate::world::World::new(name, 2, 2, cell_defs, &dir).unwrap()
    }

    fn test_position() -> PlayerPosition {
        PlayerPosition {
            x: 10,
            y: 10,
            dir: 3,
        }
    }

    fn test_stats() -> PlayerStats {
        PlayerStats {
            health: 75,
            max_health: 100,
            money: 0,
            creds: 0,
            crystals: [1, 2, 3, 4, 5, 6],
            role: 0,
            skin: 0,
            clan_id: None,
            clan_rank: 0,
            last_bonus_at: 0,
        }
    }

    fn writable(label: &str, num: i32) -> PAction {
        PAction {
            action_type: ActionType::WritableState,
            label: label.to_string(),
            num,
        }
    }

    fn label_action(action_type: ActionType, label: &str) -> PAction {
        PAction {
            action_type,
            label: label.to_string(),
            num: 0,
        }
    }

    fn clear_macro_mine_neighbors(world: &crate::world::World, pos: &PlayerPosition) {
        for (dx, dy) in [(0, 1), (-1, 0), (0, -1), (1, 0)] {
            world.set_cell_typed(
                pos.x + dx,
                pos.y + dy,
                crate::world::CellType(crate::world::cells::cell_type::EMPTY),
            );
        }
    }

    fn test_metadata() -> PlayerMetadata {
        PlayerMetadata {
            id: crate::game::player::PlayerId(1),
            name: "test".to_string(),
            passwd: String::new(),
            hash: String::new(),
            resp_x: None,
            resp_y: None,
        }
    }

    #[test]
    fn speed_pause_road_bonus_is_faster() {
        // C# Player.cs:155: на дороге ServerPause ×0.80 → меньше пауза.
        // Movement lvl0 effect=70 → pause_units=7000; off=49ms, on=39ms.
        let skills = empty_skills();
        let timing = crate::config::ProgrammatorConfig::runtime_baseline();
        let off = speed_pause(&skills, false, timing);
        let on = speed_pause(&skills, true, timing);
        assert_eq!(off, 49);
        assert_eq!(on, 39);
        assert!(on < off, "on-road должно быть быстрее off-road");
    }

    #[test]
    fn programmator_move_delay_is_skill_based_even_when_target_is_blocked() {
        let world = test_world("move_delay_no_block_penalty");
        let pos = test_position();
        world.set_cell_typed(
            pos.x + 1,
            pos.y,
            crate::world::CellType(crate::world::cells::cell_type::GREEN),
        );
        let stats = test_stats();
        let skills = empty_skills();
        let settings = PlayerSettings::default();
        let meta = test_metadata();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        let mut prog_q = ProgrammatorQueue(Vec::new());
        let mut delay = None;
        let timing = crate::config::ProgrammatorConfig::runtime_baseline();

        let result = execute_action(
            &label_action(ActionType::MoveRight, ""),
            &mut prog,
            &pos,
            &stats,
            &skills,
            &settings,
            &world,
            &meta,
            None,
            &mut prog_q,
            &mut delay,
            0,
            timing,
        );

        assert!(matches!(result, ExecResult::None));
        assert_eq!(
            delay,
            Some(Duration::from_millis(speed_pause(&skills, false, timing)))
        );
        assert!(matches!(
            prog_q.0.as_slice(),
            [ProgrammatorAction::Move { pid, session_id: None, x, y, dir }]
                if *pid == meta.id && *x == pos.x + 1 && *y == pos.y && *dir == -1
        ));
    }

    #[test]
    fn decode_prog_packet_rejects_truncated_compiled_block() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&10_i32.to_le_bytes());
        payload.extend_from_slice(&42_i32.to_le_bytes());
        payload.extend_from_slice(&[1, 2, 3]);

        assert!(ProgrammatorState::decode_prog_packet(&payload).is_none());
    }

    #[test]
    fn decode_prog_packet_accepts_empty_source_when_compiled_block_is_complete() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&3_i32.to_le_bytes());
        payload.extend_from_slice(&42_i32.to_le_bytes());
        payload.extend_from_slice(&[1, 2, 3]);

        assert_eq!(
            ProgrammatorState::decode_prog_packet(&payload),
            Some((42, String::new()))
        );
    }

    #[test]
    fn parse_text_format_maps_basic_programmator_actions() {
        let (functions, order) = ProgrammatorState::parse_text("$zghAGR+AGR-^W").unwrap();
        assert_eq!(order, vec![String::new()]);
        let actions: Vec<ActionType> = functions[""]
            .actions
            .iter()
            .map(|a| a.action_type)
            .collect();

        assert_eq!(
            actions,
            vec![
                ActionType::Dig,
                ActionType::Geology,
                ActionType::Heal,
                ActionType::EnableAgression,
                ActionType::DisableAgression,
                ActionType::MoveUp
            ]
        );
    }

    #[test]
    fn parse_text_format_maps_start_and_end_symbols_in_unity_order() {
        let (functions, _) = ProgrammatorState::parse_text("$#S#E").unwrap();
        let actions: Vec<ActionType> = functions[""]
            .actions
            .iter()
            .map(|a| a.action_type)
            .collect();

        assert_eq!(actions, vec![ActionType::Start, ActionType::Stop]);
    }

    #[test]
    fn parse_text_format_maps_writable_variables() {
        let (functions, _) = ProgrammatorState::parse_text("$(foo=7)(foo>3)(foo<9)").unwrap();
        let actions = &functions[""].actions;

        assert_eq!(actions[0].action_type, ActionType::WritableState);
        assert_eq!(actions[0].label, "foo");
        assert_eq!(actions[0].num, 7);
        assert_eq!(actions[1].action_type, ActionType::WritableStateMore);
        assert_eq!(actions[1].label, "foo");
        assert_eq!(actions[1].num, 3);
        assert_eq!(actions[2].action_type, ActionType::WritableStateLower);
        assert_eq!(actions[2].label, "foo");
        assert_eq!(actions[2].num, 9);
    }

    #[test]
    fn programmator_direct_action_delay_comes_from_config() {
        let timing = crate::config::ProgrammatorConfig {
            direct_action_delay_us: 123_456,
            ..crate::config::ProgrammatorConfig::runtime_baseline()
        };
        assert_eq!(
            super::super::system::direct_action_delay(timing),
            Duration::from_micros(123_456)
        );
    }

    #[test]
    fn unity_hand_mode_bytecodes_map_to_hand_mode_actions() {
        assert_eq!(get_action_type(179), ActionType::HandModeOn);
        assert_eq!(get_action_type(180), ActionType::HandModeOff);
        assert_eq!(get_action_type(162), ActionType::BuildBlock);
        assert_eq!(get_action_type(163), ActionType::BuildPillar);
        assert_eq!(get_action_type(164), ActionType::BuildRoad);
        assert_eq!(get_action_type(165), ActionType::BuildMilitaryBlock);
    }

    #[test]
    fn unity_programmator_extension_bytecodes_are_named() {
        assert_eq!(get_action_type(167), ActionType::OnlineGeo);
        assert_eq!(get_action_type(168), ActionType::OnlineZz);
        assert_eq!(get_action_type(169), ActionType::OnlineC190);
        assert_eq!(get_action_type(170), ActionType::OnlinePoly);
        assert_eq!(get_action_type(171), ActionType::OnlineUp);
        assert_eq!(get_action_type(172), ActionType::OnlineCraft);
        assert_eq!(get_action_type(173), ActionType::OnlineNano);
        assert_eq!(get_action_type(174), ActionType::OnlineRem);
        assert_eq!(get_action_type(175), ActionType::InventoryUp);
        assert_eq!(get_action_type(176), ActionType::InventoryLeft);
        assert_eq!(get_action_type(177), ActionType::InventoryDown);
        assert_eq!(get_action_type(178), ActionType::InventoryRight);
        assert_eq!(get_action_type(181), ActionType::DebugMessage);
        assert_eq!(get_action_type(182), ActionType::DebugPause);
        assert_eq!(get_action_type(200), ActionType::RestartRow);
    }

    #[test]
    fn parse_text_format_maps_all_current_unity_extension_tokens() {
        let (functions, _) = ProgrammatorState::parse_text(
            "$B1;B2;B3;VB;GEO;ZZ;C190;POLY;UP;CRAFT;NANO;REM;iwiaisidHand+Hand-!{dbg}{pause}RESTART;",
        )
        .unwrap();
        let actions: Vec<ActionType> = functions[""]
            .actions
            .iter()
            .map(|a| a.action_type)
            .collect();

        assert_eq!(
            actions,
            vec![
                ActionType::BuildBlock,
                ActionType::BuildPillar,
                ActionType::BuildRoad,
                ActionType::BuildMilitaryBlock,
                ActionType::OnlineGeo,
                ActionType::OnlineZz,
                ActionType::OnlineC190,
                ActionType::OnlinePoly,
                ActionType::OnlineUp,
                ActionType::OnlineCraft,
                ActionType::OnlineNano,
                ActionType::OnlineRem,
                ActionType::InventoryUp,
                ActionType::InventoryLeft,
                ActionType::InventoryDown,
                ActionType::InventoryRight,
                ActionType::HandModeOn,
                ActionType::HandModeOff,
                ActionType::DebugMessage,
                ActionType::DebugPause,
                ActionType::RestartRow,
            ]
        );

        assert_eq!(functions[""].actions[18].label, "dbg");
        assert_eq!(functions[""].actions[19].label, "pause");
    }

    #[test]
    fn run_program_accepts_current_unity_text_format() {
        let mut state = ProgrammatorState::new();

        assert!(state.run_program("$z"));
        assert!(state.running);
        assert_eq!(
            state.current_prog[""].actions[0].action_type,
            ActionType::Dig
        );
    }

    #[test]
    fn writable_state_creates_and_compares_user_variables() {
        let world = test_world("vars_user");
        let pos = test_position();
        let stats = test_stats();
        let settings = PlayerSettings::default();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        let mut delay = None;

        let result = execute_writable_state(
            &writable("foo", 7),
            &mut prog,
            WritableStateContext {
                pos: &pos,
                stats: &stats,
                settings: &settings,
                world: &world,
                geo_count: 0,
            },
            &mut delay,
        );
        assert!(matches!(result, ExecResult::BoolResult(true)));
        assert_eq!(prog.user_variables["foo"], 7);
        assert_eq!(prog.current_prog[""].state, Some(true));

        let more = PAction {
            action_type: ActionType::WritableStateMore,
            label: "foo".to_string(),
            num: 3,
        };
        let result = execute_writable_state(
            &more,
            &mut prog,
            WritableStateContext {
                pos: &pos,
                stats: &stats,
                settings: &settings,
                world: &world,
                geo_count: 0,
            },
            &mut delay,
        );
        assert!(matches!(result, ExecResult::BoolResult(true)));
        assert_eq!(prog.current_prog[""].state, Some(true));
    }

    #[test]
    fn writable_state_commands_mutate_last_user_variable() {
        let world = test_world("vars_commands");
        let pos = test_position();
        let stats = test_stats();
        let settings = PlayerSettings::default();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        let mut delay = None;

        let _ = execute_writable_state(
            &writable("foo", 7),
            &mut prog,
            WritableStateContext {
                pos: &pos,
                stats: &stats,
                settings: &settings,
                world: &world,
                geo_count: 0,
            },
            &mut delay,
        );
        let result = execute_writable_state(
            &writable("ADD", 5),
            &mut prog,
            WritableStateContext {
                pos: &pos,
                stats: &stats,
                settings: &settings,
                world: &world,
                geo_count: 0,
            },
            &mut delay,
        );

        assert!(matches!(result, ExecResult::None));
        assert_eq!(prog.user_variables["foo"], 12);
    }

    #[test]
    fn writable_state_last_variables_match_js_reference_order() {
        let mut vars = LastVariables::default();

        vars.set("foo");
        assert_eq!(vars.younger(), Some("foo"));
        assert_eq!(vars.older(), None);

        vars.set("foo");
        assert_eq!(vars.younger(), Some("foo"));
        assert_eq!(vars.older(), Some("foo"));

        vars.set("bar");
        assert_eq!(vars.younger(), Some("bar"));
        assert_eq!(vars.older(), Some("foo"));
    }

    #[test]
    fn writable_state_two_variable_commands_can_read_readonly_younger() {
        let world = test_world("vars_commands_readonly_younger");
        let pos = test_position();
        let stats = test_stats();
        let settings = PlayerSettings::default();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        prog.user_variables.insert("foo".to_string(), 10);
        prog.last_variables.set("foo");
        prog.last_variables.set("X");
        let mut delay = None;

        let result = execute_writable_state(
            &writable("SU2", 0),
            &mut prog,
            WritableStateContext {
                pos: &pos,
                stats: &stats,
                settings: &settings,
                world: &world,
                geo_count: 0,
            },
            &mut delay,
        );

        assert!(matches!(result, ExecResult::None));
        assert_eq!(prog.user_variables["foo"], 0);
    }

    #[test]
    fn writable_state_division_uses_js_zero_only_fallback() {
        let world = test_world("vars_commands_negative_divisor");
        let pos = test_position();
        let stats = test_stats();
        let settings = PlayerSettings::default();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        prog.user_variables.insert("foo".to_string(), 9);
        prog.last_variables.set("foo");
        let mut delay = None;

        let result = execute_writable_state(
            &writable("DIV", -3),
            &mut prog,
            WritableStateContext {
                pos: &pos,
                stats: &stats,
                settings: &settings,
                world: &world,
                geo_count: 0,
            },
            &mut delay,
        );

        assert!(matches!(result, ExecResult::None));
        assert_eq!(prog.user_variables["foo"], -3);
    }

    #[test]
    fn writable_state_reads_readonly_values_and_selected_cell() {
        let world = test_world("vars_readonly");
        world.set_cell(11, 10, crate::world::cells::cell_type::GREEN);
        let pos = test_position();
        let mut stats = test_stats();
        stats.health = 25;
        let settings = PlayerSettings {
            auto_dig: true,
            aggression: true,
            ..Default::default()
        };
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        prog.hand_mode_active = true;
        prog.check_x = 1;
        prog.started_at = Instant::now()
            .checked_sub(Duration::from_secs(3))
            .expect("test duration is smaller than monotonic clock range");
        prog.current_prog.get_mut("").unwrap().last_state_action = Some(ActionType::Or);
        let mut delay = None;

        for action in [
            writable("AUT", 1),
            writable("AGR", 1),
            writable("HND", 1),
            writable("DBG", 0),
            writable("STK", 0),
            writable("DIR", 1),
            writable("X", 10),
            writable("Y", 10),
            writable("HP", 25),
            writable("HPP", 25),
            writable("TIM", 3),
            writable("G", 1),
            writable("GEO", 2),
            writable("BOO", 2),
            writable("CEL", i32::from(crate::world::cells::cell_type::GREEN)),
        ] {
            let geo_count = if action.label == "GEO" { 2 } else { 0 };
            let result = execute_writable_state(
                &action,
                &mut prog,
                WritableStateContext {
                    pos: &pos,
                    stats: &stats,
                    settings: &settings,
                    world: &world,
                    geo_count,
                },
                &mut delay,
            );
            assert!(
                matches!(result, ExecResult::BoolResult(true)),
                "failed readonly {}",
                action.label
            );
        }
        assert_eq!(
            (prog.check_x, prog.check_y, prog.shift_x, prog.shift_y),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn run_if_preserves_state_on_no_jump_like_js_reference() {
        let world = test_world("run_if_state_preserve");
        let pos = test_position();
        let stats = test_stats();
        let skills = empty_skills();
        let settings = PlayerSettings::default();
        let meta = test_metadata();
        let conn = PlayerConnection {
            session_id: crate::game::SessionId::new(1),
        };
        let mut prog_q = ProgrammatorQueue(Vec::new());
        let mut delay = None;
        let mut prog = ProgrammatorState::new();
        let mut function = PFunction::new();
        function.state = Some(false);
        prog.current_prog.insert(String::new(), function);

        let result = execute_action(
            &label_action(ActionType::RunIfTrue, "next"),
            &mut prog,
            &pos,
            &stats,
            &skills,
            &settings,
            &world,
            &meta,
            Some(&conn),
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );

        assert!(matches!(result, ExecResult::None));
        assert_eq!(prog.current_prog[""].state, Some(false));

        prog.current_prog.get_mut("").unwrap().state = Some(true);
        let result = execute_action(
            &label_action(ActionType::RunIfFalse, "next"),
            &mut prog,
            &pos,
            &stats,
            &skills,
            &settings,
            &world,
            &meta,
            Some(&conn),
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );

        assert!(matches!(result, ExecResult::None));
        assert_eq!(prog.current_prog[""].state, Some(true));
    }

    #[test]
    fn run_if_clears_state_only_on_jump_like_js_reference() {
        let world = test_world("run_if_state_clear");
        let pos = test_position();
        let stats = test_stats();
        let skills = empty_skills();
        let settings = PlayerSettings::default();
        let meta = test_metadata();
        let conn = PlayerConnection {
            session_id: crate::game::SessionId::new(1),
        };
        let mut prog_q = ProgrammatorQueue(Vec::new());
        let mut delay = None;
        let mut prog = ProgrammatorState::new();
        let mut function = PFunction::new();
        function.state = Some(true);
        prog.current_prog.insert(String::new(), function);

        let result = execute_action(
            &label_action(ActionType::RunIfTrue, "next"),
            &mut prog,
            &pos,
            &stats,
            &skills,
            &settings,
            &world,
            &meta,
            Some(&conn),
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );

        assert!(matches!(result, ExecResult::Label(label) if label == "next"));
        assert_eq!(prog.current_prog[""].state, None);
    }

    #[test]
    fn macros_heal_requires_red_crystal_like_reference() {
        let world = test_world("macros_heal_red_guard");
        let pos = test_position();
        let skills = empty_skills();
        let settings = PlayerSettings::default();
        let meta = test_metadata();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        let action = label_action(ActionType::MacrosHeal, "");

        let mut no_red_stats = test_stats();
        no_red_stats.health = 50;
        no_red_stats.crystals[2] = 0;
        let mut prog_q = ProgrammatorQueue(Vec::new());
        let mut delay = None;
        let result = execute_action(
            &action,
            &mut prog,
            &pos,
            &no_red_stats,
            &skills,
            &settings,
            &world,
            &meta,
            None,
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );
        assert!(matches!(result, ExecResult::None));
        assert!(prog_q.0.is_empty());
        assert!(delay.is_none());

        let mut red_stats = no_red_stats;
        red_stats.crystals[2] = 1;
        let result = execute_action(
            &action,
            &mut prog,
            &pos,
            &red_stats,
            &skills,
            &settings,
            &world,
            &meta,
            None,
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );
        assert!(matches!(result, ExecResult::BoolResult(true)));
        assert!(matches!(
            prog_q.0.as_slice(),
            [ProgrammatorAction::Heal { pid, session_id: None }] if *pid == meta.id
        ));
        assert!(delay.is_some());
    }

    #[test]
    fn macros_mine_rotates_toward_adjacent_crystal_like_reference() {
        let world = test_world("macros_mine_rotate");
        let pos = test_position();
        clear_macro_mine_neighbors(&world, &pos);
        world.set_cell_typed(
            pos.x - 1,
            pos.y,
            crate::world::CellType(crate::world::cells::cell_type::GREEN),
        );
        let stats = test_stats();
        let skills = empty_skills();
        let settings = PlayerSettings::default();
        let meta = test_metadata();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        let mut prog_q = ProgrammatorQueue(Vec::new());
        let mut delay = None;

        let result = execute_action(
            &label_action(ActionType::MacrosMine, ""),
            &mut prog,
            &pos,
            &stats,
            &skills,
            &settings,
            &world,
            &meta,
            None,
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );

        assert!(matches!(result, ExecResult::BoolResult(true)));
        assert_eq!(prog.macros_template, None);
        assert!(delay.is_some());
        match prog_q.0.as_slice() {
            [ProgrammatorAction::Move { x, y, dir, .. }] => {
                assert_eq!((*x, *y, *dir), (pos.x, pos.y, 1));
            }
            _ => panic!("expected queued rotate/move action"),
        }
    }

    #[test]
    fn macros_mine_digs_and_remembers_direction_like_reference() {
        let world = test_world("macros_mine_dig");
        let pos = test_position();
        clear_macro_mine_neighbors(&world, &pos);
        world.set_cell_typed(
            pos.x + 1,
            pos.y,
            crate::world::CellType(crate::world::cells::cell_type::GREEN),
        );
        let stats = test_stats();
        let skills = empty_skills();
        let settings = PlayerSettings::default();
        let meta = test_metadata();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        let mut prog_q = ProgrammatorQueue(Vec::new());
        let mut delay = None;

        let result = execute_action(
            &label_action(ActionType::MacrosMine, ""),
            &mut prog,
            &pos,
            &stats,
            &skills,
            &settings,
            &world,
            &meta,
            None,
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );

        assert!(matches!(result, ExecResult::BoolResult(true)));
        assert_eq!(prog.macros_template, Some(pos.dir));
        assert!(delay.is_some());
        match prog_q.0.as_slice() {
            [ProgrammatorAction::Dig { dir, .. }] => assert_eq!(*dir, pos.dir),
            _ => panic!("expected queued Dig action"),
        }
    }

    #[test]
    fn macros_mine_fast_path_uses_template_and_clears_when_no_crystal() {
        let world = test_world("macros_mine_template");
        let pos = test_position();
        clear_macro_mine_neighbors(&world, &pos);
        world.set_cell_typed(
            pos.x + 1,
            pos.y,
            crate::world::CellType(crate::world::cells::cell_type::GREEN),
        );
        let stats = test_stats();
        let skills = empty_skills();
        let settings = PlayerSettings::default();
        let meta = test_metadata();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        prog.macros_template = Some(pos.dir);
        let mut prog_q = ProgrammatorQueue(Vec::new());
        let mut delay = None;

        let result = execute_action(
            &label_action(ActionType::MacrosMine, ""),
            &mut prog,
            &pos,
            &stats,
            &skills,
            &settings,
            &world,
            &meta,
            None,
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );

        assert!(matches!(result, ExecResult::BoolResult(true)));
        assert_eq!(prog.macros_template, Some(pos.dir));
        assert!(matches!(
            prog_q.0.as_slice(),
            [ProgrammatorAction::Dig { .. }]
        ));

        clear_macro_mine_neighbors(&world, &pos);
        prog_q.0.clear();
        delay = None;
        let result = execute_action(
            &label_action(ActionType::MacrosMine, ""),
            &mut prog,
            &pos,
            &stats,
            &skills,
            &settings,
            &world,
            &meta,
            None,
            &mut prog_q,
            &mut delay,
            0,
            crate::config::ProgrammatorConfig::runtime_baseline(),
        );

        assert!(matches!(result, ExecResult::None));
        assert_eq!(prog.macros_template, None);
        assert!(prog_q.0.is_empty());
        assert!(delay.is_none());
    }

    #[test]
    fn writable_state_delay_does_not_update_condition_state() {
        let world = test_world("vars_delay");
        let pos = test_position();
        let stats = test_stats();
        let settings = PlayerSettings::default();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        let mut delay = None;

        let result = execute_writable_state(
            &writable("del", 77),
            &mut prog,
            WritableStateContext {
                pos: &pos,
                stats: &stats,
                settings: &settings,
                world: &world,
                geo_count: 0,
            },
            &mut delay,
        );

        assert!(matches!(result, ExecResult::None));
        assert_eq!(delay, Some(Duration::from_millis(77)));
        assert_eq!(prog.current_prog[""].state, None);
    }

    #[test]
    fn writable_state_geo_label_compares_geo_count_case_insensitive() {
        let world = test_world("vars_geo");
        let pos = test_position();
        let stats = test_stats();
        let settings = PlayerSettings::default();
        let mut prog = ProgrammatorState::new();
        prog.current_prog.insert(String::new(), PFunction::new());
        let mut delay = None;

        for action in [
            PAction {
                action_type: ActionType::WritableState,
                label: "geo".to_string(),
                num: 3,
            },
            PAction {
                action_type: ActionType::WritableStateLower,
                label: "geo".to_string(),
                num: 4,
            },
            PAction {
                action_type: ActionType::WritableStateMore,
                label: "geo".to_string(),
                num: 2,
            },
        ] {
            let result = execute_writable_state(
                &action,
                &mut prog,
                WritableStateContext {
                    pos: &pos,
                    stats: &stats,
                    settings: &settings,
                    world: &world,
                    geo_count: 3,
                },
                &mut delay,
            );

            assert!(matches!(result, ExecResult::BoolResult(true)));
            assert_eq!(prog.current_prog[""].state, Some(true));
        }
    }

    #[test]
    fn programmator_snapshot_roundtrips_runtime_state() {
        let mut state = ProgrammatorState::new();
        assert!(state.run_program("$zg"));
        state.current_prog.get_mut("").unwrap().current = 1;
        state.shift_x = 2;
        state.check_y = -1;
        state.hand_mode_active = true;
        state.selected_id = Some(7);
        state.selected_data = Some("$zg".to_string());
        state.user_variables.insert("foo".to_string(), 12);
        state.last_variables.set("foo");

        let encoded = serde_json::to_string(&state.snapshot()).unwrap();
        let snapshot = serde_json::from_str(&encoded).unwrap();
        let mut restored = ProgrammatorState::new();
        restored.restore_snapshot(snapshot);

        assert!(restored.running);
        assert_eq!(restored.current_prog[""].current, 1);
        assert_eq!(
            restored.current_prog[""].actions[1].action_type,
            ActionType::Geology
        );
        assert_eq!(restored.shift_x, 2);
        assert_eq!(restored.check_y, -1);
        assert!(restored.hand_mode_active);
        assert_eq!(restored.selected_id, Some(7));
        assert_eq!(restored.selected_data.as_deref(), Some("$zg"));
        assert_eq!(restored.user_variables["foo"], 12);
        assert_eq!(restored.last_variables.younger(), Some("foo"));
    }

    #[test]
    fn invalid_program_source_stops_previous_run() {
        let mut state = ProgrammatorState::new();
        state.running = true;
        state
            .current_prog
            .insert("stale".to_string(), PFunction::new());
        state.function_order.push("stale".to_string());
        state.current_function = "stale".to_string();

        assert!(!state.run_program("not valid base64/lzma"));
        assert!(!state.running);
        assert!(state.current_prog.is_empty());
        assert!(state.function_order.is_empty());
        assert!(state.current_function.is_empty());
    }
}
