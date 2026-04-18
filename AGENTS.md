<!-- Generated: 2026-04-18 | Updated: 2026-04-18 -->

# OpenMines: Agent Context & Guidelines

## Project Overview

**OpenMines** is a high-performance game server written in Rust, designed to support a legacy Unity client. It features a sandbox world, real-time binary protocol interaction, and a modern web-based monitoring stack.

### Core Technologies

- **Language:** Rust (Edition 2024)
- **Runtime:** Tokio (Asynchronous I/O)
- **Networking:** Custom Binary TCP Protocol + Axum (HTTP API)
- **Database:** SQLite (via Rusqlite with WAL mode enabled)
- **World Management:** Memory-mapped file layers (`.mapb`) using `memmap2` for zero-copy access.
- **Game Engine:** Bevy ECS (Entity Component System).

## Key Files & Directories

| File/Path | Description |
| - | - |
| `server/` | Core Rust server logic and protocols |
| `client/` | Unity C# client and assets |
| `docs/` | Technical documentation (API, ARCHITECTURE, PROTOCOL) |
| `scripts/` | Quality assurance, build, and deployment scripts |
| `Cargo.toml` | Rust workspace and dependency configuration |
| `config.json` | Server runtime configuration (ports, logging, world size) |
| `cells.json` | Definitions of world cell types (durability, flags) |
| `openmines.db` | Main SQLite database (players, clans, buildings) |
| `*.mapb` | Binary world layers (Cells, Road, Durability) |

## Architecture

- **World Management:** Universal typed layers with atomic backups (`.bak`) and dirty chunk tracking.
- **Network:** Wire frame format: `[4B length][1B type][2B event][payload]`. Numeric values are **Little-Endian**.
- **Cron System:** Background job scheduler for recurring tasks (maintenance, logs, state flushes).
- **State:** Shared state via `Arc<GameState>` injected into handlers; avoid global mutability.

## Building and Running

- **Build:** `cargo build --release`
- **Run:** `cargo run --release`
- **Test:** `cargo test --all-targets --all-features`
- **Lint (Strict):** `cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery`
- **Format:** `cargo fmt --all`
- **Regenerate World:** `cargo run -- --regen` or set `M3R_REGEN_WORLD=1`.

## Development Conventions

- **Strict Quality:** Code MUST pass strict Clippy and rustfmt checks before committing.
- **Data Integrity:** World modifications must be followed by `world.flush()`.
- **Environment:** Use `M3R_DATA_DIR` to override the state directory (default: `data/`).
- **Checkpointing:** Use Entire CLI for history tracking (`entire status`).

## Mandatory Constraints for AI Agents

- **NO DELETION REVERSAL:** If a file or piece of code was explicitly deleted/removed, DO NOT restore it unless specifically asked.
- **NO LINTER TAMPERING:** Do not modify linter settings or suppress warnings unless instructed.
- **NO BYPASS:** Never use `--no-verify` for any operations.
- **LOCALITY:** Modify Rust/C# code only within their respective directory trees.
- **CLEANUP:** Do not touch `.omc`, `target`, `bin`, or `obj` directories.

---

### Manual Notes

ЕСЛИ БЛЯТЬ ЧТО-ТО УДАЛЕНО, НЕ НАДО ЕГО ВОЗВРАЩАТЬ!!!!!!!
Я УЖЕ ЗАЕБАЛСЯ ТО Я УДАЛЮ, ТО ТЫ ВЕРНЕШЬ, ТО Я УДАЛЮ И ТАК БЕСКОНЕЧНО
И ЗАПРЕЩЕНО МЕНЯТЬ ЛИНТЕРЫ И Т.П.
Старайся работать только через специализированных агентов.
