# Rust / Dependencies
- Prefer adding dependencies (strum, clap, indexmap, dotnet-rng, etc.) to reduce boilerplate — server is "hellishly low-level" and dependencies are pragmatically safe. Confidence: 0.80

# Workflow
- Drive momentum autonomously: don't pause for confirmation between steps; user verifies via live client play-test. Confidence: 0.75
- Commit all related files together; leave nothing uncommitted in the working tree. Confidence: 0.85

# Reference Porting
- Full 1:1 C#→Rust parity is the goal; audit everything, fix everything. Deviations only for explicit user directive or clear C# bugs. Confidence: 0.80
- Wire protocol is immutable — never touch packet names, formats, or byte-level semantics. Confidence: 0.90

# Taste (Continuously Learned by [CommandCode][cmd])

[cmd]: https://commandcode.ai/

# code-style
- When deviating from C# reference by user order, mark with "НАМЕРЕННАЯ ДЕВИАЦИЯ от C# по ПРЯМОМУ ТРЕБОВАНИЮ ПОЛЬЗОВАТЕЛЯ" comment explaining the deviation. Confidence: 0.70

# workflow
- Prefer adding dependencies (crates) to reduce boilerplate and low-level code; dismiss theoretical dependency risks — user considers them negligible in practice. Confidence: 0.80
- When user says "далее" or "работай", stop deliberating and execute immediately — user prioritizes momentum over caution. Confidence: 0.80

# workflow
- Работай автономно до конца; не жди подтверждения на каждый шаг. Если сказано "далее" — продолжай без переспроса. Confidence: 0.80
- Юзер верифицирует изменения через живой клиент (заходит в игру), а не через автоматические тесты. Тесты — только страховка, не авторитетный гейт. Confidence: 0.75
- Отвечай кратко, по-русски, без излишней осторожности и длинных преамбул. Если юзер говорит "даун" или "не мели" — ты слишком осторожничаешь. Confidence: 0.75

# porting
- Протокол (wire-пакеты) НЕИЗМЕНЯЕМ. Никогда не меняй имена пакетов (str3xx), форматы payload, байтовую семантику — сломает клиент. Confidence: 0.95

# workflow
See [workflow/taste.md](workflow/taste.md)

# client
- Never modify client networking code (packet send/receive/parse) — it's absolutely forbidden and breaks the client-server contract. Confidence: 0.95
- Client changes must be surgical and 100% proven necessary — targeted single-purpose fixes only, no refactoring or restructuring. Confidence: 0.85

# debugging
- For persistent bugs, use systematic elimination method: list all possible causes and rule them out one by one. Do not guess or try random fixes. Confidence: 0.80
# communication
- Respond in Russian. Confidence: 0.95
- Keep responses terse — user finds verbosity annoying ("ты слишком много пишешь"), asks "коротко" and "по пунктам". Confidence: 0.80

# architecture
- Server code is too low-level — aggressively add dependencies (crates) to simplify: strum for enums, clap for CLI, dotnet-rng for RNG. User explicitly asked for "прям много зависимостей". Confidence: 0.80
- Wire protocol is immutable — never change packet names, payload formats, or byte-parsing semantics. Confidence: 0.95
- For intentional deviations from C# reference, mark in code with "НАМЕРЕННАЯ ДЕВИАЦИЯ от C# по ПРЯМОМУ ТРЕБОВАНИЮ ПОЛЬЗОВАТЕЛЯ" comment. Confidence: 0.70
- Truth hierarchy for porting: Client behavior > C# reference > Rust server. Always check client first. Confidence: 0.85

# code-style
- Use `query_player_opt` helper to eliminate `.flatten()` boilerplate on `query_player` calls that return `Option<Option<T>>`. Confidence: 0.70
- Use `broadcast_hb_at` helper instead of repeating chunk_pos + broadcast_to_nearby + encode_hb_bundle + hb_bundle pattern. Confidence: 0.70

# git
- Never commit directories: /Users/murasama/Projects/games/OpenMines/client and /Users/murasama/Projects/games/OpenMines/server_reference. Confidence: 0.85

# deployment
- Use SSH VPS alias for deployment. Confidence: 0.80
- Always deploy after changes — user verifies via live client play-test. Не спрашивать, деплоить автоматически после каждого изменения. User notices when deploy is skipped and asks "деплой прошел?". Confidence: 0.90
- Docker volumes named "openmines" (not "mines3_server_data" or similar). Confidence: 0.70

# project-management
- Update all roadmaps/docs when completing work or reaching checkpoints. Confidence: 0.75

# reference-porting
- Full 1:1 C#→Rust parity including filenames, struct names, constants — not just logic. If C# has file X.cs, Rust should have X.rs. Filename deviations cause strong user frustration ("почему ты это игнорируешь?"). Confidence: 0.95
- When a feature/mechanic does NOT exist in C# reference, document it separately in a protocol-gaps/doc file rather than silently inventing behavior. Confidence: 0.75

# verification
- Independently verify completed work — don't assume it functions correctly. User reports many "done" features are actually broken (e.g., FED chat). Verify empirically before declaring completion. User gets frustrated when told to verify things himself ("всам проверяй. заебал"). Confidence: 0.80

# communication
- Use short bullet-point format when asked "по пунктам" or "коротко". User explicitly prefers terse, structured responses over paragraphs. Confidence: 0.70
- For missing C# features (not in reference), document in separate file and implement independently — don't skip. Confidence: 0.70

# debugging
- When hitting client-side bugs (not server-caused), document them in roadmaps, checkpoint, and move on — don't chase endlessly. Confidence: 0.70

# workflow
- When user says to resume from a plan file (RESUME block), read it and continue autonomously without asking what to do. Confidence: 0.75

# project-management
- .md files (ROADMAP.md, docs/PARITY_AUDIT.md) are mangled by an external editor formatter — never commit them; user handles them separately. Confidence: 0.80
- Write durable handoff state to `.remember/remember.md` when pausing long-running work — concise <20 lines: what's done, what's next, current blocker. Confidence: 0.70

# skills
- Create project skills via TDD methodology: RED (baseline agent without skill fails) → GREEN (write skill) → REFACTOR (close loopholes). Verified by running test agent. Confidence: 0.70
- Features present in C# reference: port 1:1. Features absent from C# reference: document in a separate spec and implement from scratch — never invent reference-mimicking code for missing features. Confidence: 0.70

# testing
- Improve test coverage to prevent regressions — user explicitly wants better coverage after bugs slip through. Confidence: 0.60

# security
- For game-economy arithmetic (money, crystals, costs), always use `checked_mul` for multiplication and `saturating_add`/`saturating_sub` for accumulation — overflow in release mode (`overflow-checks = false`) silently wraps and bypasses guards. Confidence: 0.80

# workflow
- When commit hook fails on pre-commit (clippy/rustfmt), run `cargo fmt --all` first, then re-stage affected files, then re-commit. Repeated recovery pattern. Confidence: 0.70
