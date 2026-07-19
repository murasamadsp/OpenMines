# Communication
- Speak briefly in Russian when communicating. Confidence: 0.85
- Respond concisely and avoid long explanations unless asked. Confidence: 0.85
- When user gives a concrete technical request, provide a working solution directly (e.g., a script) instead of explaining limitations first. Confidence: 0.75

# Workflow
See [workflow/taste.md](workflow/taste.md)
# Architecture
- Do not modify client network code (client/ directory). Confidence: 0.85
- Do not commit client/ or docs/reference/server_reference/ directories. Confidence: 0.80
- Keep code in crates/ only, not at workspace root level. Confidence: 0.75
- Do not perform I/O (send_u_packet, packet construction) inside modify_player closures; return values and build packets outside to avoid holding ECS write locks. Confidence: 0.70

# Rust
- Eliminate legacy code and workarounds instead of keeping them. Confidence: 0.70
- Prefer idiomatic, cognitively simple code with clear module boundaries. Confidence: 0.70
- Do not remove `#[allow(dead_code)]` annotations — they mark intentionally unused items for unimplemented features, not dead code to be eliminated. Confidence: 0.80

# Refactoring
- Split large files completely, not partially; create subdirectories with correct naming when needed; slight refactoring is OK but preserve existing behavior. Confidence: 0.80
