# dchat Bot Examples

This directory contains example implementations demonstrating the dchat Bot API.

## ⚠️ Important: Mock Data Notice

**These examples use mock/placeholder data for demonstration purposes only.**

### Mock Data Used

1. **Spotify Token** (`mock_spotify_token`)
   - File: `complete_integration.rs`
   - Purpose: Demonstrates music bot integration
   - **Production**: Replace with real Spotify OAuth flow
   - See: https://developer.spotify.com/documentation/web-api/tutorials/code-flow

2. **Mock Image Generation** (`create_mock_image()`)
   - Creates procedural gradient images for testing
   - **Production**: Use real image assets or user-uploaded content

3. **Mock Audio Generation** (`create_mock_audio()`)
   - Generates sine wave audio for testing
   - **Production**: Use real audio files or streaming APIs

## Running Examples

### Complete Integration Example

```bash
cargo run --example complete_integration
```

This example demonstrates:
- Bot registration and authentication
- File upload/download system
- Music API client integration (with mock token)
- User profile management
- Webhook setup

## Production Deployment Checklist

Before deploying bot functionality to production:

- [ ] Replace `mock_spotify_token` with real OAuth implementation
- [ ] Implement real API authentication for all external services
- [ ] Replace mock image/audio generators with actual content sources
- [ ] Set up webhook endpoints with TLS/HTTPS
- [ ] Configure rate limiting and error handling
- [ ] Add monitoring and logging for bot operations
- [ ] Test with real user accounts and content
- [ ] Review API usage limits for external services

## Real API Integration

### Spotify Integration

```rust
// Replace mock token with OAuth flow
let auth_code = get_spotify_auth_code().await?;
let access_token = exchange_auth_code_for_token(auth_code).await?;
music_client.set_spotify_token(access_token);
```

### File Upload

```rust
// Real file upload from filesystem or user input
let file_data = tokio::fs::read("path/to/image.jpg").await?;
let upload_result = file_manager.upload_file(
    MediaFileType::Photo,
    file_data,
    Some("image/jpeg".to_string()),
    // ... other params
).await?;
```

## Documentation

- Bot API Documentation: `../src/bot_api.rs`
- HTTP Client: `../src/api/http_client.rs`
- File Management: `../src/files/`
- Music Client: `../src/music_client.rs`

## Support

For questions about bot development:
1. Read the main [ARCHITECTURE.md](../../../ARCHITECTURE.md)
2. Check [Bot System Documentation](../../../BOT_SYSTEM_COMPLETE.md)
3. Review the API specifications in the crate source

## License

Same as parent dchat project.
