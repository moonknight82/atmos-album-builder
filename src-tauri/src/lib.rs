mod models;

use models::*;
use rusqlite::{params, Connection};
use serde_json::{Map, Value};
use std::hash::{Hash, Hasher};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
};
use tauri::Emitter;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
};
use uuid::Uuid;
use walkdir::WalkDir;

static CANCELLED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn cancelled() -> &'static Mutex<HashSet<String>> {
    CANCELLED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn app_data_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_local_dir()
        .ok_or("Cannot locate the application data directory")?
        .join("Atmos Album Builder");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn db() -> Result<Connection, String> {
    let connection = Connection::open(app_data_dir()?.join("queue.sqlite3"))
        .map_err(|e| format!("Could not open the review queue: {e}"))?;
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(connection)
}

#[tauri::command]
fn load_queue() -> Result<Vec<AlbumJob>, String> {
    let connection = db()?;
    let value: Result<String, _> =
        connection.query_row("SELECT value FROM app_state WHERE key='queue'", [], |row| {
            row.get(0)
        });
    match value {
        Ok(json) => serde_json::from_str(&json).map_err(|e| e.to_string()),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(vec![]),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
fn save_queue(albums: Vec<AlbumJob>) -> Result<(), String> {
    let connection = db()?;
    let json = serde_json::to_string(&albums).map_err(|e| e.to_string())?;
    connection
        .execute(
            "INSERT INTO app_state(key,value) VALUES('queue',?1) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![json],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn media_binary(name: &str) -> PathBuf {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            let bundled = directory.join(name);
            if bundled.exists() {
                return bundled;
            }
        }
    }
    PathBuf::from(name)
}

fn hidden(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .map(|relative| {
            relative
                .components()
                .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
        })
        .unwrap_or(false)
}

fn is_supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            extension.eq_ignore_ascii_case("m4a") || extension.eq_ignore_ascii_case("mka")
        })
        .unwrap_or(false)
}

fn image_attachment_extension(stream: &Value) -> Option<String> {
    if stream["codec_type"].as_str() != Some("attachment") {
        return None;
    }
    let empty = Map::new();
    let tags = stream["tags"].as_object().unwrap_or(&empty);
    let mime = tag(tags, &["mimetype", "mime_type"])
        .unwrap_or("")
        .to_ascii_lowercase();
    let filename = tag(tags, &["filename"]).unwrap_or("");
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let detected = match mime.as_str() {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/tiff" => Some("tiff"),
        "image/bmp" => Some("bmp"),
        _ => match extension.as_str() {
            "jpg" | "jpeg" => Some("jpg"),
            "png" => Some("png"),
            "webp" => Some("webp"),
            "tif" | "tiff" => Some("tiff"),
            "bmp" => Some("bmp"),
            _ => None,
        },
    }?;
    Some(detected.into())
}

fn natural_key(value: &str) -> Vec<String> {
    let mut parts = vec![];
    let mut buffer = String::new();
    let mut is_digit = None;
    for character in value.to_lowercase().chars() {
        let digit = character.is_ascii_digit();
        if is_digit.is_some() && is_digit != Some(digit) {
            parts.push(if is_digit == Some(true) {
                format!("{:020}", buffer.parse::<u64>().unwrap_or(0))
            } else {
                buffer.clone()
            });
            buffer.clear();
        }
        buffer.push(character);
        is_digit = Some(digit);
    }
    if !buffer.is_empty() {
        parts.push(if is_digit == Some(true) {
            format!("{:020}", buffer.parse::<u64>().unwrap_or(0))
        } else {
            buffer
        });
    }
    parts
}

fn tag<'a>(tags: &'a Map<String, Value>, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        tags.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn parse_number(value: Option<&str>) -> Option<u32> {
    value?.split('/').next()?.trim().parse::<u32>().ok()
}

fn parse_time_base(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    (denominator != 0.0).then_some(numerator / denominator)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .trim()
        .to_string()
}

async fn probe_track(path: &Path) -> Result<AlbumTrack, String> {
    let output = Command::new(media_binary("ffprobe"))
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-show_data",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .await
        .map_err(|error| format!("FFprobe is unavailable: {error}"))?;
    if !output.status.success() {
        return Err(format!("Could not inspect {}", path.display()));
    }
    let json: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    let streams = json["streams"]
        .as_array()
        .ok_or_else(|| format!("No streams found in {}", path.display()))?;
    let audio = streams
        .iter()
        .find(|stream| stream["codec_type"].as_str() == Some("audio"))
        .ok_or_else(|| format!("No audio stream found in {}", path.display()))?;
    let has_embedded_cover = streams.iter().any(|stream| {
        (stream["codec_type"].as_str() == Some("video")
            && stream["disposition"]["attached_pic"].as_i64() == Some(1))
            || image_attachment_extension(stream).is_some()
    });
    let empty = Map::new();
    let format_tags = json["format"]["tags"].as_object().unwrap_or(&empty);
    let stream_tags = audio["tags"].as_object().unwrap_or(&empty);
    let metadata = |names: &[&str]| {
        tag(format_tags, names)
            .or_else(|| tag(stream_tags, names))
            .unwrap_or("")
            .to_string()
    };
    let duration_ms = audio["duration_ts"]
        .as_i64()
        .and_then(|ticks| {
            audio["time_base"]
                .as_str()
                .and_then(parse_time_base)
                .map(|base| (ticks as f64 * base * 1000.0).round().max(0.0) as u64)
        })
        .or_else(|| {
            audio["duration"]
                .as_str()
                .and_then(|value| value.parse::<f64>().ok())
                .map(|seconds| (seconds * 1000.0).round().max(0.0) as u64)
        })
        .or_else(|| {
            json["format"]["duration"]
                .as_str()
                .and_then(|value| value.parse::<f64>().ok())
                .map(|seconds| (seconds * 1000.0).round().max(0.0) as u64)
        })
        .unwrap_or(0);
    let chapter_title = metadata(&["title"]);
    Ok(AlbumTrack {
        id: Uuid::new_v4().to_string(),
        path: path.to_string_lossy().into(),
        file_name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        duration_ms,
        chapter_title: if chapter_title.is_empty() {
            file_stem(path)
        } else {
            chapter_title
        },
        disc_number: parse_number(tag(format_tags, &["disc", "discnumber"])),
        track_number: parse_number(tag(format_tags, &["track", "tracknumber"])),
        codec: audio["codec_name"].as_str().unwrap_or("").to_string(),
        profile: audio["profile"].as_str().unwrap_or("").to_string(),
        sample_rate: audio["sample_rate"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        channels: audio["channels"].as_u64().unwrap_or(0) as u32,
        channel_layout: audio["channel_layout"].as_str().unwrap_or("").to_string(),
        codec_tag: audio["codec_tag_string"].as_str().unwrap_or("").to_string(),
        extradata_size: audio["extradata_size"].as_u64().unwrap_or(0),
        extradata_fingerprint: {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            audio["extradata"].as_str().unwrap_or("").hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        },
        album: metadata(&["album"]),
        album_artist: metadata(&["album_artist", "albumartist"]),
        artist: metadata(&["artist", "author"]),
        date: metadata(&["date", "year"]),
        genre: metadata(&["genre"]),
        comment: metadata(&["comment", "description"]),
        has_embedded_cover,
    })
}

fn consensus<F>(tracks: &[AlbumTrack], value: F) -> (String, bool)
where
    F: Fn(&AlbumTrack) -> &str,
{
    let values = tracks
        .iter()
        .map(&value)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    let Some(first) = values.first() else {
        return (String::new(), true);
    };
    let consistent = values.iter().all(|item| item.eq_ignore_ascii_case(first));
    ((*first).to_string(), consistent)
}

fn clean_file_component(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '\0' => ' ',
            _ => character,
        })
        .collect::<String>();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    cleaned.trim_matches(['.', ' ']).to_string()
}

fn default_output_filename(metadata: &AlbumMetadata, folder_name: &str) -> String {
    let title = if metadata.title.trim().is_empty() {
        folder_name.trim()
    } else {
        metadata.title.trim()
    };
    let stem = if metadata.artist.trim().is_empty() {
        title.to_string()
    } else {
        format!("{} - {}", metadata.artist.trim(), title)
    };
    let stem = clean_file_component(&stem);
    format!("{}.mkv", if stem.is_empty() { "Album" } else { &stem })
}

fn validate_output_filename(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err("Output filename must be a single valid filename".into());
    }
    let filename = if trimmed.to_ascii_lowercase().ends_with(".mkv") {
        trimmed.to_string()
    } else {
        format!("{trimmed}.mkv")
    };
    Ok(filename)
}

fn individual_output_filename(index: usize, total: usize, track: &AlbumTrack) -> String {
    let width = total.to_string().len().max(2);
    let title = clean_file_component(track.chapter_title.trim());
    let fallback = Path::new(&track.file_name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let title = if title.is_empty() {
        clean_file_component(&fallback)
    } else {
        title
    };
    format!(
        "{:0width$} - {}.mkv",
        index + 1,
        if title.is_empty() { "Track" } else { &title },
        width = width
    )
}

fn compatibility_issues(album: &AlbumJob) -> Vec<String> {
    let mut issues = vec![];
    if album.tracks.is_empty() {
        issues.push("No M4A or MKA tracks were found".into());
        return issues;
    }
    if album.tracks.iter().any(|track| track.duration_ms == 0) {
        issues.push("One or more track durations could not be read".into());
    }
    if album
        .tracks
        .iter()
        .any(|track| track.codec.trim().is_empty())
    {
        issues.push("One or more audio codecs could not be identified".into());
    }
    let signature = album.tracks[0].compatibility_signature();
    if album.export_mode != "individual"
        && album
            .tracks
            .iter()
            .skip(1)
            .any(|track| track.compatibility_signature() != signature)
    {
        issues.push(
            "The tracks do not share the same codec, sample rate, channel layout, and codec parameters; lossless concatenation is unsafe"
                .into(),
        );
    }
    if album.metadata.title.trim().is_empty() {
        issues.push("Album title is required".into());
    }
    if album.metadata.artist.trim().is_empty() {
        issues.push("Artist is required".into());
    }
    if album
        .tracks
        .iter()
        .any(|track| track.chapter_title.trim().is_empty())
    {
        issues.push("Every track needs a chapter title".into());
    }
    let visual_ok = if album.visual_mode == "video" {
        album
            .video_path
            .as_ref()
            .map(Path::new)
            .map(Path::is_file)
            .unwrap_or(false)
    } else {
        album
            .image_path
            .as_ref()
            .map(Path::new)
            .map(Path::is_file)
            .unwrap_or(false)
    };
    if !visual_ok {
        issues.push(
            if album.visual_mode == "video" {
                "Choose a usable MP4 or MOV animation"
            } else {
                "Choose a usable image for the video track"
            }
            .into(),
        );
    }
    if album.export_mode != "individual"
        && validate_output_filename(&album.output_file_name).is_err()
    {
        issues.push("Choose a valid MKV filename without folder separators".into());
    }
    issues
}

fn destination_directory(album: &AlbumJob) -> Result<PathBuf, String> {
    let directory = album
        .destination_override
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&album.source_folder);
    let directory = PathBuf::from(directory);
    if !directory.is_absolute() {
        return Err("Destination folder must be an absolute path".into());
    }
    Ok(directory)
}

fn destinations_for(album: &AlbumJob) -> Result<Vec<PathBuf>, String> {
    let directory = destination_directory(album)?;
    if album.export_mode == "individual" {
        Ok(album
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                directory.join(individual_output_filename(index, album.tracks.len(), track))
            })
            .collect())
    } else {
        Ok(vec![
            directory.join(validate_output_filename(&album.output_file_name)?)
        ])
    }
}

fn cover_cache_path(album_id: &str) -> Result<PathBuf, String> {
    let directory = app_data_dir()?.join("covers");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    Ok(directory.join(format!("{album_id}.jpg")))
}

async fn extract_cover(track: &AlbumTrack, album_id: &str) -> Result<PathBuf, String> {
    let target = cover_cache_path(album_id)?;
    let _ = fs::remove_file(&target);
    let output = Command::new(media_binary("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&track.path)
        .args(["-map", "0:v:0", "-frames:v", "1"])
        .arg(&target)
        .output()
        .await
        .map_err(|error| format!("FFmpeg is unavailable: {error}"))?;
    if output.status.success() && target.is_file() {
        return Ok(target);
    }

    let probe = Command::new(media_binary("ffprobe"))
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(&track.path)
        .output()
        .await
        .map_err(|error| format!("FFprobe is unavailable: {error}"))?;
    if probe.status.success() {
        let json: Value = serde_json::from_slice(&probe.stdout).map_err(|e| e.to_string())?;
        if let Some((stream_index, extension)) = json["streams"].as_array().and_then(|streams| {
            streams.iter().find_map(|stream| {
                Some((
                    stream["index"].as_u64()?,
                    image_attachment_extension(stream)?,
                ))
            })
        }) {
            let attachment =
                target.with_file_name(format!("{album_id}-attachment-{stream_index}.{extension}"));
            let dump_option = format!("-dump_attachment:{stream_index}");
            let dumped = Command::new(media_binary("ffmpeg"))
                .args(["-hide_banner", "-loglevel", "error", "-y"])
                .arg(dump_option)
                .arg(&attachment)
                .args(["-i"])
                .arg(&track.path)
                .args(["-map", "0:a:0", "-c:a", "copy", "-f", "streamhash", "-"])
                .output()
                .await
                .map_err(|error| format!("FFmpeg is unavailable: {error}"))?;
            if dumped.status.success() && attachment.is_file() {
                let converted = Command::new(media_binary("ffmpeg"))
                    .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                    .arg(&attachment)
                    .args(["-frames:v", "1"])
                    .arg(&target)
                    .output()
                    .await
                    .map_err(|error| format!("FFmpeg is unavailable: {error}"))?;
                let _ = fs::remove_file(&attachment);
                if converted.status.success() && target.is_file() {
                    return Ok(target);
                }
            }
            let _ = fs::remove_file(&attachment);
        }
    }
    Err(format!(
        "Could not extract embedded artwork from {}",
        track.file_name
    ))
}

async fn image_is_readable(path: &Path) -> bool {
    Command::new(media_binary("ffprobe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .await
        .map(|output| {
            output.status.success()
                && serde_json::from_slice::<Value>(&output.stdout)
                    .ok()
                    .and_then(|value| value["streams"].as_array().map(|items| !items.is_empty()))
                    .unwrap_or(false)
        })
        .unwrap_or(false)
}

async fn video_is_readable(path: &Path) -> bool {
    Command::new(media_binary("ffprobe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,codec_name:format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .await
        .map(|output| {
            if !output.status.success() {
                return false;
            }
            serde_json::from_slice::<Value>(&output.stdout)
                .ok()
                .map(|value| {
                    let has_video = value["streams"]
                        .as_array()
                        .map(|items| !items.is_empty())
                        .unwrap_or(false);
                    let duration = value["format"]["duration"]
                        .as_str()
                        .and_then(|item| item.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    has_video && duration > 0.0
                })
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

async fn refresh_album(mut album: AlbumJob, refresh_embedded_cover: bool) -> AlbumJob {
    if album.visual_mode != "video" {
        album.visual_mode = "image".into();
    }
    if album.export_mode != "individual" {
        album.export_mode = "album".into();
    }
    album.total_duration_ms = album.tracks.iter().map(|track| track.duration_ms).sum();
    if !album.file_name_overridden {
        album.output_file_name = default_output_filename(&album.metadata, &album.folder_name);
    }
    if refresh_embedded_cover && album.image_source == "embedded" {
        let first = album.tracks.first();
        if let Some(track) = first.filter(|track| track.has_embedded_cover) {
            match extract_cover(track, &album.id).await {
                Ok(path) => album.image_path = Some(path.to_string_lossy().into()),
                Err(error) => {
                    album.image_path = None;
                    album.warnings.push(error);
                }
            }
        } else {
            album.image_path = None;
        }
    }
    album.blocking_issues = compatibility_issues(&album);
    if album.visual_mode == "video" {
        if let Some(video) = album.video_path.as_deref().map(Path::new) {
            if video.is_file()
                && !video_is_readable(video).await
                && !album
                    .blocking_issues
                    .iter()
                    .any(|issue| issue.contains("MP4 or MOV"))
            {
                album
                    .blocking_issues
                    .push("The selected animation cannot be decoded by FFmpeg".into());
            }
        }
    } else if let Some(image) = album.image_path.as_deref().map(Path::new) {
        if image.is_file()
            && !image_is_readable(image).await
            && !album
                .blocking_issues
                .iter()
                .any(|issue| issue.contains("usable image"))
        {
            album
                .blocking_issues
                .push("The selected image cannot be decoded by FFmpeg".into());
        }
    }
    if !album.blocking_issues.is_empty() {
        album.approved = false;
        album.status = "needsReview".into();
    } else if album.approved && album.status != "completed" {
        album.status = "ready".into();
    }
    album
}

#[tauri::command]
async fn scan_root(app: tauri::AppHandle, root: String) -> Result<Vec<AlbumJob>, String> {
    let root = PathBuf::from(root);
    if !root.is_absolute() || !root.is_dir() {
        return Err("Choose a readable root folder".into());
    }
    let mut grouped: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for entry in WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file()
            && !hidden(&root, entry.path())
            && is_supported_audio(entry.path())
        {
            if let Some(parent) = entry.path().parent() {
                grouped
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(entry.path().to_path_buf());
            }
        }
    }
    let mut folders = grouped.into_iter().collect::<Vec<_>>();
    folders.sort_by(|(a, _), (b, _)| {
        natural_key(&a.to_string_lossy()).cmp(&natural_key(&b.to_string_lossy()))
    });
    let folder_count = folders.len();
    emit_progress(
        &app,
        ExportProgress {
            album_id: "scan".into(),
            stage: "scanning".into(),
            percent: 0,
            message: format!("Found {folder_count} album folders; reading metadata…"),
        },
    );
    let mut albums = vec![];
    for (folder_index, (folder, files)) in folders.into_iter().enumerate() {
        let percent = if folder_count == 0 {
            100
        } else {
            folder_index as u64 * 100 / folder_count as u64
        };
        emit_progress(
            &app,
            ExportProgress {
                album_id: "scan".into(),
                stage: "scanning".into(),
                percent,
                message: format!(
                    "Reading album {} of {}: {}",
                    folder_index + 1,
                    folder_count,
                    folder.file_name().unwrap_or_default().to_string_lossy()
                ),
            },
        );
        let mut tracks = vec![];
        let mut probe_warnings = vec![];
        for file in files {
            match probe_track(&file).await {
                Ok(track) => tracks.push(track),
                Err(error) => probe_warnings.push(error),
            }
        }
        tracks.sort_by(|a, b| {
            (
                a.disc_number.unwrap_or(u32::MAX),
                a.track_number.unwrap_or(u32::MAX),
                natural_key(&a.file_name),
            )
                .cmp(&(
                    b.disc_number.unwrap_or(u32::MAX),
                    b.track_number.unwrap_or(u32::MAX),
                    natural_key(&b.file_name),
                ))
        });
        if tracks.is_empty() {
            continue;
        }
        let folder_name = folder
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let (album_title, title_consistent) = consensus(&tracks, |track| &track.album);
        let (album_artist, album_artist_consistent) =
            consensus(&tracks, |track| &track.album_artist);
        let (track_artist, artist_consistent) = consensus(&tracks, |track| &track.artist);
        let (date, date_consistent) = consensus(&tracks, |track| &track.date);
        let (genre, genre_consistent) = consensus(&tracks, |track| &track.genre);
        let (comment, _) = consensus(&tracks, |track| &track.comment);
        let creator_consistent = if album_artist.is_empty() {
            artist_consistent
        } else {
            album_artist_consistent
        };
        let metadata = AlbumMetadata {
            title: if album_title.is_empty() {
                folder_name.clone()
            } else {
                album_title
            },
            artist: if album_artist.is_empty() {
                track_artist
            } else {
                album_artist
            },
            date,
            genre,
            comment,
        };
        let id = Uuid::new_v4().to_string();
        let mut warnings = probe_warnings;
        for (consistent, field) in [
            (title_consistent, "album title"),
            (creator_consistent, "artist"),
            (date_consistent, "date"),
            (genre_consistent, "genre"),
        ] {
            if !consistent {
                warnings.push(format!(
                    "Source files disagree about {field}; the first available value was used"
                ));
            }
        }
        let mut album = AlbumJob {
            id,
            source_folder: folder.to_string_lossy().into(),
            folder_name,
            tracks,
            output_file_name: String::new(),
            export_mode: "album".into(),
            metadata,
            image_path: None,
            image_source: "embedded".into(),
            visual_mode: "image".into(),
            video_path: None,
            file_name_overridden: false,
            destination_override: None,
            approved: false,
            status: "needsReview".into(),
            warnings,
            blocking_issues: vec![],
            total_duration_ms: 0,
            last_error: None,
        };
        album = refresh_album(album, true).await;
        albums.push(album);
    }
    emit_progress(
        &app,
        ExportProgress {
            album_id: "scan".into(),
            stage: "completed".into(),
            percent: 100,
            message: format!("Finished scanning {} albums", albums.len()),
        },
    );
    Ok(albums)
}

#[tauri::command]
async fn inspect_album(album: AlbumJob) -> Result<AlbumJob, String> {
    Ok(refresh_album(album, true).await)
}

#[tauri::command]
fn preview_output_path(album: AlbumJob) -> Result<OutputPreview, String> {
    let paths = destinations_for(&album)?;
    let conflict_count = paths.iter().filter(|path| path.exists()).count();
    let display_path = if album.export_mode == "individual" {
        destination_directory(&album)?.to_string_lossy().into()
    } else {
        paths
            .first()
            .map(|path| path.to_string_lossy().into())
            .unwrap_or_default()
    };
    Ok(OutputPreview {
        exists: conflict_count > 0,
        path: display_path,
        paths: paths
            .into_iter()
            .map(|path| path.to_string_lossy().into())
            .collect(),
        conflict_count,
    })
}

fn concat_escape(path: &str) -> String {
    path.replace('\\', "\\\\").replace('\'', "'\\''")
}

fn ffmetadata_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('=', "\\=")
        .replace(';', "\\;")
        .replace('#', "\\#")
}

fn build_ffmetadata(album: &AlbumJob, file_title: Option<&str>, include_chapters: bool) -> String {
    let mut metadata = String::from(";FFMETADATA1\n");
    for (key, value) in [
        ("title", file_title.unwrap_or(album.metadata.title.as_str())),
        ("album", album.metadata.title.as_str()),
        ("artist", album.metadata.artist.as_str()),
        ("album_artist", album.metadata.artist.as_str()),
        ("author", album.metadata.artist.as_str()),
        ("date", album.metadata.date.as_str()),
        ("year", album.metadata.date.as_str()),
        ("genre", album.metadata.genre.as_str()),
        ("comment", album.metadata.comment.as_str()),
    ] {
        if !value.trim().is_empty() {
            metadata.push_str(&format!("{key}={}\n", ffmetadata_escape(value.trim())));
        }
    }
    if include_chapters {
        let mut start = 0_u64;
        for track in &album.tracks {
            let end = start.saturating_add(track.duration_ms);
            metadata.push_str(&format!(
                "[CHAPTER]\nTIMEBASE=1/1000\nSTART={start}\nEND={end}\ntitle={}\n",
                ffmetadata_escape(track.chapter_title.trim())
            ));
            start = end;
        }
    }
    metadata
}

fn emit_progress(app: &tauri::AppHandle, progress: ExportProgress) {
    let _ = app.emit("export-progress", progress);
}

fn parse_stream_hash(value: &str) -> Result<String, String> {
    value
        .lines()
        .find_map(|line| {
            line.split_once('=')
                .map(|(_, hash)| hash.trim().to_string())
        })
        .filter(|hash| !hash.is_empty())
        .ok_or_else(|| "FFmpeg did not return an audio packet hash".into())
}

async fn stream_hash_for_concat(concat: &Path) -> Result<String, String> {
    let output = Command::new(media_binary("ffmpeg"))
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(concat)
        .args(["-map", "0:a:0", "-c:a", "copy", "-f", "streamhash", "-"])
        .output()
        .await
        .map_err(|e| format!("Could not hash source audio: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not hash source audio: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    parse_stream_hash(&String::from_utf8_lossy(&output.stdout))
}

async fn stream_hash_for_output(path: &Path) -> Result<String, String> {
    let output = Command::new(media_binary("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-c:a", "copy", "-f", "streamhash", "-"])
        .output()
        .await
        .map_err(|e| format!("Could not hash output audio: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not hash output audio: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    parse_stream_hash(&String::from_utf8_lossy(&output.stdout))
}

async fn probe_output(path: &Path) -> Result<Value, String> {
    let output = Command::new(media_binary("ffprobe"))
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-show_chapters",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .await
        .map_err(|e| format!("Could not verify output: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Output verification failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}

fn verify_output(
    album: &AlbumJob,
    probe: &Value,
    expected_title: &str,
    expected_chapters: usize,
) -> Result<(), String> {
    let streams = probe["streams"]
        .as_array()
        .ok_or("The MKV contains no readable streams")?;
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"].as_str() == Some("video"))
        .ok_or("The MKV is missing its video track")?;
    if video["codec_name"].as_str() != Some("h264")
        || video["width"].as_u64() != Some(1920)
        || video["height"].as_u64() != Some(1080)
    {
        return Err("The static video track is not 1920×1080 H.264".into());
    }
    let audio = streams
        .iter()
        .find(|stream| stream["codec_type"].as_str() == Some("audio"))
        .ok_or("The MKV is missing its audio track")?;
    let source = &album.tracks[0];
    if audio["codec_name"].as_str().unwrap_or("") != source.codec
        || audio["sample_rate"]
            .as_str()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0)
            != source.sample_rate
        || audio["channels"].as_u64().unwrap_or(0) as u32 != source.channels
    {
        return Err("The output audio properties do not match the source".into());
    }
    let chapter_count = probe["chapters"].as_array().map(Vec::len).unwrap_or(0);
    if chapter_count != expected_chapters {
        return Err(format!(
            "Expected {expected_chapters} chapters but found {chapter_count}"
        ));
    }
    let empty = Map::new();
    let tags = probe["format"]["tags"].as_object().unwrap_or(&empty);
    let title = tag(tags, &["title", "album"]).unwrap_or("");
    if title != expected_title {
        return Err("The output title metadata does not match the review form".into());
    }
    Ok(())
}

#[tauri::command]
async fn cancel_export(album_id: String) {
    cancelled().lock().unwrap().insert(album_id);
}

struct ExportFilePlan {
    output: PathBuf,
    replace: bool,
    file_index: usize,
    file_count: usize,
    label: String,
    file_title: Option<String>,
    include_chapters: bool,
}

async fn export_album_file(
    app: &tauri::AppHandle,
    album: AlbumJob,
    plan: ExportFilePlan,
) -> Result<(), String> {
    let ExportFilePlan {
        output,
        replace,
        file_index,
        file_count,
        label,
        file_title,
        include_chapters,
    } = plan;
    let scaled =
        |local: u64| ((file_index as u64 * 100 + local) / file_count.max(1) as u64).min(99);
    let progress_message = |message: &str| {
        if label.is_empty() {
            message.to_string()
        } else {
            format!("{label} — {message}")
        }
    };
    if output.exists() && !replace {
        return Err(format!(
            "CONFLICT: Destination already exists: {}",
            output.display()
        ));
    }
    let destination = output
        .parent()
        .ok_or("Could not determine the destination folder")?;
    fs::create_dir_all(destination).map_err(|e| format!("Could not create destination: {e}"))?;
    let work = std::env::temp_dir().join(format!("atmos-album-{}", Uuid::new_v4()));
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let concat = work.join("inputs.txt");
    let metadata = work.join("metadata.txt");
    fs::write(
        &concat,
        album
            .tracks
            .iter()
            .map(|track| format!("file '{}'\n", concat_escape(&track.path)))
            .collect::<String>(),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        &metadata,
        build_ffmetadata(&album, file_title.as_deref(), include_chapters),
    )
    .map_err(|e| e.to_string())?;
    let visual = if album.visual_mode == "video" {
        album.video_path.as_ref()
    } else {
        album.image_path.as_ref()
    }
    .map(PathBuf::from)
    .filter(|path| path.is_file())
    .ok_or("The selected visual is no longer available")?;
    let output_name = output.file_name().unwrap_or_default().to_string_lossy();
    let temporary = destination.join(format!(".{output_name}.{}.tmp.mkv", Uuid::new_v4()));
    emit_progress(
        app,
        ExportProgress {
            album_id: album.id.clone(),
            stage: "preflight".into(),
            percent: scaled(3),
            message: progress_message("Checking source audio"),
        },
    );
    let source_hash = match stream_hash_for_concat(&concat).await {
        Ok(hash) => hash,
        Err(error) => {
            let _ = fs::remove_dir_all(&work);
            return Err(error);
        }
    };
    emit_progress(
        app,
        ExportProgress {
            album_id: album.id.clone(),
            stage: "muxing".into(),
            percent: scaled(8),
            message: progress_message("Building MKV without re-encoding audio"),
        },
    );
    let mut command = Command::new(media_binary("ffmpeg"));
    command.args(["-hide_banner", "-loglevel", "error"]);
    if album.visual_mode == "video" {
        command.args(["-stream_loop", "-1", "-i"]);
    } else {
        command.args(["-loop", "1", "-framerate", "1", "-i"]);
    }
    command
        .arg(&visual)
        .args(["-f", "concat", "-safe", "0", "-i"])
        .arg(&concat)
        .args(["-i"])
        .arg(&metadata)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-map_metadata",
            "2",
            "-map_chapters",
            "2",
            "-vf",
            "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2:black,setsar=1",
            "-c:v",
            "h264_videotoolbox",
            "-q:v",
            "65",
            "-allow_sw",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "copy",
            "-shortest",
            "-progress",
            "pipe:1",
            "-nostats",
            "-f",
            "matroska",
            "-y",
        ])
        .args(if album.visual_mode == "video" {
            Vec::<&str>::new()
        } else {
            vec!["-r", "1"]
        })
        .arg(&temporary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_dir_all(&work);
            return Err(format!("FFmpeg is unavailable: {error}"));
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or("Could not read FFmpeg progress")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut stderr = child.stderr.take().ok_or("Could not read FFmpeg errors")?;
    let stderr_task = tokio::spawn(async move {
        let mut bytes = vec![];
        let _ = stderr.read_to_end(&mut bytes).await;
        bytes
    });
    let mut last_percent = 8_u64;
    loop {
        tokio::select! {
            line = lines.next_line() => match line.map_err(|e| e.to_string())? {
                Some(line) => {
                    let elapsed = line
                        .strip_prefix("out_time_us=")
                        .or_else(|| line.strip_prefix("out_time_ms="));
                    if let Some(raw) = elapsed {
                        if let Ok(microseconds) = raw.parse::<u64>() {
                            let total = album.total_duration_ms.saturating_mul(1000).max(1);
                            let percent = (8 + microseconds.saturating_mul(77) / total).min(85);
                            if percent > last_percent {
                                last_percent = percent;
                                emit_progress(app, ExportProgress {
                                    album_id: album.id.clone(),
                                    stage: "muxing".into(),
                                    percent: scaled(percent),
                                    message: progress_message(&format!("Building MKV — {percent}%")),
                                });
                            }
                        }
                    }
                }
                None => break,
            },
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
                if cancelled().lock().unwrap().remove(&album.id) {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = fs::remove_file(&temporary);
                    let _ = fs::remove_dir_all(&work);
                    return Err("Export cancelled".into());
                }
            }
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let stderr = stderr_task.await.unwrap_or_default();
    if !status.success() {
        let _ = fs::remove_file(&temporary);
        let _ = fs::remove_dir_all(&work);
        return Err(format!(
            "FFmpeg failed: {}",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    emit_progress(
        app,
        ExportProgress {
            album_id: album.id.clone(),
            stage: "verifying".into(),
            percent: scaled(88),
            message: progress_message("Verifying audio, artwork, and metadata"),
        },
    );
    let verification = async {
        let output_hash = stream_hash_for_output(&temporary).await?;
        if output_hash != source_hash {
            return Err("Audio packet verification failed; the destination was not changed".into());
        }
        let probe = probe_output(&temporary).await?;
        verify_output(
            &album,
            &probe,
            file_title.as_deref().unwrap_or(&album.metadata.title),
            if include_chapters {
                album.tracks.len()
            } else {
                0
            },
        )
    }
    .await;
    if let Err(error) = verification {
        let _ = fs::remove_file(&temporary);
        let _ = fs::remove_dir_all(&work);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, &output) {
        let _ = fs::remove_file(&temporary);
        let _ = fs::remove_dir_all(&work);
        return Err(format!("Could not commit the verified MKV: {error}"));
    }
    let _ = fs::remove_dir_all(&work);
    Ok(())
}

#[tauri::command]
async fn export_album(
    app: tauri::AppHandle,
    album: AlbumJob,
    replace: bool,
) -> Result<String, String> {
    let album = refresh_album(album, true).await;
    if !album.approved {
        return Err("Approve this album before exporting it".into());
    }
    if !album.blocking_issues.is_empty() {
        return Err(album.blocking_issues.join("; "));
    }
    let outputs = destinations_for(&album)?;
    if !replace {
        if let Some(output) = outputs.iter().find(|output| output.exists()) {
            return Err(format!(
                "CONFLICT: Destination already exists: {}",
                output.display()
            ));
        }
    }
    cancelled().lock().unwrap().remove(&album.id);
    if album.export_mode == "individual" {
        for (index, (track, output)) in album.tracks.iter().zip(outputs.iter()).enumerate() {
            if cancelled().lock().unwrap().remove(&album.id) {
                return Err("Export cancelled".into());
            }
            let mut single = album.clone();
            single.tracks = vec![track.clone()];
            single.total_duration_ms = track.duration_ms;
            single.export_mode = "album".into();
            single.output_file_name = output
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into();
            single.image_source = "custom".into();
            let label = format!("Track {} of {}", index + 1, album.tracks.len());
            export_album_file(
                &app,
                single,
                ExportFilePlan {
                    output: output.clone(),
                    replace,
                    file_index: index,
                    file_count: album.tracks.len(),
                    label,
                    file_title: Some(track.chapter_title.trim().into()),
                    include_chapters: false,
                },
            )
            .await?;
        }
    } else {
        export_album_file(
            &app,
            album.clone(),
            ExportFilePlan {
                output: outputs[0].clone(),
                replace,
                file_index: 0,
                file_count: 1,
                label: String::new(),
                file_title: None,
                include_chapters: true,
            },
        )
        .await?;
    }
    emit_progress(
        &app,
        ExportProgress {
            album_id: album.id.clone(),
            stage: "completed".into(),
            percent: 100,
            message: "Export complete".into(),
        },
    );
    if album.export_mode == "individual" {
        Ok(destination_directory(&album)?.to_string_lossy().into())
    } else {
        Ok(outputs[0].to_string_lossy().into())
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            scan_root,
            inspect_album,
            preview_output_path,
            export_album,
            cancel_export,
            load_queue,
            save_queue
        ])
        .run(tauri::generate_context!())
        .expect("error while running Atmos Album Builder");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(codec: &str, rate: u32, channels: u32) -> AlbumTrack {
        AlbumTrack {
            codec: codec.into(),
            sample_rate: rate,
            channels,
            channel_layout: "7.1".into(),
            codec_tag: "ec-3".into(),
            duration_ms: 1_000,
            chapter_title: "Track".into(),
            ..AlbumTrack::default()
        }
    }

    #[test]
    fn natural_sort_orders_track_numbers() {
        let mut values = vec!["10 Song", "2 Song", "1 Song"];
        values.sort_by_key(|value| natural_key(value));
        assert_eq!(values, vec!["1 Song", "2 Song", "10 Song"]);
    }

    #[test]
    fn supported_audio_extensions_are_case_insensitive() {
        assert!(is_supported_audio(Path::new("track.m4a")));
        assert!(is_supported_audio(Path::new("track.MKA")));
        assert!(!is_supported_audio(Path::new("track.mkv")));
    }

    #[test]
    fn recognizes_image_attachments_as_embedded_covers() {
        let attachment = serde_json::json!({
            "codec_type": "attachment",
            "tags": { "filename": "cover.png", "mimetype": "image/png" }
        });
        assert_eq!(
            image_attachment_extension(&attachment).as_deref(),
            Some("png")
        );
    }

    #[test]
    fn filename_uses_artist_and_album() {
        let metadata = AlbumMetadata {
            title: "Album: Deluxe".into(),
            artist: "Artist".into(),
            ..AlbumMetadata::default()
        };
        assert_eq!(
            default_output_filename(&metadata, "Folder"),
            "Artist - Album Deluxe.mkv"
        );
    }

    #[test]
    fn incompatible_streams_are_blocked() {
        let album = AlbumJob {
            tracks: vec![track("eac3", 48_000, 8), track("eac3", 44_100, 8)],
            metadata: AlbumMetadata {
                title: "Album".into(),
                artist: "Artist".into(),
                ..AlbumMetadata::default()
            },
            image_path: Some("/definitely/missing.jpg".into()),
            output_file_name: "Album.mkv".into(),
            ..AlbumJob::default()
        };
        let issues = compatibility_issues(&album);
        assert!(issues
            .iter()
            .any(|issue| issue.contains("lossless concatenation")));
    }

    #[test]
    fn individual_files_allow_mixed_stream_properties() {
        let album = AlbumJob {
            export_mode: "individual".into(),
            tracks: vec![track("eac3", 48_000, 8), track("ac3", 44_100, 6)],
            metadata: AlbumMetadata {
                title: "Album".into(),
                artist: "Artist".into(),
                ..AlbumMetadata::default()
            },
            output_file_name: "Album.mkv".into(),
            ..AlbumJob::default()
        };
        assert!(!compatibility_issues(&album)
            .iter()
            .any(|issue| issue.contains("lossless concatenation")));
    }

    #[test]
    fn individual_filename_uses_order_and_chapter_title() {
        let mut source = track("eac3", 48_000, 8);
        source.chapter_title = "Opening: Theme".into();
        assert_eq!(
            individual_output_filename(1, 12, &source),
            "02 - Opening Theme.mkv"
        );
    }

    #[test]
    fn selected_animation_must_exist() {
        let album = AlbumJob {
            visual_mode: "video".into(),
            video_path: Some("/definitely/missing.mp4".into()),
            tracks: vec![track("eac3", 48_000, 8)],
            metadata: AlbumMetadata {
                title: "Album".into(),
                artist: "Artist".into(),
                ..AlbumMetadata::default()
            },
            output_file_name: "Album.mkv".into(),
            ..AlbumJob::default()
        };
        assert!(compatibility_issues(&album)
            .iter()
            .any(|issue| issue.contains("MP4 or MOV")));
    }

    #[test]
    fn metadata_contains_album_tags_and_chapters() {
        let album = AlbumJob {
            tracks: vec![track("eac3", 48_000, 8), track("eac3", 48_000, 8)],
            metadata: AlbumMetadata {
                title: "A=B".into(),
                artist: "Artist".into(),
                date: "2026".into(),
                ..AlbumMetadata::default()
            },
            ..AlbumJob::default()
        };
        let metadata = build_ffmetadata(&album, None, true);
        assert!(metadata.contains("title=A\\=B"));
        assert_eq!(metadata.matches("[CHAPTER]").count(), 2);
        let track_metadata = build_ffmetadata(&album, Some("Track title"), false);
        assert!(track_metadata.contains("title=Track title"));
        assert!(track_metadata.contains("album=A\\=B"));
        assert!(!track_metadata.contains("[CHAPTER]"));
    }

    #[test]
    fn output_filename_cannot_escape_destination() {
        assert!(validate_output_filename("../album.mkv").is_err());
        assert!(validate_output_filename("folder/album.mkv").is_err());
        assert_eq!(validate_output_filename("album").unwrap(), "album.mkv");
    }
}
