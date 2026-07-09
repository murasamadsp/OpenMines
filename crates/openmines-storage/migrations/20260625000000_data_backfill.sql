-- Data backfill migration to unify dual DB migration systems
-- Handles data migrations previously performed dynamically in Rust

-- 1. Backfill boxes crystal columns from legacy cry_* columns
UPDATE boxes SET
    ze = CASE WHEN ze = 0 THEN cry_green ELSE ze END,
    cr = CASE WHEN cr = 0 THEN cry_blue ELSE cr END,
    si = CASE WHEN si = 0 THEN cry_red ELSE si END,
    be = CASE WHEN be = 0 THEN cry_violet ELSE be END,
    fi = CASE WHEN fi = 0 THEN cry_white ELSE fi END,
    go = CASE WHEN go = 0 THEN cry_cyan ELSE go END
WHERE cry_green != 0 OR cry_blue != 0 OR cry_red != 0 OR cry_violet != 0 OR cry_white != 0 OR cry_cyan != 0;

-- 2. Backfill chat_messages.player_id from players.id mapping
UPDATE chat_messages SET player_id = (
     SELECT p.id FROM players p WHERE p.name = chat_messages.player_name
 )
 WHERE player_id = 0
   AND EXISTS (
     SELECT 1 FROM players p WHERE p.name = chat_messages.player_name
 );

-- 3. Bump Movement skill level to 60 for players if it is below 60
UPDATE players SET skills = json_set(skills, '$.M.level', 60)
 WHERE json_valid(skills)
   AND json_extract(skills, '$.M.level') < 60;
