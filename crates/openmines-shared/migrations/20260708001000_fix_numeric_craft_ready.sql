-- Repair databases migrated with bare SQLite `false`, which JSON1 stores as
-- integer 0 instead of JSON boolean false.
UPDATE buildings
SET data = json_set(data, '$.craft_ready', json('false'))
WHERE data IS NOT NULL
  AND json_valid(data)
  AND json_type(data, '$.craft_ready') = 'integer'
  AND json_extract(data, '$.craft_ready') = 0;

UPDATE buildings
SET data = json_set(data, '$.craft_ready', json('true'))
WHERE data IS NOT NULL
  AND json_valid(data)
  AND json_type(data, '$.craft_ready') = 'integer'
  AND json_extract(data, '$.craft_ready') = 1;
