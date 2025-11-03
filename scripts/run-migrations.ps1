#!/usr/bin/env pwsh
# Run database migrations for dchat storage optimization
# Usage: .\run-migrations.ps1 [database-url]

param(
    [string]$DatabaseUrl = $env:DATABASE_URL,
    [switch]$Verify,
    [switch]$List,
    [switch]$Help
)

if ($Help) {
    Write-Host @"
dchat Storage Migrations Runner

USAGE:
    .\run-migrations.ps1 [OPTIONS]

OPTIONS:
    -DatabaseUrl <URL>    Database connection URL (default: `$env:DATABASE_URL)
    -Verify               Verify schema without running migrations
    -List                 List applied and pending migrations
    -Help                 Show this help message

EXAMPLES:
    # Run all pending migrations
    .\run-migrations.ps1 -DatabaseUrl "postgresql://user:pass@localhost:5432/dchat"

    # Verify schema integrity
    .\run-migrations.ps1 -Verify

    # List migration status
    .\run-migrations.ps1 -List

ENVIRONMENT VARIABLES:
    DATABASE_URL          Default database connection URL
"@
    exit 0
}

if (-not $DatabaseUrl) {
    Write-Error "Database URL not provided. Set DATABASE_URL environment variable or use -DatabaseUrl parameter."
    exit 1
}

Write-Host "dchat Storage Migrations Runner" -ForegroundColor Cyan
Write-Host "================================" -ForegroundColor Cyan
Write-Host ""

# Create temporary Rust program to run migrations
$tempDir = New-Item -ItemType Directory -Path ([System.IO.Path]::GetTempPath()) -Name "dchat-migrations-$(Get-Random)" -Force
$programPath = Join-Path $tempDir "main.rs"

$programCode = @"
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
"@

Set-Content -Path $programPath -Value $programCode

# Create Cargo.toml
$cargoToml = @"
[package]
name = "migration-runner"
version = "0.1.0"
edition = "2021"

[dependencies]
dchat-storage = { path = "../../../crates/dchat-storage" }
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio"] }
tokio = { version = "1.40", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
"@

Set-Content -Path (Join-Path $tempDir "Cargo.toml") -Value $cargoToml

try {
    # Set environment variables
    $env:DATABASE_URL = $DatabaseUrl
    
    if ($Verify) {
        $env:MIGRATION_MODE = "verify"
    } elseif ($List) {
        $env:MIGRATION_MODE = "list"
    } else {
        $env:MIGRATION_MODE = "run"
    }

    # Run the migration program
    Write-Host "Compiling migration runner..." -ForegroundColor Yellow
    Push-Location $tempDir
    cargo run --quiet 2>&1
    $exitCode = $LASTEXITCODE
    Pop-Location

    if ($exitCode -eq 0) {
        Write-Host "`n✅ Migration operation completed successfully" -ForegroundColor Green
    } else {
        Write-Host "`n❌ Migration operation failed" -ForegroundColor Red
        exit $exitCode
    }
} finally {
    # Cleanup
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
}
"@