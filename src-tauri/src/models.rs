use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AlbumMetadata {
    pub title: String,
    pub artist: String,
    pub date: String,
    pub genre: String,
    pub comment: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AlbumTrack {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub duration_ms: u64,
    pub chapter_title: String,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub codec: String,
    pub profile: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_layout: String,
    pub codec_tag: String,
    pub extradata_size: u64,
    pub extradata_fingerprint: String,
    pub album: String,
    pub album_artist: String,
    pub artist: String,
    pub date: String,
    pub genre: String,
    pub comment: String,
    pub has_embedded_cover: bool,
}

impl AlbumTrack {
    pub fn compatibility_signature(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.codec,
            self.profile,
            self.sample_rate,
            self.channels,
            self.channel_layout,
            self.codec_tag,
            self.extradata_size,
            self.extradata_fingerprint
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AlbumJob {
    pub id: String,
    pub source_folder: String,
    pub folder_name: String,
    pub tracks: Vec<AlbumTrack>,
    pub metadata: AlbumMetadata,
    pub image_path: Option<String>,
    pub image_source: String,
    pub visual_mode: String,
    pub video_path: Option<String>,
    pub output_file_name: String,
    pub export_mode: String,
    pub file_name_overridden: bool,
    pub destination_override: Option<String>,
    pub approved: bool,
    pub status: String,
    pub warnings: Vec<String>,
    pub blocking_issues: Vec<String>,
    pub total_duration_ms: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputPreview {
    pub path: String,
    pub exists: bool,
    pub paths: Vec<String>,
    pub conflict_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub album_id: String,
    pub stage: String,
    pub percent: u64,
    pub message: String,
}
