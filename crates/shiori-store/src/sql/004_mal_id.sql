-- MyAnimeList ids, from AniList's idMal, for exporting a list in the
-- MyAnimeList XML format and importing one. Unknown for anime stored before
-- this until they are fetched again.
ALTER TABLE anime ADD COLUMN mal_id INTEGER;
