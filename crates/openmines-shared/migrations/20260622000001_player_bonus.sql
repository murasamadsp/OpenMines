-- Ежедневный бонус (кнопка БОНУСЫ / GDon): время последнего клейма, unix-секунды.
-- 0 = ни разу не забирал → доступен сразу.
ALTER TABLE players ADD COLUMN last_bonus_at INTEGER NOT NULL DEFAULT 0;
