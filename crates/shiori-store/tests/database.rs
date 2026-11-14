use jiff::Timestamp;
use shiori_core::*;
use shiori_store::Database;

fn ts(s: i64) -> Timestamp {
    Timestamp::from_second(s).unwrap()
}

fn full_anime(id: u32) -> Anime {
    Anime {
        id: AnimeId(id),
        last_modified: Some(ts(1_700_000_000)),
        episode_count: Some(25),
        episode_length: Some(24),
        age_rating: AgeRating::Pg13,
        status: AiringStatus::FinishedAiring,
        kind: AnimeType::Tv,
        date_started: FuzzyDate::new(2008, 10, 2),
        date_finished: FuzzyDate::new(2009, 3, 0),
        score: 8.25,
        popularity_rank: 42,
        image_url: "https://example.com/a.jpg".into(),
        slug: "toradora".into(),
        synopsis: "Ryuuji and Taiga.".into(),
        trailer_id: "abc123".into(),
        titles: Titles {
            romaji: "Toradora!".into(),
            english: "Toradora!".into(),
            japanese: "とらドラ！".into(),
            synonyms: vec!["Tiger X Dragon".into(), "トラドラ".into()],
        },
        genres: vec!["Comedy".into(), "Romance".into()],
        producers: vec!["Genco".into()],
        studios: vec!["J.C.Staff".into()],
        tags: vec!["School".into()],
        last_aired_episode: 25,
        next_episode_time: Some(ts(1_700_000_100)),
    }
}

fn full_entry(id: u32) -> ListEntry {
    ListEntry {
        id: Some(ListEntryId(987654321)),
        anime_id: AnimeId(id),
        watched_episodes: 12,
        score: 85,
        status: ListStatus::OnHold,
        is_private: true,
        rewatched_times: 2,
        rewatching: true,
        rewatching_ep: 3,
        date_started: FuzzyDate::new(2020, 1, 0),
        date_completed: FuzzyDate::new(2021, 0, 0),
        last_updated: ts(1_700_000_200),
        notes: "notes with 'quotes' and ünïcode".into(),
        pending_delete: true,
    }
}

#[test]
fn schema_is_created_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data/anilist.sqlite");
    {
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 3);
        db.upsert_anime(&[full_anime(1)]).unwrap();
    }
    // Reopening keeps the data and does not run the migration again.
    let db = Database::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 3);
    assert_eq!(db.load_all().unwrap().anime.len(), 1);
}

#[test]
fn a_first_version_file_gains_linked_titles_and_keeps_its_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("anilist.sqlite");
    {
        // A file as the first release wrote it.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../src/sql/001_create.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO anime_settings (id, synonyms) VALUES (5, '[\"One\"]')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let db = Database::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 3);
    let settings = &db.load_all().unwrap().anime_settings[&AnimeId(5)];
    assert_eq!(settings.synonyms, ["One"]);
    assert!(settings.linked.is_empty());
}

#[test]
fn anime_round_trip() {
    let db = Database::open_in_memory().unwrap();
    let full = full_anime(1);
    let empty = Anime::new(AnimeId(2));
    db.upsert_anime(&[full.clone(), empty.clone()]).unwrap();
    let loaded = db.load_all().unwrap().anime;
    assert_eq!(loaded[&AnimeId(1)], full);
    assert_eq!(loaded[&AnimeId(2)], empty);

    let mut changed = full.clone();
    changed.titles.romaji = "Changed".into();
    changed.episode_count = None;
    db.upsert_anime(&[changed.clone()]).unwrap();
    assert_eq!(db.load_all().unwrap().anime[&AnimeId(1)], changed);
}

#[test]
fn every_enum_value_round_trips() {
    let db = Database::open_in_memory().unwrap();
    let kinds = [
        AnimeType::Unknown,
        AnimeType::Tv,
        AnimeType::Ova,
        AnimeType::Movie,
        AnimeType::Special,
        AnimeType::Ona,
        AnimeType::Music,
    ];
    let statuses = [
        AiringStatus::Unknown,
        AiringStatus::FinishedAiring,
        AiringStatus::Airing,
        AiringStatus::NotYetAired,
    ];
    let ratings = [
        AgeRating::Unknown,
        AgeRating::G,
        AgeRating::Pg,
        AgeRating::Pg13,
        AgeRating::R17,
        AgeRating::R18,
    ];
    let list_statuses = [
        ListStatus::NotInList,
        ListStatus::Watching,
        ListStatus::Completed,
        ListStatus::OnHold,
        ListStatus::Dropped,
        ListStatus::PlanToWatch,
    ];
    let mut anime = Vec::new();
    let mut entries = Vec::new();
    for i in 0..7u32 {
        let mut a = Anime::new(AnimeId(i + 1));
        a.kind = kinds[i as usize % kinds.len()];
        a.status = statuses[i as usize % statuses.len()];
        a.age_rating = ratings[i as usize % ratings.len()];
        let mut e = ListEntry::new(a.id);
        e.status = list_statuses[i as usize % list_statuses.len()];
        anime.push(a);
        entries.push(e);
    }
    db.upsert_anime(&anime).unwrap();
    db.upsert_entries(&entries).unwrap();
    let loaded = db.load_all().unwrap();
    for a in &anime {
        assert_eq!(&loaded.anime[&a.id], a);
    }
    for e in &entries {
        assert_eq!(&loaded.entries[&e.anime_id], e);
    }
}

#[test]
fn entries_round_trip_and_delete() {
    let db = Database::open_in_memory().unwrap();
    db.upsert_anime(&[full_anime(1), full_anime(2)]).unwrap();
    let full = full_entry(1);
    let empty = ListEntry::new(AnimeId(2));
    db.upsert_entries(&[full.clone(), empty.clone()]).unwrap();
    let loaded = db.load_all().unwrap().entries;
    assert_eq!(loaded[&AnimeId(1)], full);
    assert_eq!(loaded[&AnimeId(2)], empty);

    db.delete_entry(AnimeId(2)).unwrap();
    assert!(!db.load_all().unwrap().entries.contains_key(&AnimeId(2)));
}

#[test]
fn entry_needs_its_anime() {
    let db = Database::open_in_memory().unwrap();
    assert!(db.upsert_entries(&[full_entry(1)]).is_err());
}

#[test]
fn deleting_anime_removes_its_entry() {
    let db = Database::open_in_memory().unwrap();
    db.upsert_anime(&[full_anime(1)]).unwrap();
    db.upsert_entries(&[full_entry(1)]).unwrap();
    db.delete_anime(AnimeId(1)).unwrap();
    let loaded = db.load_all().unwrap();
    assert!(loaded.anime.is_empty());
    assert!(loaded.entries.is_empty());
}

#[test]
fn anime_settings_round_trip() {
    let db = Database::open_in_memory().unwrap();
    let s = AnimeSettings {
        id: AnimeId(5),
        display_title: "Shown".into(),
        folder: "/media/anime/Show".into(),
        synonyms: vec!["One".into(), "Two".into()],
        linked: vec![LinkedTitle {
            title: "Two".into(),
            date: FuzzyDate::new(2026, 11, 14),
        }],
    };
    db.upsert_anime_settings(&s).unwrap();
    // No anime with id 5 exists: settings do not depend on it.
    assert_eq!(db.load_all().unwrap().anime_settings[&AnimeId(5)], s);
    let s2 = AnimeSettings {
        synonyms: vec![],
        ..s.clone()
    };
    db.upsert_anime_settings(&s2).unwrap();
    assert_eq!(db.load_all().unwrap().anime_settings[&AnimeId(5)], s2);
}

#[test]
fn numbering_round_trip() {
    use shiori_core::recognition::{NumberingSequence, SequenceEntry};
    let db = Database::open_in_memory().unwrap();
    let entry = |anime, first, last| SequenceEntry {
        anime: AnimeId(anime),
        first,
        last,
    };
    let mut s = NumberingSequence {
        id: 0,
        root: AnimeId(1),
        entries: vec![entry(1, 1, 12), entry(2, 13, 24)],
        saved: ts(100),
    };
    s.id = db.save_numbering(&s).unwrap();
    assert!(s.id > 0);
    assert_eq!(db.load_all().unwrap().numbering, [s.clone()]);
    // Saving again replaces the entries.
    s.entries.push(entry(3, 25, 36));
    assert_eq!(db.save_numbering(&s).unwrap(), s.id);
    assert_eq!(db.load_all().unwrap().numbering, [s.clone()]);
    // An anime can be in one sequence only; the failed save leaves the old
    // sequence whole.
    let other = NumberingSequence {
        id: 0,
        root: AnimeId(9),
        entries: vec![entry(9, 1, 1), entry(2, 2, 2)],
        saved: ts(100),
    };
    assert!(db.save_numbering(&other).is_err());
    assert_eq!(db.load_all().unwrap().numbering, [s.clone()]);
    db.delete_numbering(s.id).unwrap();
    assert!(db.load_all().unwrap().numbering.is_empty());
}

#[test]
fn history_round_trip() {
    let db = Database::open_in_memory().unwrap();
    let a = db.add_history(AnimeId(1), 3, ts(100)).unwrap();
    let b = db.add_history(AnimeId(2), 1, ts(50)).unwrap();
    let history = db.load_all().unwrap().history;
    assert_eq!(
        history,
        [
            HistoryItem {
                id: a,
                anime_id: AnimeId(1),
                episode: 3,
                time: ts(100)
            },
            HistoryItem {
                id: b,
                anime_id: AnimeId(2),
                episode: 1,
                time: ts(50)
            },
        ],
        "ordered by id, not time"
    );
    db.delete_history(a).unwrap();
    assert_eq!(db.load_all().unwrap().history.len(), 1);
    db.clear_history().unwrap();
    assert!(db.load_all().unwrap().history.is_empty());
}

#[test]
fn queue_round_trip() {
    let db = Database::open_in_memory().unwrap();
    let later = QueueItem {
        anime_id: AnimeId(1),
        dirty: Fields::EPISODE | Fields::NOTES,
        time: ts(200),
        retry_count: 2,
        last_error: Some("Rate limited".into()),
    };
    let delete = QueueItem {
        anime_id: AnimeId(2),
        dirty: Fields::empty(),
        time: ts(100),
        retry_count: 0,
        last_error: None,
    };
    db.upsert_queue_item(&later).unwrap();
    db.upsert_queue_item(&delete).unwrap();
    assert_eq!(
        db.load_all().unwrap().queue,
        [delete.clone(), later.clone()]
    );
    assert!(db.queue_item(AnimeId(2)).unwrap().unwrap().is_delete());

    let updated = QueueItem {
        dirty: Fields::all(),
        ..later
    };
    db.upsert_queue_item(&updated).unwrap();
    assert_eq!(db.queue_item(AnimeId(1)).unwrap(), Some(updated));
    db.delete_queue_item(AnimeId(1)).unwrap();
    assert_eq!(db.queue_item(AnimeId(1)).unwrap(), None);
    db.clear_queue().unwrap();
    assert!(db.load_all().unwrap().queue.is_empty());
}

#[test]
fn a_local_copy_leaves_out_the_queue_and_waiting_removals() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join("anilist.sqlite")).unwrap();
    db.upsert_anime(&[full_anime(1), full_anime(2)]).unwrap();
    let kept = ListEntry {
        pending_delete: false,
        ..full_entry(1)
    };
    // full_entry is waiting to be removed.
    db.upsert_entries(&[kept.clone(), full_entry(2)]).unwrap();
    db.upsert_queue_item(&QueueItem {
        anime_id: AnimeId(1),
        dirty: Fields::EPISODE,
        time: ts(100),
        retry_count: 0,
        last_error: None,
    })
    .unwrap();
    db.add_history(AnimeId(1), 12, ts(100)).unwrap();

    let path = dir.path().join("local.sqlite");
    db.copy_as_local(&path).unwrap();
    let copy = Database::open(&path).unwrap().load_all().unwrap();
    assert_eq!(copy.anime.len(), 2);
    assert_eq!(copy.entries.into_values().collect::<Vec<_>>(), [kept]);
    assert!(copy.queue.is_empty());
    assert_eq!(copy.history.len(), 1);
    // The original is untouched.
    assert_eq!(db.load_all().unwrap().queue.len(), 1);
}

#[test]
fn a_backup_restores_everything_and_other_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(&dir.path().join("local.sqlite")).unwrap();
    db.upsert_anime(&[full_anime(1)]).unwrap();
    let entry = ListEntry {
        pending_delete: false,
        ..full_entry(1)
    };
    db.upsert_entries(std::slice::from_ref(&entry)).unwrap();
    db.add_history(AnimeId(1), 12, ts(100)).unwrap();

    let backup = dir.path().join("backup.sqlite");
    db.backup_to(&backup).unwrap();
    // Replacing an earlier backup works too.
    db.backup_to(&backup).unwrap();

    db.delete_entry(AnimeId(1)).unwrap();
    db.clear_history().unwrap();
    db.restore_from(&backup).unwrap();
    let restored = db.load_all().unwrap();
    assert_eq!(restored.entries.into_values().collect::<Vec<_>>(), [entry]);
    assert_eq!(restored.history.len(), 1);

    let text = dir.path().join("notes.txt");
    std::fs::write(&text, "not a database").unwrap();
    assert!(db.restore_from(&text).is_err());
    let other = dir.path().join("other.sqlite");
    rusqlite::Connection::open(&other)
        .unwrap()
        .execute_batch("CREATE TABLE x (y INTEGER)")
        .unwrap();
    assert!(db.restore_from(&other).is_err());
    // A refused file changes nothing.
    assert_eq!(db.load_all().unwrap().entries.len(), 1);
}
