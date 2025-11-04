//! Complete integration example demonstrating music API, database persistence, and file upload

use dchat_bots::*;
use dchat_core::Result;
use sqlx::SqlitePool;
use std::path::PathBuf;

/// Complete example: Create user profile with audio status using Spotify
#[tokio::main]
async fn main() -> Result<()> {
    println!("=== dchat Bot System - Complete Integration Example ===\n");

    // Step 1: Initialize database
    println!("1. Initializing database...");
    let pool = SqlitePool::connect("sqlite:./dchat_example.db")
        .await
        .map_err(|e| dchat_core::Error::storage(format!("Failed to connect to database: {}", e)))?;

    let profile_storage = ProfileStorage::new(pool);
    profile_storage.init_schema().await?;
    println!("   ✓ Database initialized\n");

    // Step 2: Initialize file upload system
    println!("2. Initializing file upload system...");
    let config = UploadConfig {
        storage_path: PathBuf::from("./example_uploads"),
        ..Default::default()
    };
    let file_manager = FileUploadManager::new(config);
    file_manager.init_storage().await?;
    println!("   ✓ File upload system ready\n");

    // Step 3: Initialize music API client
    println!("3. Initializing music API client...");
    let mut music_client = MusicApiClient::new();

    // In a real application, you would:
    // let spotify_token = MusicApiClient::authenticate_spotify(
    //     "your_client_id",
    //     "your_client_secret"
    // ).await?;
    // music_client.set_spotify_token(spotify_token);

    // For this example, we'll use a mock token
    music_client.set_spotify_token("mock_spotify_token".to_string());
    println!("   ✓ Music API client ready (mock token)\n");

    // Step 4: Create user profile
    println!("4. Creating user profile...");
    let user_id = dchat_core::types::UserId::new();

    // Upload profile picture
    let profile_pic_data = create_mock_image(200, 200);
    let profile_pic_upload = file_manager
        .upload_file(
            MediaFileType::Photo,
            profile_pic_data,
            Some("image/jpeg".to_string()),
            Some(200),
            Some(200),
            None,
        )
        .await?;

    let profile_picture = ProfilePicture {
        file_id: profile_pic_upload.file_id.clone(),
        file_unique_id: profile_pic_upload.file_unique_id.clone(),
        small_file_id: Some(profile_pic_upload.file_id.clone()),
        large_file_id: Some(profile_pic_upload.file_id.clone()),
        uploaded_at: chrono::Utc::now(),
    };

    let profile = UserProfile {
        user_id: user_id.clone(),
        username: "musiclover".to_string(),
        display_name: "Music Lover".to_string(),
        bio: Some("🎵 Rock music enthusiast | Classic hits 24/7".to_string()),
        profile_picture: Some(profile_picture),
        status: None,
        online_status: OnlineStatus::Online,
        last_seen: Some(chrono::Utc::now()),
        created_at: chrono::Utc::now(),
        privacy: PrivacySettings {
            profile_picture_visibility: VisibilityLevel::Everyone,
            status_visibility: VisibilityLevel::Contacts,
            last_seen_visibility: VisibilityLevel::Contacts,
            bio_visibility: VisibilityLevel::Everyone,
            message_visibility: VisibilityLevel::Everyone,
        },
        is_verified: false,
        metadata: std::collections::HashMap::new(),
    };

    profile_storage.save_profile(&profile).await?;
    println!("   ✓ Profile created: @{}", profile.username);
    println!("   ✓ Display name: {}", profile.display_name);
    println!("   ✓ Bio: {}", profile.bio.as_ref().unwrap());
    println!(
        "   ✓ Profile picture uploaded (ID: {})\n",
        profile_pic_upload.file_id
    );

    // Step 5: Create audio status with Spotify track
    println!("5. Creating audio status with music...");

    // Upload background image for status
    let bg_image_data = create_mock_image(1080, 1920);
    let bg_image_upload = file_manager
        .upload_file(
            MediaFileType::Photo,
            bg_image_data,
            Some("image/jpeg".to_string()),
            Some(1080),
            Some(1920),
            None,
        )
        .await?;

    // Upload audio preview
    let audio_data = create_mock_audio(30);
    let audio_upload = file_manager
        .upload_file(
            MediaFileType::Audio,
            audio_data,
            Some("audio/mpeg".to_string()),
            None,
            None,
            Some(30),
        )
        .await?;

    // Create Spotify track metadata (in real app, this would come from API)
    let spotify_track = MusicApiTrack {
        provider: MusicProvider::Spotify,
        track_id: "spotify:track:4u7EnebtmKWzUH433cf5Qv".to_string(),
        track_name: "Bohemian Rhapsody".to_string(),
        artist_name: "Queen".to_string(),
        album_name: Some("A Night at the Opera".to_string()),
        album_art_url: Some(
            "https://i.scdn.co/image/ab67616d0000b273e319baafd16e84f0408af2a0".to_string(),
        ),
        preview_url: Some("https://p.scdn.co/mp3-preview/...".to_string()),
    };

    // Create audio status
    let status = UserStatus {
        id: uuid::Uuid::new_v4(),
        status_type: StatusType::Audio {
            audio_file_id: audio_upload.file_id.clone(),
            background_image_id: Some(bg_image_upload.file_id.clone()),
            duration: 243,
            title: Some(spotify_track.track_name.clone()),
            artist: Some(spotify_track.artist_name.clone()),
            music_api_track_id: Some(spotify_track.clone()),
        },
        caption: Some("🎵 Classic rock never dies! #BohemianRhapsody #Queen".to_string()),
        background_color: Some("#1DB954".to_string()), // Spotify green
        created_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
        view_count: 0,
        viewers: Vec::new(),
    };

    profile_storage.save_status(&user_id, &status).await?;
    println!("   ✓ Audio status created");
    println!(
        "   ✓ Track: {} - {}",
        spotify_track.track_name, spotify_track.artist_name
    );
    println!("   ✓ Album: {}", spotify_track.album_name.as_ref().unwrap());
    println!("   ✓ Provider: {:?}", spotify_track.provider);
    println!(
        "   ✓ Background image: {} ({}x{})",
        bg_image_upload.file_id, 1080, 1920
    );
    println!("   ✓ Audio file: {} (30 seconds)", audio_upload.file_id);
    println!("   ✓ Expires at: {}\n", status.expires_at);

    // Step 6: Retrieve and display profile with status
    println!("6. Retrieving profile from database...");
    let retrieved_profile = profile_storage
        .get_profile(&user_id)
        .await?
        .expect("Profile should exist");

    println!("   ✓ Retrieved: @{}", retrieved_profile.username);
    println!("   ✓ Online status: {:?}", retrieved_profile.online_status);
    println!("   ✓ Verified: {}", retrieved_profile.is_verified);

    let active_statuses = profile_storage.get_active_statuses(&user_id).await?;
    println!("   ✓ Active statuses: {}\n", active_statuses.len());

    // Step 7: Search for users
    println!("7. Testing search functionality...");
    let search_results = profile_storage.search_profiles("music", 10).await?;
    println!(
        "   ✓ Found {} profiles matching 'music'",
        search_results.len()
    );
    for result in &search_results {
        println!("     - @{}: {}", result.username, result.display_name);
    }
    println!();

    // Step 8: Get storage statistics
    println!("8. Storage statistics:");
    let stats = file_manager.get_storage_stats().await?;
    println!("   ✓ Total files: {}", stats.total_files);
    println!("   ✓ Total size: {:.2} MB", stats.size_mb());
    println!();

    // Step 9: Simulate status views
    println!("9. Simulating status views...");
    let mut status_copy = status.clone();
    let viewer1 = dchat_core::types::UserId::new();
    let viewer2 = dchat_core::types::UserId::new();

    status_copy.view_count += 2;
    status_copy.viewers.push(viewer1);
    status_copy.viewers.push(viewer2);

    profile_storage.save_status(&user_id, &status_copy).await?;
    println!("   ✓ Status viewed by 2 users");

    let updated_statuses = profile_storage.get_active_statuses(&user_id).await?;
    if let Some(updated) = updated_statuses.first() {
        println!("   ✓ View count: {}", updated.view_count);
        println!("   ✓ Viewers: {}\n", updated.viewers.len());
    }

    // Step 10: Demonstrate file retrieval
    println!("10. Testing file retrieval...");
    let retrieved_audio = file_manager.get_file(&audio_upload.file_id).await?;
    println!("   ✓ Retrieved audio file: {} bytes", retrieved_audio.len());

    let retrieved_bg = file_manager.get_file(&bg_image_upload.file_id).await?;
    println!(
        "   ✓ Retrieved background image: {} bytes\n",
        retrieved_bg.len()
    );

    println!("=== Integration Example Complete! ===");
    println!("\nSummary:");
    println!("- ✓ Database persistence working");
    println!("- ✓ File upload system working");
    println!("- ✓ Music API integration ready");
    println!("- ✓ User profiles with pictures");
    println!("- ✓ Audio status with Spotify metadata");
    println!("- ✓ Status expiration tracking");
    println!("- ✓ Privacy settings configured");
    println!("- ✓ Search functionality working");

    Ok(())
}

// Helper functions to create mock data

fn create_mock_image(width: u32, height: u32) -> Vec<u8> {
    // Create a simple gradient image (mock data)
    let size = (width * height * 3) as usize;
    let mut data = Vec::with_capacity(size);

    for y in 0..height {
        for x in 0..width {
            let r = ((x as f32 / width as f32) * 255.0) as u8;
            let g = ((y as f32 / height as f32) * 255.0) as u8;
            let b = 128;
            data.push(r);
            data.push(g);
            data.push(b);
        }
    }

    data
}

fn create_mock_audio(duration_seconds: u32) -> Vec<u8> {
    // Create mock audio data (sine wave)
    let sample_rate = 44100;
    let samples = sample_rate * duration_seconds;
    let mut data = Vec::with_capacity(samples as usize * 2);

    for i in 0..samples {
        let t = i as f32 / sample_rate as f32;
        let freq = 440.0; // A4 note
        let amplitude = 0.3;
        let sample = (amplitude * (2.0 * std::f32::consts::PI * freq * t).sin() * 32767.0) as i16;

        data.push((sample & 0xFF) as u8);
        data.push(((sample >> 8) & 0xFF) as u8);
    }

    data
}
