CREATE TABLE anime (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL DEFAULT '',
  english TEXT NOT NULL DEFAULT '',
  japanese TEXT NOT NULL DEFAULT '',
  synonym TEXT NOT NULL DEFAULT '[]',
  type INTEGER NOT NULL DEFAULT 0,
  status INTEGER NOT NULL DEFAULT 0,
  episode_count INTEGER,
  episode_length INTEGER,
  date_start TEXT NOT NULL DEFAULT '',
  date_end TEXT NOT NULL DEFAULT '',
  image TEXT NOT NULL DEFAULT '',
  trailer_id TEXT NOT NULL DEFAULT '',
  slug TEXT NOT NULL DEFAULT '',
  age_rating INTEGER NOT NULL DEFAULT 0,
  genres TEXT NOT NULL DEFAULT '[]',
  tags TEXT NOT NULL DEFAULT '[]',
  producers TEXT NOT NULL DEFAULT '[]',
  studios TEXT NOT NULL DEFAULT '[]',
  score REAL NOT NULL DEFAULT 0,
  popularity INTEGER NOT NULL DEFAULT 0,
  synopsis TEXT NOT NULL DEFAULT '',
  last_aired_episode INTEGER NOT NULL DEFAULT 0,
  next_episode_time INTEGER,
  modified INTEGER
);

CREATE TABLE anime_list (
  media_id INTEGER PRIMARY KEY REFERENCES anime (id),
  id INTEGER,
  progress INTEGER NOT NULL DEFAULT 0,
  date_start TEXT NOT NULL DEFAULT '',
  date_end TEXT NOT NULL DEFAULT '',
  score INTEGER NOT NULL DEFAULT 0,
  status INTEGER NOT NULL DEFAULT 0,
  private INTEGER NOT NULL DEFAULT 0,
  rewatched_times INTEGER NOT NULL DEFAULT 0,
  rewatching INTEGER NOT NULL DEFAULT 0,
  rewatching_ep INTEGER NOT NULL DEFAULT 0,
  notes TEXT NOT NULL DEFAULT '',
  last_updated INTEGER NOT NULL DEFAULT 0,
  pending_delete INTEGER NOT NULL DEFAULT 0
);

-- No foreign key: settings outlive a refetch of the anime.
CREATE TABLE anime_settings (
  id INTEGER PRIMARY KEY,
  display_title TEXT NOT NULL DEFAULT '',
  folder TEXT NOT NULL DEFAULT '',
  synonyms TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE history (
  id INTEGER PRIMARY KEY,
  anime_id INTEGER NOT NULL,
  episode INTEGER NOT NULL,
  time INTEGER NOT NULL
);

-- dirty holds the changed-field bits; 0 means delete.
CREATE TABLE queue (
  anime_id INTEGER PRIMARY KEY,
  dirty INTEGER NOT NULL,
  time INTEGER NOT NULL,
  retry_count INTEGER NOT NULL DEFAULT 0,
  last_error TEXT
);
