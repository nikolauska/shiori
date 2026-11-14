//! The SQLite database of one service.

use std::collections::BTreeMap;
use std::path::Path;

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, Row, params};
use shiori_core::recognition::{NumberingSequence, SequenceEntry};
use shiori_core::{
    AgeRating, AiringStatus, Anime, AnimeId, AnimeSettings, AnimeType, Fields, FuzzyDate,
    HistoryItem, ListEntry, ListEntryId, ListStatus, QueueItem, Titles,
};

use crate::Result;

/// Schema changes in order. The position in this list plus one is the
/// version stored in `PRAGMA user_version`; never edit an entry once
/// released, only add new ones.
const MIGRATIONS: &[&str] = &[
    include_str!("sql/001_create.sql"),
    include_str!("sql/002_linked_titles.sql"),
    include_str!("sql/003_numbering.sql"),
];

/// Everything loaded at start.
#[derive(Debug, Default)]
pub struct Loaded {
    pub anime: BTreeMap<AnimeId, Anime>,
    pub entries: BTreeMap<AnimeId, ListEntry>,
    pub anime_settings: BTreeMap<AnimeId, AnimeSettings>,
    /// Ordered by id.
    pub history: Vec<HistoryItem>,
    /// Ordered by time.
    pub queue: Vec<QueueItem>,
    /// Ordered by id.
    pub numbering: Vec<NumberingSequence>,
}

/// One open database. Kept open for the lifetime of the app.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens or creates the database file and brings its schema up to date.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    /// A database that lives only in memory, for tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version = self.schema_version()?;
        if version >= MIGRATIONS.len() {
            return Ok(());
        }
        // All pending migrations in one transaction, so a failure leaves the
        // file at its old version instead of half upgraded.
        let tx = self.conn.unchecked_transaction()?;
        for sql in &MIGRATIONS[version..] {
            tx.execute_batch(sql)?;
        }
        tx.pragma_update(None, "user_version", MIGRATIONS.len() as i64)?;
        tx.commit()?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<usize> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        Ok(usize::try_from(version).unwrap_or(0))
    }

    pub fn load_all(&self) -> Result<Loaded> {
        Ok(Loaded {
            anime: self.load_anime()?,
            entries: self.load_entries()?,
            anime_settings: self.load_anime_settings()?,
            history: self.load_history()?,
            queue: self.load_queue()?,
            numbering: self.load_numbering()?,
        })
    }

    // Anime

    pub fn upsert_anime(&self, items: &[Anime]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO anime (id, title, english, japanese, synonym, type, status,
                    episode_count, episode_length, date_start, date_end, image, trailer_id,
                    slug, age_rating, genres, tags, producers, studios, score, popularity,
                    synopsis, last_aired_episode, next_episode_time, modified)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                    ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
                 ON CONFLICT (id) DO UPDATE SET
                    title = excluded.title, english = excluded.english,
                    japanese = excluded.japanese, synonym = excluded.synonym,
                    type = excluded.type, status = excluded.status,
                    episode_count = excluded.episode_count,
                    episode_length = excluded.episode_length,
                    date_start = excluded.date_start, date_end = excluded.date_end,
                    image = excluded.image, trailer_id = excluded.trailer_id,
                    slug = excluded.slug, age_rating = excluded.age_rating,
                    genres = excluded.genres, tags = excluded.tags,
                    producers = excluded.producers, studios = excluded.studios,
                    score = excluded.score, popularity = excluded.popularity,
                    synopsis = excluded.synopsis,
                    last_aired_episode = excluded.last_aired_episode,
                    next_episode_time = excluded.next_episode_time,
                    modified = excluded.modified",
            )?;
            for a in items {
                stmt.execute(params![
                    a.id.0,
                    a.titles.romaji,
                    a.titles.english,
                    a.titles.japanese,
                    json(&a.titles.synonyms),
                    anime_type_to_db(a.kind),
                    airing_status_to_db(a.status),
                    a.episode_count,
                    a.episode_length,
                    a.date_started.to_storage(),
                    a.date_finished.to_storage(),
                    a.image_url,
                    a.trailer_id,
                    a.slug,
                    age_rating_to_db(a.age_rating),
                    json(&a.genres),
                    json(&a.tags),
                    json(&a.producers),
                    json(&a.studios),
                    f64::from(a.score),
                    a.popularity_rank,
                    a.synopsis,
                    a.last_aired_episode,
                    a.next_episode_time.map(Timestamp::as_second),
                    a.last_modified.map(Timestamp::as_second),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes an anime and its list entry, which cannot exist without it.
    pub fn delete_anime(&self, id: AnimeId) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM anime_list WHERE media_id = ?1", [id.0])?;
        tx.execute("DELETE FROM anime WHERE id = ?1", [id.0])?;
        tx.commit()?;
        Ok(())
    }

    fn load_anime(&self) -> Result<BTreeMap<AnimeId, Anime>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, english, japanese, synonym, type, status, episode_count,
                episode_length, date_start, date_end, image, trailer_id, slug, age_rating,
                genres, tags, producers, studios, score, popularity, synopsis,
                last_aired_episode, next_episode_time, modified
             FROM anime",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Anime {
                id: AnimeId(row.get(0)?),
                titles: Titles {
                    romaji: row.get(1)?,
                    english: row.get(2)?,
                    japanese: row.get(3)?,
                    synonyms: from_json(&row.get::<_, String>(4)?),
                },
                kind: anime_type_from_db(row.get(5)?),
                status: airing_status_from_db(row.get(6)?),
                episode_count: row.get(7)?,
                episode_length: row.get(8)?,
                date_started: FuzzyDate::parse(&row.get::<_, String>(9)?),
                date_finished: FuzzyDate::parse(&row.get::<_, String>(10)?),
                image_url: row.get(11)?,
                trailer_id: row.get(12)?,
                slug: row.get(13)?,
                age_rating: age_rating_from_db(row.get(14)?),
                genres: from_json(&row.get::<_, String>(15)?),
                tags: from_json(&row.get::<_, String>(16)?),
                producers: from_json(&row.get::<_, String>(17)?),
                studios: from_json(&row.get::<_, String>(18)?),
                score: row.get::<_, f64>(19)? as f32,
                popularity_rank: row.get(20)?,
                synopsis: row.get(21)?,
                last_aired_episode: row.get(22)?,
                next_episode_time: timestamp(row.get(23)?),
                last_modified: timestamp(row.get(24)?),
            })
        })?;
        let mut map = BTreeMap::new();
        for a in rows {
            let a = a?;
            map.insert(a.id, a);
        }
        Ok(map)
    }

    // List entries

    pub fn upsert_entries(&self, entries: &[ListEntry]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO anime_list (media_id, id, progress, date_start, date_end, score,
                    status, private, rewatched_times, rewatching, rewatching_ep, notes,
                    last_updated, pending_delete)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT (media_id) DO UPDATE SET
                    id = excluded.id, progress = excluded.progress,
                    date_start = excluded.date_start, date_end = excluded.date_end,
                    score = excluded.score, status = excluded.status,
                    private = excluded.private, rewatched_times = excluded.rewatched_times,
                    rewatching = excluded.rewatching, rewatching_ep = excluded.rewatching_ep,
                    notes = excluded.notes, last_updated = excluded.last_updated,
                    pending_delete = excluded.pending_delete",
            )?;
            for e in entries {
                stmt.execute(params![
                    e.anime_id.0,
                    e.id.map(|id| id.0),
                    e.watched_episodes,
                    e.date_started.to_storage(),
                    e.date_completed.to_storage(),
                    e.score,
                    list_status_to_db(e.status),
                    e.is_private,
                    e.rewatched_times,
                    e.rewatching,
                    e.rewatching_ep,
                    e.notes,
                    e.last_updated.as_second(),
                    e.pending_delete,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_entry(&self, anime_id: AnimeId) -> Result<()> {
        self.conn
            .execute("DELETE FROM anime_list WHERE media_id = ?1", [anime_id.0])?;
        Ok(())
    }

    fn load_entries(&self) -> Result<BTreeMap<AnimeId, ListEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT media_id, id, progress, date_start, date_end, score, status, private,
                rewatched_times, rewatching, rewatching_ep, notes, last_updated, pending_delete
             FROM anime_list",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ListEntry {
                anime_id: AnimeId(row.get(0)?),
                id: row.get::<_, Option<i64>>(1)?.map(ListEntryId),
                watched_episodes: row.get(2)?,
                date_started: FuzzyDate::parse(&row.get::<_, String>(3)?),
                date_completed: FuzzyDate::parse(&row.get::<_, String>(4)?),
                score: row.get(5)?,
                status: list_status_from_db(row.get(6)?),
                is_private: row.get(7)?,
                rewatched_times: row.get(8)?,
                rewatching: row.get(9)?,
                rewatching_ep: row.get(10)?,
                notes: row.get(11)?,
                last_updated: timestamp(Some(row.get(12)?)).unwrap_or(Timestamp::UNIX_EPOCH),
                pending_delete: row.get(13)?,
            })
        })?;
        let mut map = BTreeMap::new();
        for e in rows {
            let e = e?;
            map.insert(e.anime_id, e);
        }
        Ok(map)
    }

    // Per-anime settings

    pub fn upsert_anime_settings(&self, s: &AnimeSettings) -> Result<()> {
        self.conn.execute(
            "INSERT INTO anime_settings (id, display_title, folder, synonyms, linked)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET
                display_title = excluded.display_title, folder = excluded.folder,
                synonyms = excluded.synonyms, linked = excluded.linked",
            params![
                s.id.0,
                s.display_title,
                s.folder,
                json(&s.synonyms),
                json(&s.linked)
            ],
        )?;
        Ok(())
    }

    fn load_anime_settings(&self) -> Result<BTreeMap<AnimeId, AnimeSettings>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, display_title, folder, synonyms, linked FROM anime_settings")?;
        let rows = stmt.query_map([], |row| {
            Ok(AnimeSettings {
                id: AnimeId(row.get(0)?),
                display_title: row.get(1)?,
                folder: row.get(2)?,
                synonyms: from_json(&row.get::<_, String>(3)?),
                linked: from_json(&row.get::<_, String>(4)?),
            })
        })?;
        let mut map = BTreeMap::new();
        for s in rows {
            let s = s?;
            map.insert(s.id, s);
        }
        Ok(map)
    }

    // Remembered numbering

    /// Saves a sequence and its entries in one transaction, so a failure
    /// keeps the old one whole. Returns the sequence's id, new when `id` is
    /// 0.
    pub fn save_numbering(&self, sequence: &NumberingSequence) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let saved = sequence.saved.as_second();
        let id = if sequence.id == 0 {
            tx.execute(
                "INSERT INTO numbering_sequence (root, saved_at) VALUES (?1, ?2)",
                params![sequence.root.0, saved],
            )?;
            tx.last_insert_rowid()
        } else {
            tx.execute(
                "UPDATE numbering_sequence SET root = ?2, saved_at = ?3 WHERE id = ?1",
                params![sequence.id, sequence.root.0, saved],
            )?;
            tx.execute(
                "DELETE FROM numbering_entry WHERE sequence_id = ?1",
                [sequence.id],
            )?;
            sequence.id
        };
        for e in &sequence.entries {
            tx.execute(
                "INSERT INTO numbering_entry (sequence_id, anime_id, first, last)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, e.anime.0, e.first, e.last],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn delete_numbering(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM numbering_sequence WHERE id = ?1", [id])?;
        Ok(())
    }

    fn load_numbering(&self) -> Result<Vec<NumberingSequence>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, root, saved_at FROM numbering_sequence ORDER BY id")?;
        let mut sequences: Vec<NumberingSequence> = stmt
            .query_map([], |row| {
                Ok(NumberingSequence {
                    id: row.get(0)?,
                    root: AnimeId(row.get(1)?),
                    entries: Vec::new(),
                    saved: Timestamp::from_second(row.get(2)?).unwrap_or_default(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT sequence_id, anime_id, first, last FROM numbering_entry ORDER BY first",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                SequenceEntry {
                    anime: AnimeId(row.get(1)?),
                    first: row.get(2)?,
                    last: row.get(3)?,
                },
            ))
        })?;
        for row in rows {
            let (id, entry) = row?;
            if let Some(s) = sequences.iter_mut().find(|s| s.id == id) {
                s.entries.push(entry);
            }
        }
        Ok(sequences)
    }

    // History

    /// Returns the id of the new item.
    pub fn add_history(&self, anime_id: AnimeId, episode: u32, time: Timestamp) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO history (anime_id, episode, time) VALUES (?1, ?2, ?3)",
            params![anime_id.0, episode, time.as_second()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn delete_history(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM history WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        self.conn.execute("DELETE FROM history", [])?;
        Ok(())
    }

    fn load_history(&self) -> Result<Vec<HistoryItem>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, anime_id, episode, time FROM history ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok(HistoryItem {
                id: row.get(0)?,
                anime_id: AnimeId(row.get(1)?),
                episode: row.get(2)?,
                time: timestamp(Some(row.get(3)?)).unwrap_or(Timestamp::UNIX_EPOCH),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // Queue

    pub fn upsert_queue_item(&self, item: &QueueItem) -> Result<()> {
        self.conn.execute(
            "INSERT INTO queue (anime_id, dirty, time, retry_count, last_error)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (anime_id) DO UPDATE SET
                dirty = excluded.dirty, time = excluded.time,
                retry_count = excluded.retry_count, last_error = excluded.last_error",
            params![
                item.anime_id.0,
                item.dirty.bits(),
                item.time.as_second(),
                item.retry_count,
                item.last_error,
            ],
        )?;
        Ok(())
    }

    pub fn delete_queue_item(&self, anime_id: AnimeId) -> Result<()> {
        self.conn
            .execute("DELETE FROM queue WHERE anime_id = ?1", [anime_id.0])?;
        Ok(())
    }

    pub fn clear_queue(&self) -> Result<()> {
        self.conn.execute("DELETE FROM queue", [])?;
        Ok(())
    }

    fn load_queue(&self) -> Result<Vec<QueueItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT anime_id, dirty, time, retry_count, last_error FROM queue
             ORDER BY time, anime_id",
        )?;
        let rows = stmt.query_map([], queue_item)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn queue_item(&self, anime_id: AnimeId) -> Result<Option<QueueItem>> {
        Ok(self
            .conn
            .query_row(
                "SELECT anime_id, dirty, time, retry_count, last_error FROM queue
                 WHERE anime_id = ?1",
                [anime_id.0],
                queue_item,
            )
            .optional()?)
    }
}

fn queue_item(row: &Row<'_>) -> rusqlite::Result<QueueItem> {
    Ok(QueueItem {
        anime_id: AnimeId(row.get(0)?),
        dirty: Fields::from_bits_truncate(row.get(1)?),
        time: timestamp(Some(row.get(2)?)).unwrap_or(Timestamp::UNIX_EPOCH),
        retry_count: row.get(3)?,
        last_error: row.get(4)?,
    })
}

fn json<T: serde::Serialize>(list: &[T]) -> String {
    serde_json::to_string(list).unwrap_or_else(|_| "[]".into())
}

/// A damaged list column is read as empty rather than failing the whole load;
/// the next fetch from the service rewrites it.
fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Vec<T> {
    serde_json::from_str(text).unwrap_or_else(|error| {
        tracing::warn!("Invalid list in database: {error}");
        Vec::new()
    })
}

fn timestamp(seconds: Option<i64>) -> Option<Timestamp> {
    seconds.and_then(|s| Timestamp::from_second(s).ok())
}

// The numbers below are part of the file format. Never renumber them.

fn anime_type_to_db(v: AnimeType) -> i64 {
    match v {
        AnimeType::Unknown => 0,
        AnimeType::Tv => 1,
        AnimeType::Ova => 2,
        AnimeType::Movie => 3,
        AnimeType::Special => 4,
        AnimeType::Ona => 5,
        AnimeType::Music => 6,
    }
}

fn anime_type_from_db(v: i64) -> AnimeType {
    match v {
        1 => AnimeType::Tv,
        2 => AnimeType::Ova,
        3 => AnimeType::Movie,
        4 => AnimeType::Special,
        5 => AnimeType::Ona,
        6 => AnimeType::Music,
        _ => AnimeType::Unknown,
    }
}

fn airing_status_to_db(v: AiringStatus) -> i64 {
    match v {
        AiringStatus::Unknown => 0,
        AiringStatus::FinishedAiring => 1,
        AiringStatus::Airing => 2,
        AiringStatus::NotYetAired => 3,
    }
}

fn airing_status_from_db(v: i64) -> AiringStatus {
    match v {
        1 => AiringStatus::FinishedAiring,
        2 => AiringStatus::Airing,
        3 => AiringStatus::NotYetAired,
        _ => AiringStatus::Unknown,
    }
}

fn age_rating_to_db(v: AgeRating) -> i64 {
    match v {
        AgeRating::Unknown => 0,
        AgeRating::G => 1,
        AgeRating::Pg => 2,
        AgeRating::Pg13 => 3,
        AgeRating::R17 => 4,
        AgeRating::R18 => 5,
    }
}

fn age_rating_from_db(v: i64) -> AgeRating {
    match v {
        1 => AgeRating::G,
        2 => AgeRating::Pg,
        3 => AgeRating::Pg13,
        4 => AgeRating::R17,
        5 => AgeRating::R18,
        _ => AgeRating::Unknown,
    }
}

fn list_status_to_db(v: ListStatus) -> i64 {
    match v {
        ListStatus::NotInList => 0,
        ListStatus::Watching => 1,
        ListStatus::Completed => 2,
        ListStatus::OnHold => 3,
        ListStatus::Dropped => 4,
        ListStatus::PlanToWatch => 5,
    }
}

fn list_status_from_db(v: i64) -> ListStatus {
    match v {
        1 => ListStatus::Watching,
        2 => ListStatus::Completed,
        3 => ListStatus::OnHold,
        4 => ListStatus::Dropped,
        5 => ListStatus::PlanToWatch,
        _ => ListStatus::NotInList,
    }
}
