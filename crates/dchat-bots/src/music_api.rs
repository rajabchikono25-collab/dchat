//! Music API integration for Spotify, Apple Music, and other providers

use base64::Engine;
use dchat_core::{Error, Result};
use dchat_identity::profile::{MusicApiTrack, MusicProvider};
use serde::{Deserialize, Serialize};

/// Music API client for fetching track metadata
pub struct MusicApiClient {
    spotify_token: Option<String>,
    apple_music_token: Option<String>,
    http_client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyTrack {
    id: String,
    name: String,
    artists: Vec<SpotifyArtist>,
    album: SpotifyAlbum,
    duration_ms: u64,
    preview_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyArtist {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyAlbum {
    name: String,
    images: Vec<SpotifyImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyImage {
    url: String,
    height: Option<u32>,
    width: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifySearchResponse {
    tracks: SpotifyTracks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyTracks {
    items: Vec<SpotifyTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppleMusicTrack {
    id: String,
    attributes: AppleMusicAttributes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppleMusicAttributes {
    name: String,
    artist_name: String,
    album_name: String,
    duration_in_millis: u64,
    artwork: AppleMusicArtwork,
    previews: Vec<AppleMusicPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppleMusicArtwork {
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppleMusicPreview {
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppleMusicSearchResponse {
    results: AppleMusicResults,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppleMusicResults {
    songs: AppleMusicSongs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppleMusicSongs {
    data: Vec<AppleMusicTrack>,
}

impl MusicApiClient {
    /// Create new music API client
    pub fn new() -> Self {
        Self {
            spotify_token: None,
            apple_music_token: None,
            http_client: reqwest::Client::new(),
        }
    }

    /// Set Spotify access token
    pub fn set_spotify_token(&mut self, token: String) {
        self.spotify_token = Some(token);
    }

    /// Set Apple Music developer token
    pub fn set_apple_music_token(&mut self, token: String) {
        self.apple_music_token = Some(token);
    }

    /// Search for tracks on Spotify
    pub async fn search_spotify(&self, query: &str, limit: u32) -> Result<Vec<MusicApiTrack>> {
        let token = self
            .spotify_token
            .as_ref()
            .ok_or_else(|| Error::network("Spotify token not set"))?;

        let url = format!(
            "https://api.spotify.com/v1/search?q={}&type=track&limit={}",
            urlencoding::encode(query),
            limit
        );

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| Error::network(format!("Spotify API request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Spotify API error: {}",
                response.status()
            )));
        }

        let search_response: SpotifySearchResponse = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse Spotify response: {}", e)))?;

        Ok(search_response
            .tracks
            .items
            .into_iter()
            .map(|track| MusicApiTrack {
                provider: MusicProvider::Spotify,
                track_id: format!("spotify:track:{}", track.id),
                track_name: track.name,
                artist_name: track
                    .artists
                    .first()
                    .map(|a| a.name.clone())
                    .unwrap_or_default(),
                album_name: Some(track.album.name),
                album_art_url: track.album.images.first().map(|img| img.url.clone()),
                preview_url: track.preview_url,
            })
            .collect())
    }

    /// Get Spotify track by ID
    pub async fn get_spotify_track(&self, track_id: &str) -> Result<MusicApiTrack> {
        let token = self
            .spotify_token
            .as_ref()
            .ok_or_else(|| Error::network("Spotify token not set"))?;

        // Extract track ID from spotify URI (spotify:track:ID) or use directly
        let id = track_id.strip_prefix("spotify:track:").unwrap_or(track_id);

        let url = format!("https://api.spotify.com/v1/tracks/{}", id);

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| Error::network(format!("Spotify API request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Spotify API error: {}",
                response.status()
            )));
        }

        let track: SpotifyTrack = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse Spotify response: {}", e)))?;

        Ok(MusicApiTrack {
            provider: MusicProvider::Spotify,
            track_id: format!("spotify:track:{}", track.id),
            track_name: track.name,
            artist_name: track
                .artists
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_default(),
            album_name: Some(track.album.name),
            album_art_url: track.album.images.first().map(|img| img.url.clone()),
            preview_url: track.preview_url,
        })
    }

    /// Search for tracks on Apple Music
    pub async fn search_apple_music(
        &self,
        query: &str,
        limit: u32,
        country: &str,
    ) -> Result<Vec<MusicApiTrack>> {
        let token = self
            .apple_music_token
            .as_ref()
            .ok_or_else(|| Error::network("Apple Music token not set"))?;

        let url = format!(
            "https://api.music.apple.com/v1/catalog/{}/search?term={}&types=songs&limit={}",
            country,
            urlencoding::encode(query),
            limit
        );

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| Error::network(format!("Apple Music API request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Apple Music API error: {}",
                response.status()
            )));
        }

        let search_response: AppleMusicSearchResponse = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse Apple Music response: {}", e)))?;

        Ok(search_response
            .results
            .songs
            .data
            .into_iter()
            .map(|track| {
                let attrs = track.attributes;
                MusicApiTrack {
                    provider: MusicProvider::AppleMusic,
                    track_id: track.id,
                    track_name: attrs.name,
                    artist_name: attrs.artist_name,
                    album_name: Some(attrs.album_name),
                    album_art_url: Some(attrs.artwork.url.replace("{w}x{h}", "600x600")),
                    preview_url: attrs.previews.first().map(|p| p.url.clone()),
                }
            })
            .collect())
    }

    /// Get Apple Music track by ID
    pub async fn get_apple_music_track(
        &self,
        track_id: &str,
        country: &str,
    ) -> Result<MusicApiTrack> {
        let token = self
            .apple_music_token
            .as_ref()
            .ok_or_else(|| Error::network("Apple Music token not set"))?;

        let url = format!(
            "https://api.music.apple.com/v1/catalog/{}/songs/{}",
            country, track_id
        );

        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| Error::network(format!("Apple Music API request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Apple Music API error: {}",
                response.status()
            )));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse Apple Music response: {}", e)))?;

        let track = &data["data"][0];
        let attrs = &track["attributes"];

        Ok(MusicApiTrack {
            provider: MusicProvider::AppleMusic,
            track_id: track_id.to_string(),
            track_name: attrs["name"].as_str().unwrap_or_default().to_string(),
            artist_name: attrs["artistName"].as_str().unwrap_or_default().to_string(),
            album_name: Some(attrs["albumName"].as_str().unwrap_or_default().to_string()),
            album_art_url: attrs["artwork"]["url"]
                .as_str()
                .map(|url| url.replace("{w}x{h}", "600x600")),
            preview_url: attrs["previews"][0]["url"].as_str().map(String::from),
        })
    }

    /// Authenticate with Spotify using client credentials flow
    pub async fn authenticate_spotify(client_id: &str, client_secret: &str) -> Result<String> {
        let client = reqwest::Client::new();

        let params = [("grant_type", "client_credentials")];

        let auth_header = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", client_id, client_secret));

        let response = client
            .post("https://accounts.spotify.com/api/token")
            .header("Authorization", format!("Basic {}", auth_header))
            .form(&params)
            .send()
            .await
            .map_err(|e| Error::network(format!("Spotify auth request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Spotify auth failed: {}",
                response.status()
            )));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse Spotify auth response: {}", e)))?;

        data["access_token"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::network("No access token in Spotify response"))
    }
}

impl Default for MusicApiClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_music_api_client_creation() {
        let client = MusicApiClient::new();
        assert!(client.spotify_token.is_none());
        assert!(client.apple_music_token.is_none());
    }

    #[tokio::test]
    async fn test_set_tokens() {
        let mut client = MusicApiClient::new();
        client.set_spotify_token("test_spotify_token".to_string());
        client.set_apple_music_token("test_apple_token".to_string());

        assert_eq!(client.spotify_token, Some("test_spotify_token".to_string()));
        assert_eq!(
            client.apple_music_token,
            Some("test_apple_token".to_string())
        );
    }
}
