//! Prints the anime id and episode for a file name.
//!
//! Usage: cargo run -p shiori-core --example identify -- <anime.json> <file name> [anilist|myanimelist|kitsu]
//!
//! `anime.json` is an array of anime objects with the field names of
//! `shiori_core::Anime`; missing fields take their defaults. The relations
//! file is the bundled `data/anime-relations.txt`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use shiori_core::recognition::{ParseOptions, Relations, TitleIndex, identify, parse_path};
use shiori_core::{Anime, ServiceId};

const RELATIONS: &str = include_str!("../../../data/anime-relations.txt");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(anime_path), Some(file_name)) = (args.first(), args.get(1)) else {
        eprintln!("usage: identify <anime.json> <file name> [anilist|myanimelist|kitsu]");
        return ExitCode::from(2);
    };
    let service = match args.get(2).map(String::as_str) {
        None | Some("anilist") => ServiceId::AniList,
        Some("myanimelist") => ServiceId::MyAnimeList,
        Some("kitsu") => ServiceId::Kitsu,
        Some(other) => {
            eprintln!("unknown service: {other}");
            return ExitCode::from(2);
        }
    };
    let list: Vec<Anime> = match std::fs::read_to_string(anime_path)
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
    {
        Ok(list) => list,
        Err(error) => {
            eprintln!("could not read {anime_path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let anime: BTreeMap<_, _> = list.into_iter().map(|a| (a.id, a)).collect();
    let index = TitleIndex::build(anime.values(), |_| None);
    let relations = Relations::parse(RELATIONS, service);

    let mut episode = parse_path(Path::new(file_name), &[], ParseOptions::default());
    let title = episode.title().unwrap_or_default().to_owned();
    match identify(
        &mut episode,
        &index,
        &anime,
        &relations,
        &Default::default(),
    ) {
        Some(id) => {
            let episodes = match episode.episode_number_range() {
                Some((a, b)) if b > a => format!("{a}-{b}"),
                Some((a, _)) => a.to_string(),
                None => "-".into(),
            };
            println!("anime {id} episode {episodes}");
            ExitCode::SUCCESS
        }
        None => {
            println!("no match for title {title:?}");
            ExitCode::FAILURE
        }
    }
}
