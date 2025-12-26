//! Database schema definitions

/// SQL schema for dchat database
pub struct Schema;

impl Schema {
    /// Get the complete database schema
    pub fn create_tables() -> Vec<&'static str> {
        vec![
            // Users table
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                public_key BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                reputation_messaging INTEGER DEFAULT 0,
                reputation_governance INTEGER DEFAULT 0,
                reputation_relay INTEGER DEFAULT 0
            )
            "#,
            // Identities table (for multi-identity support)
            r#"
            CREATE TABLE IF NOT EXISTS identities (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                identity_type TEXT NOT NULL,
                public_key BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
            // Devices table
            r#"
            CREATE TABLE IF NOT EXISTS devices (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                device_name TEXT NOT NULL,
                public_key BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                trusted INTEGER DEFAULT 0,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
            // Messages table
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                sender_id TEXT NOT NULL,
                recipient_id TEXT,
                channel_id TEXT,
                content_type TEXT NOT NULL,
                content TEXT NOT NULL,
                encrypted_payload BLOB NOT NULL,
                timestamp INTEGER NOT NULL,
                sequence_num INTEGER,
                status TEXT NOT NULL,
                expires_at INTEGER,
                size INTEGER NOT NULL,
                content_hash TEXT,
                FOREIGN KEY (sender_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
            // Channels table
            r#"
            CREATE TABLE IF NOT EXISTS channels (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                creator_id TEXT NOT NULL,
                channel_type TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                member_count INTEGER DEFAULT 0,
                FOREIGN KEY (creator_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
            // Channel members table
            r#"
            CREATE TABLE IF NOT EXISTS channel_members (
                channel_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                joined_at INTEGER NOT NULL,
                role TEXT DEFAULT 'member',
                PRIMARY KEY (channel_id, user_id),
                FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
            // Guardians table
            r#"
            CREATE TABLE IF NOT EXISTS guardians (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                guardian_id TEXT NOT NULL,
                public_key BLOB NOT NULL,
                added_at INTEGER NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
            // Recovery requests table
            r#"
            CREATE TABLE IF NOT EXISTS recovery_requests (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                new_public_key BLOB NOT NULL,
                initiated_at INTEGER NOT NULL,
                timelock_until INTEGER NOT NULL,
                required_approvals INTEGER NOT NULL,
                status TEXT NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
            // Delivery proofs table
            r#"
            CREATE TABLE IF NOT EXISTS delivery_proofs (
                message_id TEXT PRIMARY KEY,
                relay_peer_id TEXT NOT NULL,
                recipient_signature BLOB,
                timestamp INTEGER NOT NULL,
                chain_tx_hash TEXT,
                FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
            )
            "#,
            // Key rotation history
            r#"
            CREATE TABLE IF NOT EXISTS key_rotations (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                old_key_hash BLOB NOT NULL,
                new_key_hash BLOB NOT NULL,
                rotated_at INTEGER NOT NULL,
                reason TEXT,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
            // Generic key/value store for lightweight client metadata
            r#"
            CREATE TABLE IF NOT EXISTS client_kv (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
        ]
    }

    /// Create indexes for performance
    pub fn create_indexes() -> Vec<&'static str> {
        vec![
            "CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id)",
            "CREATE INDEX IF NOT EXISTS idx_messages_recipient ON messages(recipient_id)",
            "CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel_id)",
            "CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_messages_expires_at ON messages(expires_at)",
            "CREATE INDEX IF NOT EXISTS idx_messages_content_hash ON messages(content_hash)",
            "CREATE INDEX IF NOT EXISTS idx_devices_user ON devices(user_id)",
            "CREATE INDEX IF NOT EXISTS idx_channel_members_user ON channel_members(user_id)",
            "CREATE INDEX IF NOT EXISTS idx_guardians_user ON guardians(user_id)",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_definitions() {
        let tables = Schema::create_tables();
        assert!(tables.len() > 0);

        let indexes = Schema::create_indexes();
        assert!(indexes.len() > 0);
    }
}
