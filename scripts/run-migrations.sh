#!/usr/bin/env bash
# Run database migrations for dchat storage optimization
# Usage: ./run-migrations.sh [database-url]

set -e

DATABASE_URL="${1:-$DATABASE_URL}"
VERIFY=false
LIST=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --verify)
            VERIFY=true
            shift
            ;;
        --list)
            LIST=true
            shift
            ;;
        --help)
            cat <<EOF
dchat Storage Migrations Runner

USAGE:
    ./run-migrations.sh [OPTIONS] [DATABASE_URL]

OPTIONS:
    --verify              Verify schema without running migrations
    --list                List applied and pending migrations
    --help                Show this help message

EXAMPLES:
    # Run all pending migrations
    ./run-migrations.sh "postgresql://user:pass@localhost:5432/dchat"

    # Verify schema integrity
    ./run-migrations.sh --verify

    # List migration status
    ./run-migrations.sh --list

ENVIRONMENT VARIABLES:
    DATABASE_URL          Default database connection URL
EOF
            exit 0
            ;;
        *)
            DATABASE_URL="$1"
            shift
            ;;
    esac
done

if [ -z "$DATABASE_URL" ]; then
    echo "Error: Database URL not provided. Set DATABASE_URL environment variable or pass as argument." >&2
    exit 1
fi

echo "dchat Storage Migrations Runner"
echo "================================"
echo ""

# Create temporary directory
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

# Create Rust program
cat > "$TEMP_DIR/main.rs" <<'RUST'
use dchat_storage::{MigrationRunner, MIGRATIONS};
use sqlx::PgPool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into())
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")?;
    let mode = std::env::var("MIGRATION_MODE").unwrap_or_else(|_| "run".to_string());

    println!("Connecting to database...");
    let pool = PgPool::connect(&database_url).await?;
    let runner = MigrationRunner::new(pool);

    match mode.as_str() {
        "run" => {
            println!("Running migrations...");
            let applied = runner.run_all().await?;
            println!("\n✅ Applied {} migrations", applied);
        }
        "verify" => {
            println!("Verifying schema...");
            let valid = runner.verify().await?;
            if valid {
                println!("\n✅ Schema verification passed");
            } else {
                eprintln!("\n❌ Schema verification failed");
                std::process::exit(1);
            }
        }
        "list" => {
            println!("Applied migrations:");
            let applied = runner.list_applied().await?;
            for (id, name, applied_at) in applied {
                println!("  ✅ {} - {} (applied: {})", id, name, applied_at);
            }

            println!("\nPending migrations:");
            let pending = runner.list_pending().await?;
            if pending.is_empty() {
                println!("  (none)");
            } else {
                for migration in pending {
                    println!("  ⏳ {} - {}", migration.id, migration.name);
                }
            }
        }
        _ => {
            eprintln!("Unknown mode: {}", mode);
            std::process::exit(1);
        }
    }

    Ok(())
}
RUST

# Create Cargo.toml
cat > "$TEMP_DIR/Cargo.toml" <<EOF
[package]
name = "migration-runner"
version = "0.1.0"
edition = "2021"

[dependencies]
dchat-storage = { path = "$(pwd)/crates/dchat-storage" }
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio"] }
tokio = { version = "1.40", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
EOF

# Set environment variables
export DATABASE_URL

if [ "$VERIFY" = true ]; then
    export MIGRATION_MODE="verify"
elif [ "$LIST" = true ]; then
    export MIGRATION_MODE="list"
else
    export MIGRATION_MODE="run"
fi

# Run the migration program
echo "Compiling migration runner..."
cd "$TEMP_DIR"
cargo run --quiet

if [ $? -eq 0 ]; then
    echo ""
    echo "✅ Migration operation completed successfully"
else
    echo ""
    echo "❌ Migration operation failed"
    exit 1
fi
