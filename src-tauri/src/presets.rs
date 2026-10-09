//! Device-oriented output profiles. Values follow the documented playback
//! capabilities of each device family (H.264 profile/level, max resolution).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub group: &'static str,
    pub format: &'static str,
    pub codec: &'static str,
    pub profile: Option<&'static str>,
    pub level: Option<&'static str>,
    pub max_width: u32,
    pub max_height: u32,
    pub video_bitrate_k: Option<u32>,
    pub audio_bitrate_k: u32,
    pub crf: u32,
}

pub const PRESETS: &[Preset] = &[
    Preset { id: "iphone_modern", group: "apple", format: "mp4", codec: "h264", profile: Some("high"), level: Some("4.1"), max_width: 1920, max_height: 1080, video_bitrate_k: None, audio_bitrate_k: 160, crf: 22 },
    Preset { id: "iphone_hevc", group: "apple", format: "mp4", codec: "hevc", profile: None, level: None, max_width: 3840, max_height: 2160, video_bitrate_k: None, audio_bitrate_k: 160, crf: 24 },
    Preset { id: "iphone_legacy", group: "apple", format: "mp4", codec: "h264", profile: Some("baseline"), level: Some("3.0"), max_width: 640, max_height: 480, video_bitrate_k: Some(1500), audio_bitrate_k: 128, crf: 23 },
    Preset { id: "ipod_classic", group: "apple", format: "m4v", codec: "h264", profile: Some("baseline"), level: Some("3.0"), max_width: 320, max_height: 240, video_bitrate_k: Some(768), audio_bitrate_k: 128, crf: 23 },
    Preset { id: "ipod_touch", group: "apple", format: "m4v", codec: "h264", profile: Some("main"), level: Some("3.1"), max_width: 960, max_height: 640, video_bitrate_k: Some(2500), audio_bitrate_k: 128, crf: 23 },
    Preset { id: "ipad", group: "apple", format: "mp4", codec: "h264", profile: Some("high"), level: Some("4.1"), max_width: 1920, max_height: 1080, video_bitrate_k: None, audio_bitrate_k: 160, crf: 21 },
    Preset { id: "android_1080", group: "android", format: "mp4", codec: "h264", profile: Some("high"), level: Some("4.1"), max_width: 1920, max_height: 1080, video_bitrate_k: None, audio_bitrate_k: 160, crf: 22 },
    Preset { id: "android_720", group: "android", format: "mp4", codec: "h264", profile: Some("main"), level: Some("3.1"), max_width: 1280, max_height: 720, video_bitrate_k: None, audio_bitrate_k: 128, crf: 23 },
    Preset { id: "web_480", group: "web", format: "mp4", codec: "h264", profile: Some("main"), level: Some("3.0"), max_width: 854, max_height: 480, video_bitrate_k: None, audio_bitrate_k: 96, crf: 26 },
    Preset { id: "web_webm", group: "web", format: "webm", codec: "vp9", profile: None, level: None, max_width: 1920, max_height: 1080, video_bitrate_k: None, audio_bitrate_k: 128, crf: 32 },
];

pub fn get(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}
