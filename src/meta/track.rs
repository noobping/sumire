use std::time::SystemTime;

pub(crate) const ALBUM_COVER_BASE: &str = "https://cdn.listen.moe/covers/";
pub(crate) const ARTIST_IMAGE_BASE: &str = "https://cdn.listen.moe/artists/";

/// Track info sent to the UI thread.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub artist: String,
    pub title: String,
    pub album_cover: Option<String>,
    pub artist_image: Option<String>,
    pub start_time_utc: SystemTime,
    pub duration_secs: u32,
}
