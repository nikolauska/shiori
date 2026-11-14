-- Alternative titles the user added by linking media to an anime, with the
-- date, as a JSON list of {title, date}. The titles are also in synonyms,
-- which stays what recognition reads.
ALTER TABLE anime_settings ADD COLUMN linked TEXT NOT NULL DEFAULT '[]';
