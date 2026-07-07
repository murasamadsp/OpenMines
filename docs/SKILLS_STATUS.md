# Skills Status

Дата: 2026-07-07.

Цель файла: честно фиксировать, какие скиллы реально подключены к runtime, а
какие пока только существуют как wire/DB-код. Наличие `SkillType` не означает
готовую механику.

## Инварианты

- Единственный machine-code скилла — `SkillType::code()` / wire/DB code.
- `/skill` принимает только wire/DB code. Алиасов вроде `GEO` нет намеренно.
- `skill_effect` и `exp_needed` должны быть исчерпывающими `match` без wildcard:
  новый `SkillType` обязан явно получить формулу/стоимость опыта.
- Опыт начисляется только установленному в слот скиллу. Это соответствует
  текущей модели `add_skill_exp`.

## Runtime Coverage

| Skill | Code | Current runtime status |
|---|---:|---|
| Movement | `M` | affects speed packet; gains exp on move and dig-turn |
| Digging | `d` | affects dig damage; gains exp on destroy and boulder push |
| MineGeneral | `m` | affects crystal mining; gains exp from crystal yield |
| MineGreen/Blue/Red/Violet/White/Cyan | `G/B/R/V/W/C` | affects crystal yield by color; color-specific exp hook is not wired |
| Health | `l` | affects max HP at login/admin set; gains exp on hurt/death paths |
| Repair | `e` | affects heal amount; gains exp on successful heal |
| AntiGun | `u` | reduces gun damage; gains exp on gun hurt; `exp_needed=0` |
| Induction | `*I` | affects gun-shot crystal induction; gains exp on gun hurt |
| BuildGreen/Yellow/Red/Road/Structure/War | `L/Y/E/A/O/*L` | affects current build/upgrade paths; gains exp on successful placement |
| Packing | `p` | has capacity formula, but outbound basket capacity is currently hardcoded to C# value `1`; runtime effect is partial |
| Geology | `U` | required for `Xgeo`; no exp hook currently wired |
| Remaining SkillType variants | various | code/formula/UI may exist, but no proven gameplay hook in current Rust runtime |

## Open Work

- Decide whether color-specific mining skills should gain exp together with
  `MineGeneral`, and verify against reference behavior before changing.
- Audit every `SkillType` against gameplay action, exp source, UI install/upgrade
  path, and packet sync.
- Replace trait methods marked `dead_code` with either live runtime use or remove
  them after a reference-backed decision.
- Make `Packing` semantics explicit: C# basket packet currently sends capacity
  `1`, while `SkillType::Packing` still has a formula.
