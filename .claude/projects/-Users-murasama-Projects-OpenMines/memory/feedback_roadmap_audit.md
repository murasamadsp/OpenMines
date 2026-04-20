---
name: roadmap-audit-approach
description: ROADMAP.md audit must be done item-by-item with deep analysis, never batched - previous agents broke functionality
type: feedback
---

ROADMAP.md пункты нельзя аудитировать батчем. Каждый пункт требует сверхглубокого анализа отдельно.

**Why:** Ранее агенты разрушили функционал. Пользователь сам составил ROADMAP после этого. Поверхностный аудит ("это уже работает") опасен — может быть частично сломано или не совпадать с C# референсом.

**How to apply:** при работе с ROADMAP — один пункт за раз, полное сравнение с server_reference/, проверка edge cases, только потом галочка. Никогда не батчить.
