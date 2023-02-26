use std::fmt::Write;
use std::fs;
use std::{collections::HashMap, io::Read, path::Path};

use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use time::{
    ext::NumericalDuration, macros::format_description, Duration, OffsetDateTime,
    PrimitiveDateTime, UtcOffset,
};

const SONG_PLAY_DATA_PATH: &str =
    r#"C:\Program Files (x86)\Steam\steamapps\common\Beat Saber\UserData\SongPlayData.json"#;
const SONG_HASH_DATA_PATH: &str = r#"C:\Program Files (x86)\Steam\steamapps\common\Beat Saber\UserData\SongCore\SongHashData.dat"#;
const SONG_DURATION_CACHE_PATH: &str = r#"C:\Program Files (x86)\Steam\steamapps\common\Beat Saber\UserData\SongCore\SongDurationCache.dat"#;

const VIDEOS_FOLDER: &str = r#"C:\Users\jools\Videos\"#;
const SEGMENTS_FOLDER: &str = r#"C:\Users\jools\Videos\"#;

fn main() -> Result<()> {
    let song_core_data_cache: SongCoreDataCache =
        read_song_core_data_cache(SONG_HASH_DATA_PATH, SONG_DURATION_CACHE_PATH)?;

    let song_plays: Vec<SongPlay> = read_song_plays(SONG_PLAY_DATA_PATH)?;

    for path in fs::read_dir(VIDEOS_FOLDER)? {
        let path = path.unwrap();
        if !path.file_type().unwrap().is_file() {
            continue;
        }
        if path.path().extension() != Some("mkv".as_ref()) {
            continue;
        }
        let video_path = path.path();
        let output_path = Path::new(SEGMENTS_FOLDER).join(format!(
            "{}_segments.csv",
            video_path.file_stem().unwrap().to_string_lossy()
        ));
        match make_clip_cuts_csv(&song_core_data_cache, &song_plays, &video_path, output_path) {
            Ok(num_clips) => {
                println!(
                    "OK: {} : {num_clips:3} segments",
                    path.file_name().to_string_lossy()
                )
            }
            Err(err) => {
                eprintln!("FAIL: {} : {:?}", path.file_name().to_string_lossy(), err)
            }
        }
    }
    Ok(())
}

fn make_clip_cuts_csv(
    song_core_data_cache: &SongCoreDataCache,
    song_plays: &[SongPlay],
    video_path: &Path,
    output_path: std::path::PathBuf,
) -> Result<usize> {
    let (video_start, video_end) = read_video_timestamp_range(video_path)?;

    let clip_segments =
        find_clip_segments(video_start, video_end, &song_plays, &song_core_data_cache);
    let clip_segments_csv = clip_segments_to_csv(&clip_segments, &song_core_data_cache);

    std::io::Write::write_all(
        &mut std::fs::File::create(output_path)?,
        clip_segments_csv.as_bytes(),
    )?;

    Ok(clip_segments.len())
}

fn read_video_length(video_path: impl AsRef<Path>) -> Result<Duration> {
    use matroska::Matroska;
    use std::fs::File;
    let f = File::open(video_path)?;
    let matroska = Matroska::open(f)?;
    let duration: std::time::Duration = matroska.info.duration.context("no duration")?;
    duration.try_into().context("convert duration")
}

fn read_video_timestamp_range(
    video_path: impl AsRef<Path>,
) -> Result<(OffsetDateTime, OffsetDateTime)> {
    let video_filename_timestamp_format =
        format_description!("[year]-[month]-[day] [hour]-[minute]-[second]");
    let video_file_stem = video_path
        .as_ref()
        .file_stem()
        .context("stem")?
        .to_str()
        .context("to str")?;
    let start_timestamp =
        PrimitiveDateTime::parse(video_file_stem, video_filename_timestamp_format)?;
    let start_timestamp = start_timestamp.assume_offset(UtcOffset::current_local_offset()?);

    let video_length = read_video_length(video_path)?;

    Ok((start_timestamp, start_timestamp + video_length))
}

pub fn try_read_from_json_file<T: DeserializeOwned>(
    file_path: impl AsRef<std::path::Path>,
) -> anyhow::Result<T> {
    let mut s = String::new();
    std::fs::File::open(&file_path)?.read_to_string(&mut s)?;
    let t = serde_json::from_str(&s)?;
    Ok(t)
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongInfoDat {
    #[serde(rename = "_version")]
    pub version: String,
    #[serde(rename = "_songName")]
    pub song_name: String,
    #[serde(rename = "_songSubName")]
    pub song_sub_name: String,
    #[serde(rename = "_songAuthorName")]
    pub song_author_name: String,
    #[serde(rename = "_levelAuthorName")]
    pub level_author_name: String,
}

fn try_read_song_info(song_path: impl AsRef<Path>) -> anyhow::Result<SongInfoDat> {
    try_read_from_json_file(song_path.as_ref().join("info.dat"))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongPlay {
    pub timestamp: OffsetDateTime,
    pub song_hash: String,
    pub difficulty: i32,
    pub characteristic: String,
    pub raw_song_play: RawSongPlay,
}

fn read_song_plays(song_play_data_path: impl AsRef<Path>) -> Result<Vec<SongPlay>> {
    let raw_song_play_data: RawSongPlayData = try_read_from_json_file(song_play_data_path)?;

    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"custom_level_([0-9A-F]{40})___(\d)___([[:alnum:]]+)").unwrap();
    }

    let mut song_plays = vec![];
    for (song, plays) in raw_song_play_data.iter() {
        let Some(captures) = RE.captures(song) else {
            // println!("unrecognized song: {song}");
            continue;
        };
        let song_hash = captures.get(1).unwrap().as_str();
        let difficulty_str = captures.get(2).unwrap().as_str();
        let Ok(difficulty) = difficulty_str.parse() else {
            continue;
        };
        let characteristic = captures.get(3).unwrap().as_str();
        for play in plays.iter() {
            let timestamp = OffsetDateTime::from_unix_timestamp(play.date / 1_000).unwrap();
            song_plays.push(SongPlay {
                timestamp,
                song_hash: song_hash.to_string(),
                difficulty,
                characteristic: characteristic.to_string(),
                raw_song_play: play.clone(),
            });
        }
    }

    Ok(song_plays)
}

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
) -> Result<SongCoreDataCache> {
    let song_hash_data: JsonSongHashData = try_read_from_json_file(song_hash_data_path)?;
    let song_duration_cache: JsonSongDurationCache =
        try_read_from_json_file(song_duration_cache_path)?;

    let mut out = SongCoreDataCache::default();

    for (path, song_hash_element) in song_hash_data {
        let duration_cache_element = song_duration_cache[&path].clone();
        out.insert(
            song_hash_element.song_hash.clone(),
            SongCoreDataElement {
                path,
                directory_hash: song_hash_element.directory_hash,
                song_hash: song_hash_element.song_hash,
                id: duration_cache_element.id,
                duration: duration_cache_element.duration,
            },
        );
    }

    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipSegment {
    pub start: f64,
    pub end: f64,
    pub song_hash: String,
    pub song_play: SongPlay,
}

fn find_clip_segments(
    start_timestamp: OffsetDateTime,
    end_timestamp: OffsetDateTime,
    song_plays: &[SongPlay],
    song_core_data_cache: &SongCoreDataCache,
) -> Vec<ClipSegment> {
    let mut ordered_plays = vec![];
    for play in song_plays.iter() {
        if play.timestamp >= start_timestamp && play.timestamp <= end_timestamp {
            ordered_plays.push((play.timestamp, play.clone()));
        }
    }
    ordered_plays.sort_by_key(|(timestamp, _)| *timestamp);

    let mut segments = vec![];
    for (play_timestamp, song_play) in ordered_plays.iter() {
        let song_data = &song_core_data_cache[&song_play.song_hash];
        let start =
            ((*play_timestamp - song_data.duration.seconds()) - start_timestamp).as_seconds_f64();
        let end = (*play_timestamp - start_timestamp).as_seconds_f64();
        segments.push(ClipSegment {
            start,
            end,
            song_hash: song_play.song_hash.clone(),
            song_play: song_play.clone(),
        });
    }
    // fixup segment start time for failed maps (since we don't know the actual start time of the plays, just the end time)
    for i in 1..segments.len() {
        if segments[i].start < segments[i - 1].end {
            // assume one second buffer time
            segments[i].start = segments[i - 1].end + 1.0;
        }
    }

    segments
}

fn clip_segments_to_csv(
    clip_segments: &[ClipSegment],
    song_core_data_cache: &SongCoreDataCache,
) -> String {
    let mut out = String::new();
    for segment in clip_segments.iter() {
        let song_info_str = get_song_info_str(&segment.song_play, song_core_data_cache);
        let song_info_str = format!("\"{}\"", song_info_str.replace("\"", "\"\""));
        writeln!(
            &mut out,
            "{},{},{}",
            segment.start, segment.end, song_info_str
        )
        .unwrap();
    }
    out
}

fn get_song_info_str(song_play: &SongPlay, song_core_data_cache: &SongCoreDataCache) -> String {
    let song_hash = &song_play.song_hash;
    let Some(song_core_data) = song_core_data_cache.get(song_hash) else {
        return song_hash.to_string();
    };
    let path = &song_core_data.path;
    let Ok(song_info) = try_read_song_info(path) else {
        return song_hash.to_string();
    };
    let difficulty = match song_play.difficulty {
        0 => "E",
        1 => "M",
        2 => "H",
        3 => "E",
        4 => "Ep",
        _ => "?",
    };
    let if_fail = if song_play.raw_song_play.last_note == -1 {
        ""
    } else {
        "[F] "
    };
    format!(
        "{if_fail}{} - {difficulty} [{}] {}",
        song_info.song_name, song_info.song_author_name, song_info.level_author_name
    )
}
