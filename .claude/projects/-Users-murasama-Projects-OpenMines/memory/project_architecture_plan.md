---
name: architecture-rewrite-plan
description: Complete post-parity architecture rewrite plan — 4 layers, 3 threads, 0 shared mutable state, 8 migration stages
type: project
---

After achieving 1:1 C# reference parity, rewrite architecture in 8 stages.

**Target:** 4 layers (Network, Game, Persistence, Protocol), 3 threads (N session tasks, 1 game thread, 1 persistence task), 0 shared mutable state.

**Why:** Current GameState god-object with DashMaps + ECS behind RwLock creates global bottleneck, deadlock risk, untestable coupling. C# reference also uses global singletons but gets away with it because physics runs single-threaded — we should formalize this.

**Key decisions:**
- Game thread is std::thread (not tokio) — CPU-bound, needs precise 50ms tick
- Sessions are pure codec actors — decode wire → PlayerCommand, GameEvent → encode wire
- PlayerCommand enum replaces TY dispatch (typed, exhaustive)
- GameEvent enum replaces BroadcastQueue + direct send_u_packet
- EventBuffer ECS resource replaces BroadcastQueue/ProgrammatorQueue
- No DashMap, no RwLock, no Arc on game state
- Persistence layer batches writes, game thread never waits for DB
- 20 ticks/sec (50ms) — movement/dig cooldown is 333ms so 50ms latency invisible

**Migration stages (each produces working server):**
0. Extract pure logic functions (formulas, validation, parsers) — 0 risk
1. Define PlayerCommand + GameEvent enums — 0 risk
2. Game thread receives commands via channel, still uses old handlers — medium risk
3. Migrate DashMap → ECS Resources one by one — high risk
4. Kill Arc<GameState>, game thread owns ECS directly — point of no return
5. Persistence layer as separate task with batch writes — low risk
6. EventBuffer replaces direct packet sends — medium risk
7. Tests: game logic without network, integration tests with mock sessions — 0 risk

**How to apply:** Start only after ROADMAP parity is complete. Each stage = 1-3 sessions, total ~14 sessions. Never do 2 stages at once.
