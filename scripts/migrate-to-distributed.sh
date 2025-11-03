#!/bin/bash
#
# Migration script: SQLite single-server → Distributed multi-region storage
#
# This script migrates data from the current single-server SQLite setup
# to the new distributed storage architecture with CockroachDB, Redis,
# MinIO, and TiKV.
#
# Usage: ./migrate-to-distributed.sh [--dry-run] [--verify-only]
#
# Prerequisites:
# - Source SQLite database at data/dchat.db
# - CockroachDB cluster accessible via config/storage-distributed.toml
# - MinIO/S3 bucket configured and accessible
# - Redis cluster running
# - TiKV cluster running
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CONFIG_FILE="${PROJECT_ROOT}/config/storage-distributed.toml"
SOURCE_DB="${PROJECT_ROOT}/data/dchat.db"
BACKUP_DIR="${PROJECT_ROOT}/backups/migration-$(date +%Y%m%d-%H%M%S)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

DRY_RUN=false
VERIFY_ONLY=false

# Parse arguments
for arg in "$@"; do
    case $arg in
        --dry-run)
            DRY_RUN=true
            echo -e "${YELLOW}Running in DRY RUN mode - no changes will be made${NC}"
            ;;
        --verify-only)
            VERIFY_ONLY=true
            echo -e "${YELLOW}Running in VERIFY ONLY mode - checking connectivity only${NC}"
            ;;
    esac
done

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_prerequisites() {
    log_info "Checking prerequisites..."
    
    # Check if source database exists
    if [ ! -f "$SOURCE_DB" ]; then
        log_error "Source database not found: $SOURCE_DB"
        exit 1
    fi
    
    # Check if config file exists
    if [ ! -f "$CONFIG_FILE" ]; then
        log_error "Configuration file not found: $CONFIG_FILE"
        exit 1
    fi
    
    # Check required commands
    for cmd in sqlite3 psql redis-cli aws cargo; do
        if ! command -v $cmd &> /dev/null; then
            log_error "Required command not found: $cmd"
            exit 1
        fi
    done
    
    log_info "Prerequisites check passed"
}

create_backup() {
    log_info "Creating backup of source database..."
    
    mkdir -p "$BACKUP_DIR"
    cp "$SOURCE_DB" "$BACKUP_DIR/dchat.db.backup"
    
    # Export to SQL dump
    sqlite3 "$SOURCE_DB" .dump > "$BACKUP_DIR/dchat.sql"
    
    log_info "Backup created at: $BACKUP_DIR"
}

verify_connectivity() {
    log_info "Verifying connectivity to distributed storage..."
    
    # Test CockroachDB connectivity
    log_info "Testing CockroachDB connection..."
    if cargo run --bin dchat-storage-test -- --test-db; then
        log_info "✓ CockroachDB connectivity verified"
    else
        log_error "✗ CockroachDB connectivity failed"
        return 1
    fi
    
    # Test Redis connectivity
    log_info "Testing Redis cluster connection..."
    if cargo run --bin dchat-storage-test -- --test-cache; then
        log_info "✓ Redis cluster connectivity verified"
    else
        log_error "✗ Redis cluster connectivity failed"
        return 1
    fi
    
    # Test MinIO/S3 connectivity
    log_info "Testing MinIO/S3 connection..."
    if cargo run --bin dchat-storage-test -- --test-object-storage; then
        log_info "✓ MinIO/S3 connectivity verified"
    else
        log_error "✗ MinIO/S3 connectivity failed"
        return 1
    fi
    
    # Test TiKV connectivity
    log_info "Testing TiKV connection..."
    if cargo run --bin dchat-storage-test -- --test-tikv; then
        log_info "✓ TiKV connectivity verified"
    else
        log_error "✗ TiKV connectivity failed"
        return 1
    fi
    
    log_info "All connectivity tests passed"
}

export_sqlite_data() {
    log_info "Exporting data from SQLite..."
    
    # Export messages
    log_info "Exporting messages..."
    sqlite3 "$SOURCE_DB" <<EOF > "$BACKUP_DIR/messages.csv"
.headers on
.mode csv
SELECT * FROM messages;
EOF
    
    # Export channels
    log_info "Exporting channels..."
    sqlite3 "$SOURCE_DB" <<EOF > "$BACKUP_DIR/channels.csv"
.headers on
.mode csv
SELECT * FROM channels;
EOF
    
    # Export users
    log_info "Exporting users..."
    sqlite3 "$SOURCE_DB" <<EOF > "$BACKUP_DIR/users.csv"
.headers on
.mode csv
SELECT * FROM users;
EOF
    
    # Export content store
    log_info "Exporting content store..."
    sqlite3 "$SOURCE_DB" <<EOF > "$BACKUP_DIR/content_store.csv"
.headers on
.mode csv
SELECT * FROM content_store;
EOF
    
    # Count records
    local message_count=$(sqlite3 "$SOURCE_DB" "SELECT COUNT(*) FROM messages;")
    local channel_count=$(sqlite3 "$SOURCE_DB" "SELECT COUNT(*) FROM channels;")
    local user_count=$(sqlite3 "$SOURCE_DB" "SELECT COUNT(*) FROM users;")
    
    log_info "Exported $message_count messages, $channel_count channels, $user_count users"
}

import_to_cockroachdb() {
    log_info "Importing data to CockroachDB..."
    
    if [ "$DRY_RUN" = true ]; then
        log_warn "DRY RUN: Skipping CockroachDB import"
        return 0
    fi
    
    # Use the dchat migration tool
    cargo run --bin dchat-migrate -- \
        --source "$SOURCE_DB" \
        --config "$CONFIG_FILE" \
        --target cockroachdb \
        --batch-size 1000
    
    log_info "CockroachDB import completed"
}

migrate_media_files() {
    log_info "Migrating media files to MinIO..."
    
    if [ "$DRY_RUN" = true ]; then
        log_warn "DRY RUN: Skipping media file migration"
        return 0
    fi
    
    # Find all media files in local storage
    local media_dir="${PROJECT_ROOT}/data/media"
    
    if [ ! -d "$media_dir" ]; then
        log_warn "No media directory found at $media_dir"
        return 0
    fi
    
    # Upload to MinIO using migration tool
    cargo run --bin dchat-migrate -- \
        --source-dir "$media_dir" \
        --config "$CONFIG_FILE" \
        --target object-storage \
        --preserve-paths
    
    log_info "Media files migrated to MinIO"
}

migrate_blockchain_state() {
    log_info "Migrating blockchain state to TiKV..."
    
    if [ "$DRY_RUN" = true ]; then
        log_warn "DRY RUN: Skipping blockchain state migration"
        return 0
    fi
    
    # Use migration tool to move blockchain state
    cargo run --bin dchat-migrate -- \
        --source "$SOURCE_DB" \
        --config "$CONFIG_FILE" \
        --target tikv \
        --state-only
    
    log_info "Blockchain state migrated to TiKV"
}

verify_migration() {
    log_info "Verifying data migration..."
    
    # Run verification tool
    cargo run --bin dchat-verify-migration -- \
        --source "$SOURCE_DB" \
        --config "$CONFIG_FILE" \
        --check-all
    
    if [ $? -eq 0 ]; then
        log_info "✓ Migration verification passed"
    else
        log_error "✗ Migration verification failed"
        return 1
    fi
}

update_configuration() {
    log_info "Updating runtime configuration..."
    
    if [ "$DRY_RUN" = true ]; then
        log_warn "DRY RUN: Skipping configuration update"
        return 0
    fi
    
    # Create symlink to distributed config
    local runtime_config="${PROJECT_ROOT}/config/storage.toml"
    
    if [ -f "$runtime_config" ]; then
        mv "$runtime_config" "${runtime_config}.single-server.backup"
    fi
    
    ln -sf storage-distributed.toml "$runtime_config"
    
    log_info "Configuration updated to use distributed storage"
}

warmup_caches() {
    log_info "Warming up Redis caches..."
    
    if [ "$DRY_RUN" = true ]; then
        log_warn "DRY RUN: Skipping cache warmup"
        return 0
    fi
    
    # Use warmup tool to pre-populate caches
    cargo run --bin dchat-cache-warmup -- \
        --config "$CONFIG_FILE" \
        --recent-messages 10000 \
        --active-users 1000 \
        --popular-channels 100
    
    log_info "Caches warmed up"
}

print_summary() {
    log_info ""
    log_info "========================================"
    log_info "Migration Summary"
    log_info "========================================"
    log_info "Backup location: $BACKUP_DIR"
    log_info "Configuration: $CONFIG_FILE"
    log_info ""
    log_info "Distributed storage is now active:"
    log_info "  - CockroachDB: Multi-region SQL database"
    log_info "  - Redis Cluster: Distributed cache"
    log_info "  - MinIO/S3: Object storage for media"
    log_info "  - TiKV: Blockchain state storage"
    log_info ""
    log_info "Next steps:"
    log_info "  1. Test the application: cargo run"
    log_info "  2. Monitor health: cargo run --bin dchat-health-check"
    log_info "  3. Review logs: docker-compose logs -f"
    log_info ""
    log_info "To rollback, restore from: $BACKUP_DIR"
    log_info "========================================"
}

main() {
    log_info "Starting migration from single-server to distributed storage"
    log_info "Project root: $PROJECT_ROOT"
    log_info ""
    
    check_prerequisites
    
    if [ "$VERIFY_ONLY" = true ]; then
        verify_connectivity
        exit 0
    fi
    
    create_backup
    verify_connectivity
    export_sqlite_data
    import_to_cockroachdb
    migrate_media_files
    migrate_blockchain_state
    verify_migration
    update_configuration
    warmup_caches
    
    print_summary
    
    log_info "Migration completed successfully!"
}

# Run main function
main
