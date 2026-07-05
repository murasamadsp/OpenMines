CREATE UNIQUE INDEX IF NOT EXISTS idx_clan_requests_unique_pair
ON clan_requests(clan_id, player_id);
