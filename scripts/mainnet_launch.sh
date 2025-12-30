#!/bin/bash
#
# Mainnet Launch Script for dchat
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
#
# Usage: ./mainnet_launch.sh <command> [options]
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DCHAT_BIN="${DCHAT_BIN:-dchat}"
DEFAULT_CHAIN_ID="dchat-mainnet-1"
DEFAULT_INITIAL_SUPPLY="1000000000" # 1 billion DCHAT
DEFAULT_MIN_STAKE="10000"           # 10,000 DCHAT minimum

print_header() {
	echo ""
	echo "═══════════════════════════════════════════════════════════════"
	echo "  $1"
	echo "═══════════════════════════════════════════════════════════════"
	echo ""
}

print_step() {
	echo "📋 Step $1: $2"
}

cmd_help() {
	cat <<EOF
dchat Mainnet Launch Script

USAGE:
    $0 <command> [options]

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
    $0 init-manifest --chain-id dchat-mainnet-1 --output ./manifest.json

    # 2. Each validator creates commitment
    $0 create-commitment --key-file ./validator.key --name "Validator-US-East" \\
        --stake 50000 --address "validator1.example.com:26656" \\
        --region us-east --output ./my-commitment.json

    # 3. Coordinator adds each commitment
    $0 add-commitment --manifest ./manifest.json --commitment ./validator1.json

    # 4. Validate manifest
    $0 validate --manifest ./manifest.json

    # 5. Generate genesis
    $0 generate-genesis --manifest ./manifest.json --coordinator-key ./coord.key \\
        --output ./genesis/

    # 6. Start validator
    $0 start-validator --genesis-dir ./genesis/ --key-file ./validator.key

EOF
}

cmd_init_manifest() {
	local chain_id="${1:-$DEFAULT_CHAIN_ID}"
	local output="${2:-./prestake-manifest.json}"
	local initial_supply="${3:-$DEFAULT_INITIAL_SUPPLY}"
	local min_stake="${4:-$DEFAULT_MIN_STAKE}"

	print_header "Initializing Pre-Stake Manifest"

	echo "Chain ID: $chain_id"
	echo "Initial Supply: $initial_supply DCHAT"
	echo "Minimum Stake: $min_stake DCHAT"
	echo "Output: $output"
	echo ""

	$DCHAT_BIN pre-stake-genesis init-manifest \
		--chain-id "$chain_id" \
		--output "$output" \
		--initial-supply "$initial_supply" \
		--min-stake "$min_stake"

	echo ""
	echo "✅ Manifest created successfully!"
	echo ""
	echo "Next steps:"
	echo "  1. Share the chain ID '$chain_id' with all validators"
	echo "  2. Each validator runs: $0 create-commitment --chain-id $chain_id ..."
	echo "  3. Collect all commitment files"
	echo "  4. Run: $0 add-commitment for each commitment"
}

cmd_create_commitment() {
	local key_file="$1"
	local name="$2"
	local stake="$3"
	local address="$4"
	local region="$5"
	local lockup="${6:-30}"
	local chain_id="${7:-$DEFAULT_CHAIN_ID}"
	local output="${8:-./my-commitment.json}"

	print_header "Creating Bond Commitment"

	echo "Validator Name: $name"
	echo "Stake Amount: $stake DCHAT"
	echo "Network Address: $address"
	echo "Region: $region"
	echo "Lockup Period: $lockup days"
	echo "Chain ID: $chain_id"
	echo ""

	$DCHAT_BIN pre-stake-genesis create-commitment \
		--key-file "$key_file" \
		--name "$name" \
		--stake "$stake" \
		--address "$address" \
		--region "$region" \
		--lockup-days "$lockup" \
		--chain-id "$chain_id" \
		--output "$output"

	echo ""
	echo "✅ Bond commitment created and signed!"
	echo "   File: $output"
	echo ""
	echo "⚠️  IMPORTANT: This commitment is cryptographically binding."
	echo "   By sharing this file, you commit to staking $stake DCHAT at genesis."
	echo ""
	echo "Next step: Send $output to the genesis coordinator"
}

cmd_add_commitment() {
	local manifest="$1"
	local commitment="$2"

	print_header "Adding Commitment to Manifest"

	echo "Manifest: $manifest"
	echo "Commitment: $commitment"
	echo ""

	$DCHAT_BIN pre-stake-genesis add-commitment \
		--manifest "$manifest" \
		--commitment "$commitment"

	echo ""
	echo "✅ Commitment added successfully!"
}

cmd_validate() {
	local manifest="$1"

	print_header "Validating Pre-Stake Manifest"

	echo "Manifest: $manifest"
	echo ""

	$DCHAT_BIN pre-stake-genesis validate-manifest \
		--manifest "$manifest"
}

cmd_generate_genesis() {
	local manifest="$1"
	local coordinator_key="$2"
	local output="${3:-./genesis}"

	print_header "Generating Genesis Files"

	echo "Manifest: $manifest"
	echo "Coordinator Key: $coordinator_key"
	echo "Output Directory: $output"
	echo ""

	$DCHAT_BIN pre-stake-genesis generate-genesis \
		--manifest "$manifest" \
		--coordinator-key "$coordinator_key" \
		--output "$output"

	echo ""
	echo "✅ Genesis files generated successfully!"
	echo ""
	echo "Generated files:"
	ls -la "$output/"
	echo ""
	echo "Next steps:"
	echo "  1. Distribute the genesis directory to ALL validators"
	echo "  2. ALL validators must use the EXACT same genesis files"
	echo "  3. Coordinate a launch time and start all validators"
}

cmd_start_validator() {
	local genesis_dir="$1"
	local key_file="$2"
	local data_dir="${3:-./data}"

	print_header "Starting Validator"

	echo "Genesis Directory: $genesis_dir"
	echo "Key File: $key_file"
	echo "Data Directory: $data_dir"
	echo ""

	# Verify genesis files exist
	if [[ ! -f "$genesis_dir/currency_chain_genesis.json" ]]; then
		echo "❌ Error: currency_chain_genesis.json not found in $genesis_dir"
		exit 1
	fi
	if [[ ! -f "$genesis_dir/chat_chain_genesis.json" ]]; then
		echo "❌ Error: chat_chain_genesis.json not found in $genesis_dir"
		exit 1
	fi

	echo "🚀 Starting validator..."
	echo ""

	exec $DCHAT_BIN \
		--role validator \
		--data-dir "$data_dir" \
		--genesis-dir "$genesis_dir" \
		--key-file "$key_file"
}

cmd_full_flow() {
	print_header "dchat Mainnet Launch - Full Flow"

	echo "This interactive flow will guide you through the mainnet launch process."
	echo ""
	echo "Are you the genesis COORDINATOR or a VALIDATOR?"
	echo "  1) Coordinator (I'm organizing the launch)"
	echo "  2) Validator (I'm joining the network)"
	echo ""
	read -p "Enter choice [1/2]: " role_choice

	case $role_choice in
	1)
		coordinator_flow
		;;
	2)
		validator_flow
		;;
	*)
		echo "Invalid choice. Exiting."
		exit 1
		;;
	esac
}

coordinator_flow() {
	print_header "Coordinator Flow"

	echo "Step 1: Initialize the pre-stake manifest"
	echo ""
	read -p "Chain ID [$DEFAULT_CHAIN_ID]: " chain_id
	chain_id="${chain_id:-$DEFAULT_CHAIN_ID}"

	read -p "Initial Supply (DCHAT) [$DEFAULT_INITIAL_SUPPLY]: " supply
	supply="${supply:-$DEFAULT_INITIAL_SUPPLY}"

	read -p "Minimum Stake (DCHAT) [$DEFAULT_MIN_STAKE]: " min_stake
	min_stake="${min_stake:-$DEFAULT_MIN_STAKE}"

	read -p "Manifest output file [./prestake-manifest.json]: " manifest
	manifest="${manifest:-./prestake-manifest.json}"

	cmd_init_manifest "$chain_id" "$manifest" "$supply" "$min_stake"

	echo ""
	echo "Now wait for validators to send their commitment files."
	echo "Once you have at least 4 commitments from 3+ regions, continue."
	echo ""
	read -p "Press Enter when ready to add commitments..."

	while true; do
		read -p "Commitment file path (or 'done' to finish): " commitment
		if [[ $commitment == "done" ]]; then
			break
		fi
		if [[ -f $commitment ]]; then
			cmd_add_commitment "$manifest" "$commitment"
		else
			echo "File not found: $commitment"
		fi
	done

	echo ""
	echo "Step 2: Validate the manifest"
	cmd_validate "$manifest"

	echo ""
	echo "Step 3: Generate genesis files"
	read -p "Coordinator key file: " coord_key
	read -p "Genesis output directory [./genesis]: " genesis_dir
	genesis_dir="${genesis_dir:-./genesis}"

	cmd_generate_genesis "$manifest" "$coord_key" "$genesis_dir"

	echo ""
	echo "🎉 Genesis files are ready!"
	echo ""
	echo "DISTRIBUTE THESE FILES TO ALL VALIDATORS:"
	echo "  $genesis_dir/currency_chain_genesis.json"
	echo "  $genesis_dir/chat_chain_genesis.json"
	echo "  $genesis_dir/genesis.json"
	echo ""
	echo "Coordinate a specific launch time with all validators."
	echo "All validators must start within a few minutes of each other."
}

validator_flow() {
	print_header "Validator Flow"

	echo "Step 1: Generate or locate your validator key"
	echo ""
	read -p "Key file path (or 'generate' for new key): " key_file

	if [[ $key_file == "generate" ]]; then
		read -p "Output path for new key [./validator.key]: " key_file
		key_file="${key_file:-./validator.key}"
		$DCHAT_BIN keygen --output "$key_file"
		echo "✅ Key generated: $key_file"
		echo "⚠️  BACKUP THIS KEY SECURELY!"
	fi

	echo ""
	echo "Step 2: Create your bond commitment"
	echo ""
	read -p "Chain ID (get from coordinator): " chain_id
	read -p "Your validator name: " name
	read -p "Stake amount (DCHAT): " stake
	read -p "Network address (IP:port or DNS:port): " address
	read -p "Geographic region (e.g., us-east, eu-west, ap-south): " region
	read -p "Lockup period in days [30]: " lockup
	lockup="${lockup:-30}"
	read -p "Commitment output file [./my-commitment.json]: " output
	output="${output:-./my-commitment.json}"

	cmd_create_commitment "$key_file" "$name" "$stake" "$address" "$region" "$lockup" "$chain_id" "$output"

	echo ""
	echo "📤 Send $output to the genesis coordinator"
	echo ""
	echo "Wait for the coordinator to send you the genesis files."
	read -p "Press Enter when you have the genesis files..."

	read -p "Genesis directory path: " genesis_dir
	read -p "Data directory [./data]: " data_dir
	data_dir="${data_dir:-./data}"

	cmd_start_validator "$genesis_dir" "$key_file" "$data_dir"
}

# Main command dispatch
case "${1:-help}" in
init-manifest)
	shift
	cmd_init_manifest "$@"
	;;
create-commitment)
	shift
	cmd_create_commitment "$@"
	;;
add-commitment)
	shift
	cmd_add_commitment "$@"
	;;
validate)
	shift
	cmd_validate "$@"
	;;
generate-genesis)
	shift
	cmd_generate_genesis "$@"
	;;
start-validator)
	shift
	cmd_start_validator "$@"
	;;
full-flow)
	cmd_full_flow
	;;
help | --help | -h)
	cmd_help
	;;
*)
	echo "Unknown command: $1"
	echo "Run '$0 help' for usage."
	exit 1
	;;
esac
