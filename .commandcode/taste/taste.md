# Communication
- Speak briefly in Russian when communicating. Confidence: 0.85
- Respond concisely and avoid long explanations unless asked. Confidence: 0.80

# Workflow
- Run `cargo clippy --all-targets` and `cargo fmt --all` before committing. Confidence: 0.80
- Work only on main branch; do not create branches. Confidence: 0.75
- Do not revert or discard previous agents' changes. Confidence: 0.75
- Ask before deleting any files or directories. Confidence: 0.75
- Prioritize plan execution over micro-fixes and tangents. Confidence: 0.75

# Architecture
- Do not modify client network code (client/ directory). Confidence: 0.85
- Do not commit client/ or docs/reference/server_reference/ directories. Confidence: 0.80
- Keep code in crates/ only, not at workspace root level. Confidence: 0.75

# Rust
- Eliminate legacy code and workarounds instead of keeping them. Confidence: 0.70
- Prefer idiomatic, cognitively simple code with clear module boundaries. Confidence: 0.70
