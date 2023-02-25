use std::{collections::HashMap, fs, io::Read, path::Path};

use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use time::{
    macros::format_description, Duration, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset,
};

const LATEST_LOG_PATH: &str =
    r#"C:\Program Files (x86)\Steam\steamapps\common\Beat Saber\Logs\_latest.log"#;
const SONG_PLAY_DATA_PATH: &str =
    r#"C:\Program Files (x86)\Steam\steamapps\common\Beat Saber\UserData\SongPlayData.json"#;
const COMBINED_SCRAPPED_DATA_PATH: &str = r#"C:\Users\jools\Documents\GitHub\BeatSaberScrappedData\combinedScrappedData\combinedScrappedData.json"#;
const SONG_HASH_DATA_PATH: &str = r#"C:\Program Files (x86)\Steam\steamapps\common\Beat Saber\UserData\SongCore\SongHashData.dat"#;
const SONG_DURATION_CACHE_PATH: &str = r#"C:\Program Files (x86)\Steam\steamapps\common\Beat Saber\UserData\SongCore\SongDurationCache.dat"#;

fn main() {
    println!("Hello, world!");

    let video_path = r#"C:\Users\jools\Videos\2023-02-24 20-02-23.mkv"#;
    let (video_start, video_end) = read_video_timestamp_range(video_path);
    dbg!(video_start, video_end);

    // let combined_scrapped_data = read_beatsaber_combined_scrapped_data(COMBINED_SCRAPPED_DATA_PATH);
    // dbg!(combined_scrapped_data.len());

    let song_core_data_cache =
        read_song_core_data_cache(SONG_HASH_DATA_PATH, SONG_DURATION_CACHE_PATH);
    // dbg!(song_core_data_cache.len());

    let raw_song_play_data = read_raw_song_play_data(SONG_PLAY_DATA_PATH);
    // dbg!(song_play_data.len());

    find_clip_segments(
        video_start,
        video_end,
        &raw_song_play_data,
        &song_core_data_cache,
    );

    // read_log_file(LATEST_LOG_PATH);
}

// // region: get info about video file

fn read_video_length(video_path: impl AsRef<Path>) -> Duration {
    use matroska::Matroska;
    use std::fs::File;
    let f = File::open(video_path).unwrap();
    let matroska = Matroska::open(f).unwrap();
    matroska.info.duration.unwrap().try_into().unwrap()
}

fn read_video_timestamp_range(video_path: impl AsRef<Path>) -> (OffsetDateTime, OffsetDateTime) {
    let video_filename_timestamp_format =
        format_description!("[year]-[month]-[day] [hour]-[minute]-[second]");
    let video_file_stem = video_path.as_ref().file_stem().unwrap().to_str().unwrap();
    let start_timestamp =
        PrimitiveDateTime::parse(video_file_stem, video_filename_timestamp_format).unwrap();
    let start_timestamp = start_timestamp.assume_offset(UtcOffset::current_local_offset().unwrap());

    let video_length = read_video_length(video_path);

    (start_timestamp, start_timestamp + video_length)
}

// // endregion

fn read_log_file(log_file_path: impl AsRef<Path>) {
    let metadata = std::fs::metadata(&log_file_path).unwrap();
    let log_created_timestamp = OffsetDateTime::from(metadata.created().unwrap());
    let local_offset = UtcOffset::local_offset_at(log_created_timestamp).unwrap();
    // dbg!(log_created_timestamp.to_offset(local_offset));
    let log_created_timestamp = log_created_timestamp.to_offset(local_offset);

    let file_content = fs::read_to_string(&log_file_path).unwrap();
    let lines = file_content.lines().collect_vec();

    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"\[[A-Z]+ @ (?P<time>\d\d:\d\d:\d\d) \| [a-zA-Z0-9/_\.\- ]+\]").unwrap();
    }
    for line in lines.iter() {
        match get_log_line_time(&line) {
            Some(_) => {}
            None => {
                println!("line didn't match: {line}");
            }
        }
    }

    let start_time = get_log_line_time(lines[0]).unwrap();
    let end_time = get_log_line_time(lines[lines.len() - 1]).unwrap();
    println!("start_time: {:?}", start_time);
    println!("end_time: {:?}", end_time);

    let start_timestamp = log_created_timestamp.replace_time(start_time);
    let end_timestamp = log_created_timestamp.replace_time(end_time);
    println!("start_timestamp: {:?}", start_timestamp);
    println!("end_timestamp: {:?}", end_timestamp);

    let song_play_data = read_raw_song_play_data(SONG_PLAY_DATA_PATH);
    dbg!(song_play_data.len());
    let mut ordered_plays = vec![];
    for (song, plays) in song_play_data.iter() {
        for play in plays.iter() {
            let play_timestamp =
                // OffsetDateTime::from_unix_timestamp_nanos((play.date as i128) * 1_000_000).unwrap();
                OffsetDateTime::from_unix_timestamp(play.date / 1_000).unwrap();
            if play_timestamp >= start_timestamp && play_timestamp <= end_timestamp {
                ordered_plays.push((play_timestamp, song.clone()));
            }
        }
    }
    ordered_plays.sort();
    for (play_timestamp, song) in ordered_plays.iter() {
        println!("{} {}", play_timestamp, song);
    }
}

fn get_log_line_time(line: &str) -> Option<Time> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"\[[A-Z]+ @ (?P<time>\d\d:\d\d:\d\d) \| [a-zA-Z0-9/_\.\- ]+\]").unwrap();
    }
    match RE.captures(&line) {
        Some(captures) => {
            let time_str = &captures.name("time")?.as_str();
            let format = format_description!("[hour]:[minute]:[second]");
            let time = Time::parse(&time_str, &format).ok()?;
            Some(time)
        }
        None => None,
    }
}

// // region: JSON stuff

pub fn read_from_json_file<T: DeserializeOwned>(file_path: impl AsRef<std::path::Path>) -> T {
    let mut s = String::new();
    std::fs::File::open(&file_path)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    serde_json::from_str(&s).unwrap()
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSongPlay {
    #[serde(rename = "Date")]
    pub date: i64,
    #[serde(rename = "ModifiedScore")]
    pub modified_score: i64,
    #[serde(rename = "RawScore")]
    pub raw_score: i64,
    #[serde(rename = "LastNote")]
    pub last_note: i64,
    #[serde(rename = "Param")]
    pub param: i64,
}

pub type RawSongPlayData = HashMap<String, Vec<RawSongPlay>>;

fn read_raw_song_play_data(song_play_data_path: impl AsRef<Path>) -> RawSongPlayData {
    read_from_json_file(song_play_data_path)
}

// #[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
// pub struct RawCombinedScrappedDataElement {
//     #[serde(rename = "Key")]
//     pub key: String,
//     #[serde(rename = "Hash")]
//     pub hash: String,
//     #[serde(rename = "SongName")]
//     pub song_name: String,
//     #[serde(rename = "SongSubName")]
//     pub song_sub_name: String,
//     #[serde(rename = "SongAuthorName")]
//     pub song_author_name: String,
//     #[serde(rename = "LevelAuthorName")]
//     pub level_author_name: String,
//     // TODO
//     // #[serde(rename = "Diffs")]
//     // pub diffs: String,
//     // #[serde(rename = "Chars")]
//     // pub chars: String,
//     #[serde(rename = "Uploaded")]
//     pub uploaded: String,
//     #[serde(rename = "Uploader")]
//     pub uploader: String,
//     #[serde(rename = "Bpm")]
//     pub bpm: f64,
//     #[serde(rename = "Upvotes")]
//     pub upvotes: i64,
//     #[serde(rename = "Downvotes")]
//     pub downvotes: i64,
//     #[serde(rename = "Duration")]
//     pub duration: i64,
// }
//
// pub type RawCombinedScrappedData = Vec<RawCombinedScrappedDataElement>;
//
// fn read_beatsaber_combined_scrapped_data(
//     combined_scrapped_data_path: impl AsRef<Path>,
// ) -> RawCombinedScrappedData {
//     read_from_json_file(combined_scrapped_data_path)
// }

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSongHashDataElement {
    #[serde(rename = "directoryHash")]
    pub directory_hash: i64,
    #[serde(rename = "songHash")]
    pub song_hash: String,
}

pub type JsonSongHashData = HashMap<String, JsonSongHashDataElement>;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSongDurationCacheElement {
    pub id: String,
    pub duration: f64,
}

pub type JsonSongDurationCache = HashMap<String, JsonSongDurationCacheElement>;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SongCoreDataElement {
    pub path: String,
    pub directory_hash: i64,
    pub song_hash: String,
    pub id: String,
    pub duration: f64,
}

pub type SongCoreDataCache = HashMap<String, SongCoreDataElement>;

fn read_song_core_data_cache(
    song_hash_data_path: impl AsRef<Path>,
    song_duration_cache_path: impl AsRef<Path>,
) -> SongCoreDataCache {
    let song_hash_data: JsonSongHashData = read_from_json_file(song_hash_data_path);
    let song_duration_cache: JsonSongDurationCache = read_from_json_file(song_duration_cache_path);

    let mut out = SongCoreDataCache::default();

    for (path, song_hash_element) in song_hash_data {
        let duration_cache_element = song_duration_cache[&path].clone();
        out.insert(
            path.clone(),
            SongCoreDataElement {
                path,
                directory_hash: song_hash_element.directory_hash,
                song_hash: song_hash_element.song_hash,
                id: duration_cache_element.id,
                duration: duration_cache_element.duration,
            },
        );
    }

    out
}

// // endregion

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClipSegment {
    pub begin: Duration,
    pub end: Duration,
    pub song_hash: String,
}

fn find_clip_segments(
    // video_path: impl AsRef<Path>,
    start_timestamp: OffsetDateTime,
    end_timestamp: OffsetDateTime,
    raw_song_play_data: &RawSongPlayData,
    song_core_data_cache: &SongCoreDataCache,
) -> Vec<ClipSegment> {
    let mut ordered_plays = vec![];
    for (song, plays) in raw_song_play_data.iter() {
        for play in plays.iter() {
            let play_timestamp =
                // OffsetDateTime::from_unix_timestamp_nanos((play.date as i128) * 1_000_000).unwrap();
                OffsetDateTime::from_unix_timestamp(play.date / 1_000).unwrap();
            if play_timestamp >= start_timestamp && play_timestamp <= end_timestamp {
                ordered_plays.push((play_timestamp, song.clone()));
            }
        }
    }
    ordered_plays.sort();
    for (play_timestamp, song) in ordered_plays.iter() {
        println!("{} {}", play_timestamp, song);
    }

    todo!()
}
