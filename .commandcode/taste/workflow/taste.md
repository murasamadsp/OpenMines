# workflow
- Use subagents (port-verifier skill) for C#→Rust parity verification, fan out many agents on Sonnet for cost efficiency. Confidence: 0.85
- Fix bugs empirically — "имперически" — root-cause investigation first, then fix, verify with tests. Never speculate or blame without data. Confidence: 0.75
- Commit everything, nothing left uncommitted. Include .claude/, docs, configs. Explicit user rule. Confidence: 0.85
- Work autonomously to completion — user says "далее" and "работай до конца автономно", wants momentum without pausing to ask. User gets angry at pauses ("ты встал. полчаса на месте простоял"). Confidence: 0.85
- When user says "ДАЛЕЕ" or "далее работай" — continue immediately, don't recap, don't acknowledge, just execute next task. Confidence: 0.85
- Never write malformed tool calls — parent/child XML tag mismatch causes parse failures. Confidence: 0.70
- Don't trust documentation or prior claims that features work — verify empirically in live client. User repeatedly found "done" features non-functional (FED chat, DNO messages, scrollbar flicker). Confidence: 0.80
- Fix all lints before committing; user explicitly demands lint-free commits. Confidence: 0.70
