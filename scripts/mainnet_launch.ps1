# Mainnet Launch Script for dchat (Windows PowerShell)
#
# This script implements the pre-stake genesis flow to solve the 
# chicken-and-egg problem of mainnet launch.
#
# Flow:
# 1. Coordinator creates pre-stake manifest
# 2. Each validator creates a signed bond commitment  
# 3. Coordinator collects and adds all commitments to manifest
# 4. Coordinator validates manifest (min 4 validators, 3 regions)
# 5. Coordinator generates genesis files
# 6. All validators start with the same genesis files

param(
    [Parameter(Position=0)]
    [string]$Command = "help",
    
    [string]$ChainId = "dchat-mainnet-1",
    [string]$Output,
    [string]$Manifest,
    [string]$Commitment,
    [string]$KeyFile,
    [string]$CoordinatorKey,
    [string]$Name,
    [int64]$Stake,
    [string]$Address,
    [string]$Region,
    [int]$LockupDays = 30,
    [int64]$InitialSupply = 1000000000,
    [int64]$MinStake = 10000,
    [string]$GenesisDir,
    [string]$DataDir = "./data"
)

$ErrorActionPreference = "Stop"
$DchatBin = if ($env:DCHAT_BIN) { $env:DCHAT_BIN } else { "dchat" }

function Write-Header {
    param([string]$Title)
    Write-Host ""
    Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
    Write-Host "  $Title" -ForegroundColor Cyan
    Write-Host "═══════════════════════════════════════════════════════════════" -ForegroundColor Cyan
    Write-Host ""
}

function Show-Help {
    @"
dchat Mainnet Launch Script (PowerShell)

USAGE:
    .\mainnet_launch.ps1 <command> [options]

COMMANDS:
    init-manifest       Create a new pre-stake manifest (coordinator only)
    create-commitment   Create a signed bond commitment (each validator)
    add-commitment      Add a commitment to the manifest (coordinator only)
    validate            Validate the manifest is ready for genesis
    generate-genesis    Generate genesis files from manifest (coordinator only)
    start-validator     Start a validator with genesis files
    full-flow           Run the full mainnet launch flow (interactive)
    help                Show this help message

EXAMPLES:
    # 1. Coordinator initializes manifest
    .\mainnet_launch.ps1 init-manifest -ChainId dchat-mainnet-1 -Output ./manifest.json

    # 2. Each validator creates commitment
    .\mainnet_launch.ps1 create-commitment -KeyFile ./validator.key -Name "Validator-US-East" ``
        -Stake 50000 -Address "validator1.example.com:26656" ``
        -Region us-east -Output ./my-commitment.json

    # 3. Coordinator adds each commitment
    .\mainnet_launch.ps1 add-commitment -Manifest ./manifest.json -Commitment ./validator1.json

    # 4. Validate manifest
    .\mainnet_launch.ps1 validate -Manifest ./manifest.json

    # 5. Generate genesis
    .\mainnet_launch.ps1 generate-genesis -Manifest ./manifest.json -CoordinatorKey ./coord.key ``
        -Output ./genesis/

    # 6. Start validator
    .\mainnet_launch.ps1 start-validator -GenesisDir ./genesis/ -KeyFile ./validator.key

"@
}

function Invoke-InitManifest {
    $outputPath = if ($Output) { $Output } else { "./prestake-manifest.json" }
    
    Write-Header "Initializing Pre-Stake Manifest"

    Write-Host "Chain ID: $ChainId"
    Write-Host "Initial Supply: $InitialSupply DCHAT"
    Write-Host "Minimum Stake: $MinStake DCHAT"
    Write-Host "Output: $outputPath"
    Write-Host ""

    & $DchatBin pre-stake-genesis init-manifest `
        --chain-id $ChainId `
        --output $outputPath `
        --initial-supply $InitialSupply `
        --min-stake $MinStake

    if ($LASTEXITCODE -eq 0) {
        Write-Host ""
        Write-Host "✅ Manifest created successfully!" -ForegroundColor Green
        Write-Host ""
        Write-Host "Next steps:"
        Write-Host "  1. Share the chain ID '$ChainId' with all validators"
        Write-Host "  2. Each validator runs: .\mainnet_launch.ps1 create-commitment -ChainId $ChainId ..."
        Write-Host "  3. Collect all commitment files"
        Write-Host "  4. Run: .\mainnet_launch.ps1 add-commitment for each commitment"
    }
}

function Invoke-CreateCommitment {
    if (-not $KeyFile) { throw "KeyFile is required" }
    if (-not $Name) { throw "Name is required" }
    if (-not $Stake) { throw "Stake is required" }
    if (-not $Address) { throw "Address is required" }
    if (-not $Region) { throw "Region is required" }
    
    $outputPath = if ($Output) { $Output } else { "./my-commitment.json" }

    Write-Header "Creating Bond Commitment"

    Write-Host "Validator Name: $Name"
    Write-Host "Stake Amount: $Stake DCHAT"
    Write-Host "Network Address: $Address"
    Write-Host "Region: $Region"
    Write-Host "Lockup Period: $LockupDays days"
    Write-Host "Chain ID: $ChainId"
    Write-Host ""

    & $DchatBin pre-stake-genesis create-commitment `
        --key-file $KeyFile `
        --name $Name `
        --stake $Stake `
        --address $Address `
        --region $Region `
        --lockup-days $LockupDays `
        --chain-id $ChainId `
        --output $outputPath

    if ($LASTEXITCODE -eq 0) {
        Write-Host ""
        Write-Host "✅ Bond commitment created and signed!" -ForegroundColor Green
        Write-Host "   File: $outputPath"
        Write-Host ""
        Write-Host "⚠️  IMPORTANT: This commitment is cryptographically binding." -ForegroundColor Yellow
        Write-Host "   By sharing this file, you commit to staking $Stake DCHAT at genesis."
        Write-Host ""
        Write-Host "Next step: Send $outputPath to the genesis coordinator"
    }
}

function Invoke-AddCommitment {
    if (-not $Manifest) { throw "Manifest is required" }
    if (-not $Commitment) { throw "Commitment is required" }

    Write-Header "Adding Commitment to Manifest"

    Write-Host "Manifest: $Manifest"
    Write-Host "Commitment: $Commitment"
    Write-Host ""

    & $DchatBin pre-stake-genesis add-commitment `
        --manifest $Manifest `
        --commitment $Commitment

    if ($LASTEXITCODE -eq 0) {
        Write-Host ""
        Write-Host "✅ Commitment added successfully!" -ForegroundColor Green
    }
}

function Invoke-Validate {
    if (-not $Manifest) { throw "Manifest is required" }

    Write-Header "Validating Pre-Stake Manifest"

    Write-Host "Manifest: $Manifest"
    Write-Host ""

    & $DchatBin pre-stake-genesis validate-manifest `
        --manifest $Manifest
}

function Invoke-GenerateGenesis {
    if (-not $Manifest) { throw "Manifest is required" }
    if (-not $CoordinatorKey) { throw "CoordinatorKey is required" }
    
    $outputPath = if ($Output) { $Output } else { "./genesis" }

    Write-Header "Generating Genesis Files"

    Write-Host "Manifest: $Manifest"
    Write-Host "Coordinator Key: $CoordinatorKey"
    Write-Host "Output Directory: $outputPath"
    Write-Host ""

    & $DchatBin pre-stake-genesis generate-genesis `
        --manifest $Manifest `
        --coordinator-key $CoordinatorKey `
        --output $outputPath

    if ($LASTEXITCODE -eq 0) {
        Write-Host ""
        Write-Host "✅ Genesis files generated successfully!" -ForegroundColor Green
        Write-Host ""
        Write-Host "Generated files:"
        Get-ChildItem $outputPath | Format-Table Name, Length
        Write-Host ""
        Write-Host "Next steps:"
        Write-Host "  1. Distribute the genesis directory to ALL validators"
        Write-Host "  2. ALL validators must use the EXACT same genesis files"
        Write-Host "  3. Coordinate a launch time and start all validators"
    }
}

function Invoke-StartValidator {
    if (-not $GenesisDir) { throw "GenesisDir is required" }
    if (-not $KeyFile) { throw "KeyFile is required" }

    Write-Header "Starting Validator"

    Write-Host "Genesis Directory: $GenesisDir"
    Write-Host "Key File: $KeyFile"
    Write-Host "Data Directory: $DataDir"
    Write-Host ""

    # Verify genesis files exist
    $currencyGenesis = Join-Path $GenesisDir "currency_chain_genesis.json"
    $chatGenesis = Join-Path $GenesisDir "chat_chain_genesis.json"
    
    if (-not (Test-Path $currencyGenesis)) {
        throw "currency_chain_genesis.json not found in $GenesisDir"
    }
    if (-not (Test-Path $chatGenesis)) {
        throw "chat_chain_genesis.json not found in $GenesisDir"
    }

    Write-Host "🚀 Starting validator..." -ForegroundColor Green
    Write-Host ""

    & $DchatBin `
        --role validator `
        --data-dir $DataDir `
        --genesis-dir $GenesisDir `
        --key-file $KeyFile
}

function Invoke-FullFlow {
    Write-Header "dchat Mainnet Launch - Full Flow"

    Write-Host "This interactive flow will guide you through the mainnet launch process."
    Write-Host ""
    Write-Host "Are you the genesis COORDINATOR or a VALIDATOR?"
    Write-Host "  1) Coordinator (I'm organizing the launch)"
    Write-Host "  2) Validator (I'm joining the network)"
    Write-Host ""
    $roleChoice = Read-Host "Enter choice [1/2]"

    switch ($roleChoice) {
        "1" { Invoke-CoordinatorFlow }
        "2" { Invoke-ValidatorFlow }
        default { throw "Invalid choice" }
    }
}

function Invoke-CoordinatorFlow {
    Write-Header "Coordinator Flow"

    Write-Host "Step 1: Initialize the pre-stake manifest"
    Write-Host ""
    
    $inputChainId = Read-Host "Chain ID [dchat-mainnet-1]"
    if (-not $inputChainId) { $inputChainId = "dchat-mainnet-1" }
    
    $inputSupply = Read-Host "Initial Supply (DCHAT) [1000000000]"
    if (-not $inputSupply) { $inputSupply = 1000000000 }
    
    $inputMinStake = Read-Host "Minimum Stake (DCHAT) [10000]"
    if (-not $inputMinStake) { $inputMinStake = 10000 }
    
    $inputManifest = Read-Host "Manifest output file [./prestake-manifest.json]"
    if (-not $inputManifest) { $inputManifest = "./prestake-manifest.json" }

    $Script:ChainId = $inputChainId
    $Script:InitialSupply = [int64]$inputSupply
    $Script:MinStake = [int64]$inputMinStake
    $Script:Output = $inputManifest
    Invoke-InitManifest

    Write-Host ""
    Write-Host "Now wait for validators to send their commitment files."
    Write-Host "Once you have at least 4 commitments from 3+ regions, continue."
    Write-Host ""
    Read-Host "Press Enter when ready to add commitments..."

    while ($true) {
        $commitmentPath = Read-Host "Commitment file path (or 'done' to finish)"
        if ($commitmentPath -eq "done") { break }
        
        if (Test-Path $commitmentPath) {
            $Script:Manifest = $inputManifest
            $Script:Commitment = $commitmentPath
            Invoke-AddCommitment
        } else {
            Write-Host "File not found: $commitmentPath" -ForegroundColor Yellow
        }
    }

    Write-Host ""
    Write-Host "Step 2: Validate the manifest"
    $Script:Manifest = $inputManifest
    Invoke-Validate

    Write-Host ""
    Write-Host "Step 3: Generate genesis files"
    $coordKey = Read-Host "Coordinator key file"
    $genesisDir = Read-Host "Genesis output directory [./genesis]"
    if (-not $genesisDir) { $genesisDir = "./genesis" }

    $Script:CoordinatorKey = $coordKey
    $Script:Output = $genesisDir
    Invoke-GenerateGenesis

    Write-Host ""
    Write-Host "🎉 Genesis files are ready!" -ForegroundColor Green
    Write-Host ""
    Write-Host "DISTRIBUTE THESE FILES TO ALL VALIDATORS:"
    Write-Host "  $genesisDir\currency_chain_genesis.json"
    Write-Host "  $genesisDir\chat_chain_genesis.json"
    Write-Host "  $genesisDir\genesis.json"
    Write-Host ""
    Write-Host "Coordinate a specific launch time with all validators."
    Write-Host "All validators must start within a few minutes of each other."
}

function Invoke-ValidatorFlow {
    Write-Header "Validator Flow"

    Write-Host "Step 1: Generate or locate your validator key"
    Write-Host ""
    $keyPath = Read-Host "Key file path (or 'generate' for new key)"

    if ($keyPath -eq "generate") {
        $keyPath = Read-Host "Output path for new key [./validator.key]"
        if (-not $keyPath) { $keyPath = "./validator.key" }
        & $DchatBin keygen --output $keyPath
        Write-Host "✅ Key generated: $keyPath" -ForegroundColor Green
        Write-Host "⚠️  BACKUP THIS KEY SECURELY!" -ForegroundColor Yellow
    }

    Write-Host ""
    Write-Host "Step 2: Create your bond commitment"
    Write-Host ""
    
    $inputChainId = Read-Host "Chain ID (get from coordinator)"
    $inputName = Read-Host "Your validator name"
    $inputStake = Read-Host "Stake amount (DCHAT)"
    $inputAddress = Read-Host "Network address (IP:port or DNS:port)"
    $inputRegion = Read-Host "Geographic region (e.g., us-east, eu-west, ap-south)"
    $inputLockup = Read-Host "Lockup period in days [30]"
    if (-not $inputLockup) { $inputLockup = 30 }
    $inputOutput = Read-Host "Commitment output file [./my-commitment.json]"
    if (-not $inputOutput) { $inputOutput = "./my-commitment.json" }

    $Script:KeyFile = $keyPath
    $Script:ChainId = $inputChainId
    $Script:Name = $inputName
    $Script:Stake = [int64]$inputStake
    $Script:Address = $inputAddress
    $Script:Region = $inputRegion
    $Script:LockupDays = [int]$inputLockup
    $Script:Output = $inputOutput
    Invoke-CreateCommitment

    Write-Host ""
    Write-Host "📤 Send $inputOutput to the genesis coordinator"
    Write-Host ""
    Write-Host "Wait for the coordinator to send you the genesis files."
    Read-Host "Press Enter when you have the genesis files..."

    $genesisDir = Read-Host "Genesis directory path"
    $dataDir = Read-Host "Data directory [./data]"
    if (-not $dataDir) { $dataDir = "./data" }

    $Script:GenesisDir = $genesisDir
    $Script:KeyFile = $keyPath
    $Script:DataDir = $dataDir
    Invoke-StartValidator
}

# Main command dispatch
switch ($Command.ToLower()) {
    "init-manifest"       { Invoke-InitManifest }
    "create-commitment"   { Invoke-CreateCommitment }
    "add-commitment"      { Invoke-AddCommitment }
    "validate"            { Invoke-Validate }
    "generate-genesis"    { Invoke-GenerateGenesis }
    "start-validator"     { Invoke-StartValidator }
    "full-flow"           { Invoke-FullFlow }
    "help"                { Show-Help }
    default               { 
        Write-Host "Unknown command: $Command" -ForegroundColor Red
        Write-Host "Run '.\mainnet_launch.ps1 help' for usage."
        exit 1
    }
}
