-- Crafter completion resend state.
-- `buildings.data` is strict JSON; add the field explicitly so serde can stay
-- fail-fast and not accept hidden defaults.
UPDATE buildings
SET data = json_set(data, '$.craft_ready', false)
WHERE data IS NOT NULL
  AND json_valid(data)
  AND json_type(data, '$.craft_ready') IS NULL;
