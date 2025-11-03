#!/usr/bin/env pwsh
# Migrate from local SQLite storage to distributed multi-tier architecture
# 
# This script:
# 1. Exports data from SQLite
# 2. Transforms schema for CockroachDB
# 3. Imports to distributed storage
# 4. Verifies data integrity
# 5. Updates application configuration

param(
    [Parameter(Mandatory=$false)]
    [string]$SqlitePath = "C:\Users\USER\dchat\data\dchat.db",
    
    [Parameter(Mandatory=$false)]
    [string]$CockroachUrl = "postgresql://dchat:changeme@localhost:26257/dchat",
    
    [Parameter(Mandatory=$false)]
    [switch]$DryRun = $false,
    
    [Parameter(Mandatory=$false)]
    [switch]$SkipVerification = $false
)

$ErrorActionPreference = "Stop"

# Color output functions
function Write-Step { param($msg) Write-Host "➜ $msg" -ForegroundColor Cyan }
function Write-Success { param($msg) Write-Host "✓ $msg" -ForegroundColor Green }
function Write-Failure { param($msg) Write-Host "✗ $msg" -ForegroundColor Red }
function Write-Warning { param($msg) Write-Host "⚠ $msg" -ForegroundColor Yellow }

# ============================================================================
# Prerequisites Check
# ============================================================================

Write-Step "Checking prerequisites..."

# Check for sqlite3
try {
    $sqliteVersion = sqlite3 --version
    Write-Success "SQLite found: $sqliteVersion"
} catch {
    Write-Failure "SQLite not found. Please install: https://www.sqlite.org/download.html"
    exit 1
}

# Check for psql
try {
    $psqlVersion = psql --version
    Write-Success "PostgreSQL client found: $psqlVersion"
} catch {
    Write-Failure "psql not found. Please install PostgreSQL client tools."
    exit 1
}

# Check SQLite database exists
if (-not (Test-Path $SqlitePath)) {
    Write-Failure "SQLite database not found at: $SqlitePath"
    exit 1
}
Write-Success "SQLite database found: $SqlitePath"

# ============================================================================
# Step 1: Export SQLite Data
# ============================================================================

Write-Step "Exporting SQLite data..."

$exportPath = "dchat_export_$(Get-Date -Format 'yyyyMMdd_HHmmss').sql"

if ($DryRun) {
    Write-Warning "DRY RUN: Would export to $exportPath"
} else {
    sqlite3 $SqlitePath ".dump" | Out-File -FilePath $exportPath -Encoding UTF8
    $exportSize = (Get-Item $exportPath).Length / 1MB
    Write-Success "Exported to $exportPath ($([math]::Round($exportSize, 2)) MB)"
}

# ============================================================================
# Step 2: Transform Schema for CockroachDB
# ============================================================================

Write-Step "Transforming schema for CockroachDB..."

$transformedPath = $exportPath -replace '\.sql$', '_transformed.sql'

if ($DryRun) {
    Write-Warning "DRY RUN: Would transform to $transformedPath"
} else {
    $content = Get-Content $exportPath -Raw
    
    # SQLite -> PostgreSQL transformations
    $content = $content -replace 'AUTOINCREMENT', 'SERIAL'
    $content = $content -replace 'INTEGER PRIMARY KEY', 'BIGSERIAL PRIMARY KEY'
    $content = $content -replace 'DATETIME', 'TIMESTAMP'
    $content = $content -replace 'REAL', 'DOUBLE PRECISION'
    
    # Add CockroachDB-specific optimizations
    $content = "-- CockroachDB migration from SQLite`n" + 
               "-- Generated: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')`n`n" +
               "SET sql_safe_updates = false;`n`n" +
               $content
    
    $content | Out-File -FilePath $transformedPath -Encoding UTF8
    Write-Success "Schema transformed: $transformedPath"
}

# ============================================================================
# Step 3: Count Records in SQLite
# ============================================================================

Write-Step "Counting records in SQLite..."

$tables = @("messages", "users", "channels", "identities")
$recordCounts = @{}

foreach ($table in $tables) {
    try {
        $count = sqlite3 $SqlitePath "SELECT COUNT(*) FROM $table" 2>$null
        if ($count -match '^\d+$') {
            $recordCounts[$table] = [int]$count
            Write-Host "  $table`: $count rows" -ForegroundColor Gray
        }
    } catch {
        Write-Warning "Table $table not found or error: $_"
    }
}

$totalRecords = ($recordCounts.Values | Measure-Object -Sum).Sum
Write-Success "Total records in SQLite: $totalRecords"

# ============================================================================
# Step 4: Import to CockroachDB
# ============================================================================

Write-Step "Importing to CockroachDB..."

if ($DryRun) {
    Write-Warning "DRY RUN: Would import $totalRecords records to $CockroachUrl"
} else {
    try {
        # Test connection first
        $testQuery = "SELECT version();"
        $version = psql $CockroachUrl -t -c $testQuery 2>&1
        Write-Success "Connected to CockroachDB: $version"
        
        # Import data
        Write-Host "  Importing data (this may take several minutes)..." -ForegroundColor Gray
        psql $CockroachUrl -f $transformedPath 2>&1 | Out-Null
        
        if ($LASTEXITCODE -eq 0) {
            Write-Success "Data imported successfully"
        } else {
            Write-Failure "Import failed with exit code $LASTEXITCODE"
            exit 1
        }
    } catch {
        Write-Failure "Failed to connect or import: $_"
        exit 1
    }
}

# ============================================================================
# Step 5: Verify Data Integrity
# ============================================================================

if (-not $SkipVerification -and -not $DryRun) {
    Write-Step "Verifying data integrity..."
    
    $allMatch = $true
    
    foreach ($table in $tables) {
        if ($recordCounts.ContainsKey($table)) {
            try {
                $cockroachCount = psql $CockroachUrl -t -c "SELECT COUNT(*) FROM $table;" | Out-String
                $cockroachCount = [int]($cockroachCount.Trim())
                
                if ($cockroachCount -eq $recordCounts[$table]) {
                    Write-Success "  $table`: $cockroachCount rows (match)"
                } else {
                    Write-Failure "  $table`: SQLite=$($recordCounts[$table]), CockroachDB=$cockroachCount (MISMATCH)"
                    $allMatch = $false
                }
            } catch {
                Write-Warning "  $table`: Could not verify - $_"
            }
        }
    }
    
    if ($allMatch) {
        Write-Success "✓ All tables verified successfully"
    } else {
        Write-Failure "✗ Data integrity verification failed"
        exit 1
    }
}

# ============================================================================
# Step 6: Update Application Configuration
# ============================================================================

Write-Step "Updating application configuration..."

$configPath = "C:\Users\USER\dchat\config.toml"
$distributedConfigPath = "C:\Users\USER\dchat\config\storage-distributed.toml"

if ($DryRun) {
    Write-Warning "DRY RUN: Would update $configPath with distributed storage settings"
} else {
    if (Test-Path $distributedConfigPath) {
        Copy-Item $distributedConfigPath $configPath -Force
        Write-Success "Configuration updated to use distributed storage"
    } else {
        Write-Warning "Distributed config not found: $distributedConfigPath"
        Write-Warning "Please manually update $configPath"
    }
}

# ============================================================================
# Step 7: Deploy to Kubernetes (Optional)
# ============================================================================

Write-Step "Checking Kubernetes deployment..."

try {
    $kubectlVersion = kubectl version --client --short 2>$null
    Write-Success "kubectl found: $kubectlVersion"
    
    if (-not $DryRun) {
        Write-Host "`nDo you want to deploy distributed storage to Kubernetes? (y/n): " -NoNewline -ForegroundColor Cyan
        $response = Read-Host
        
        if ($response -eq 'y') {
            Write-Step "Deploying storage infrastructure..."
            
            kubectl apply -f k8s\storage-distributed.yaml
            
            if ($LASTEXITCODE -eq 0) {
                Write-Success "Storage infrastructure deployed to Kubernetes"
                
                Write-Step "Waiting for pods to be ready (this may take 5-10 minutes)..."
                kubectl wait --for=condition=ready pod -l app=cockroachdb -n dchat-storage --timeout=600s
                kubectl wait --for=condition=ready pod -l app=redis-cluster -n dchat-storage --timeout=300s
                kubectl wait --for=condition=ready pod -l app=minio -n dchat-storage --timeout=300s
                kubectl wait --for=condition=ready pod -l app=tikv -n dchat-storage --timeout=600s
                
                Write-Success "All storage pods are ready"
            } else {
                Write-Failure "Kubernetes deployment failed"
            }
        } else {
            Write-Warning "Skipping Kubernetes deployment"
        }
    }
} catch {
    Write-Warning "kubectl not found - skipping Kubernetes deployment"
}

# ============================================================================
# Summary
# ============================================================================

Write-Host "`n" + ("=" * 80) -ForegroundColor Cyan
Write-Host "Migration Summary" -ForegroundColor Cyan
Write-Host ("=" * 80) -ForegroundColor Cyan

Write-Host "Source: " -NoNewline
Write-Host $SqlitePath -ForegroundColor Yellow

Write-Host "Destination: " -NoNewline
Write-Host $CockroachUrl -ForegroundColor Yellow

Write-Host "Total records migrated: " -NoNewline
Write-Host $totalRecords -ForegroundColor Green

if ($DryRun) {
    Write-Warning "`nThis was a DRY RUN - no actual changes were made"
    Write-Host "Run without -DryRun to perform actual migration" -ForegroundColor Gray
} else {
    Write-Success "`n✓ Migration completed successfully!"
}

Write-Host "`nNext steps:" -ForegroundColor Cyan
Write-Host "  1. Update connection strings in your application" -ForegroundColor Gray
Write-Host "  2. Deploy validators with new storage backend" -ForegroundColor Gray
Write-Host "  3. Monitor storage health: kubectl get pods -n dchat-storage" -ForegroundColor Gray
Write-Host "  4. Set up backups with scripts\backup-distributed-storage.ps1" -ForegroundColor Gray

Write-Host "`n" + ("=" * 80) -ForegroundColor Cyan
