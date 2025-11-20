//! Currency chain transaction parser
//!
//! Parses currency chain blocks and extracts transaction information
//! for balance tracking, payment verification, and staking operations.

use crate::currency_transactions::*;
use dchat_core::types::UserId;
use dchat_core::{Error, Result};
use serde_json::Value;
use uuid::Uuid;

/// Transaction parser for currency chain blocks
pub struct CurrencyTransactionParser;

impl CurrencyTransactionParser {
    /// Parse transactions from a currency chain block
    pub fn parse_block_transactions(block_data: &Value) -> Result<Vec<ParsedTransaction>> {
        let empty_vec = vec![];
        let tx_array = block_data["transactions"]
            .as_array()
            .unwrap_or(&empty_vec);

        let mut transactions = Vec::new();

        for tx_data in tx_array {
            match Self::parse_transaction(tx_data) {
                Ok(tx) => transactions.push(tx),
                Err(e) => {
                    tracing::warn!("Failed to parse transaction: {}", e);
                    continue;
                }
            }
        }

        Ok(transactions)
    }

    /// Parse a single transaction
    fn parse_transaction(tx_data: &Value) -> Result<ParsedTransaction> {
        // Get transaction type
        let tx_type_str = tx_data["type"]
            .as_str()
            .ok_or_else(|| Error::validation("Missing transaction type"))?;

        let tx_type = Self::parse_tx_type(tx_type_str)?;

        // Get common fields
        let tx_hash = tx_data["hash"]
            .as_str()
            .ok_or_else(|| Error::validation("Missing transaction hash"))?
            .to_string();

        let from_str = tx_data["from"].as_str();
        let to_str = tx_data["to"].as_str();

        // Parse specific transaction based on type
        match tx_type {
            CurrencyTransactionType::Transfer => {
                Self::parse_transfer(tx_data, &tx_hash, from_str, to_str)
            }
            CurrencyTransactionType::Stake => Self::parse_stake(tx_data, &tx_hash, from_str),
            CurrencyTransactionType::Unstake => Self::parse_unstake(tx_data, &tx_hash, from_str),
            CurrencyTransactionType::Delegate => {
                Self::parse_delegate(tx_data, &tx_hash, from_str, to_str)
            }
            CurrencyTransactionType::Undelegate => {
                Self::parse_undelegate(tx_data, &tx_hash, from_str, to_str)
            }
            CurrencyTransactionType::ClaimRewards => {
                Self::parse_claim_rewards(tx_data, &tx_hash, from_str)
            }
            CurrencyTransactionType::Slash => Self::parse_slash(tx_data, &tx_hash),
            CurrencyTransactionType::BlockReward => Self::parse_block_reward(tx_data, &tx_hash),
            CurrencyTransactionType::RelayPayment => {
                Self::parse_relay_payment(tx_data, &tx_hash, from_str, to_str)
            }
            CurrencyTransactionType::ChannelAccess => {
                Self::parse_channel_access(tx_data, &tx_hash, from_str, to_str)
            }
        }
    }

    /// Parse transaction type string
    fn parse_tx_type(tx_type_str: &str) -> Result<CurrencyTransactionType> {
        match tx_type_str.to_lowercase().as_str() {
            "transfer" | "send" | "payment" => Ok(CurrencyTransactionType::Transfer),
            "stake" => Ok(CurrencyTransactionType::Stake),
            "unstake" => Ok(CurrencyTransactionType::Unstake),
            "delegate" => Ok(CurrencyTransactionType::Delegate),
            "undelegate" => Ok(CurrencyTransactionType::Undelegate),
            "claimrewards" | "claim_rewards" => Ok(CurrencyTransactionType::ClaimRewards),
            "slash" | "slashing" => Ok(CurrencyTransactionType::Slash),
            "blockreward" | "block_reward" | "coinbase" => {
                Ok(CurrencyTransactionType::BlockReward)
            }
            "relaypayment" | "relay_payment" => Ok(CurrencyTransactionType::RelayPayment),
            "channelaccess" | "channel_access" => Ok(CurrencyTransactionType::ChannelAccess),
            _ => Err(Error::validation(format!(
                "Unknown transaction type: {}",
                tx_type_str
            ))),
        }
    }

    /// Parse transfer transaction
    fn parse_transfer(
        tx_data: &Value,
        tx_hash: &str,
        from_str: Option<&str>,
        to_str: Option<&str>,
    ) -> Result<ParsedTransaction> {
        let from = from_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid from address"))?;

        let to = to_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid to address"))?;

        let amount = Self::parse_amount(tx_data["value"].as_str())?;
        let fee = tx_data["fee"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let memo = tx_data["memo"].as_str().map(|s| s.to_string());

        let tx = TransferTx {
            tx_id: Uuid::new_v4(),
            from,
            to,
            amount,
            memo,
            fee,
            timestamp: chrono::Utc::now(),
            nonce: tx_data["nonce"].as_u64().unwrap_or(0),
            signature: Self::parse_signature(tx_data),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::Transfer,
            data: TransactionData::Transfer(tx),
        })
    }

    /// Parse stake transaction
    fn parse_stake(
        tx_data: &Value,
        tx_hash: &str,
        from_str: Option<&str>,
    ) -> Result<ParsedTransaction> {
        let staker = from_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid staker address"))?;

        let amount = Self::parse_amount(tx_data["value"].as_str())?;

        let stake_type_str = tx_data["stakeType"]
            .as_str()
            .unwrap_or(tx_data["stake_type"].as_str().unwrap_or("validator"));

        let stake_type = match stake_type_str.to_lowercase().as_str() {
            "validator" => StakeType::Validator,
            "relay" => StakeType::Relay,
            "reputation" => StakeType::Reputation,
            _ => StakeType::Validator,
        };

        let lock_duration_days = tx_data["lockDuration"]
            .as_u64()
            .or_else(|| tx_data["lock_duration"].as_u64())
            .map(|d| d as u32);

        let tx = StakeTx {
            tx_id: Uuid::new_v4(),
            staker,
            amount,
            stake_type,
            lock_duration_days,
            timestamp: chrono::Utc::now(),
            nonce: tx_data["nonce"].as_u64().unwrap_or(0),
            signature: Self::parse_signature(tx_data),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::Stake,
            data: TransactionData::Stake(tx),
        })
    }

    /// Parse unstake transaction
    fn parse_unstake(
        tx_data: &Value,
        tx_hash: &str,
        from_str: Option<&str>,
    ) -> Result<ParsedTransaction> {
        let unstaker = from_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid unstaker address"))?;

        let amount = Self::parse_amount(tx_data["value"].as_str())?;

        let stake_type_str = tx_data["stakeType"]
            .as_str()
            .unwrap_or(tx_data["stake_type"].as_str().unwrap_or("validator"));

        let stake_type = match stake_type_str.to_lowercase().as_str() {
            "validator" => StakeType::Validator,
            "relay" => StakeType::Relay,
            "reputation" => StakeType::Reputation,
            _ => StakeType::Validator,
        };

        // Unstaking typically has a 7-day timelock
        let unlock_at = chrono::Utc::now() + chrono::Duration::days(7);

        let tx = UnstakeTx {
            tx_id: Uuid::new_v4(),
            unstaker,
            amount,
            stake_type,
            unlock_at,
            timestamp: chrono::Utc::now(),
            nonce: tx_data["nonce"].as_u64().unwrap_or(0),
            signature: Self::parse_signature(tx_data),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::Unstake,
            data: TransactionData::Unstake(tx),
        })
    }

    /// Parse delegate transaction
    fn parse_delegate(
        tx_data: &Value,
        tx_hash: &str,
        from_str: Option<&str>,
        to_str: Option<&str>,
    ) -> Result<ParsedTransaction> {
        let delegator = from_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid delegator address"))?;

        let validator = to_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid validator address"))?;

        let amount = Self::parse_amount(tx_data["value"].as_str())?;

        let tx = DelegateTx {
            tx_id: Uuid::new_v4(),
            delegator,
            validator,
            amount,
            timestamp: chrono::Utc::now(),
            nonce: tx_data["nonce"].as_u64().unwrap_or(0),
            signature: Self::parse_signature(tx_data),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::Delegate,
            data: TransactionData::Delegate(tx),
        })
    }

    /// Parse undelegate transaction
    fn parse_undelegate(
        tx_data: &Value,
        tx_hash: &str,
        from_str: Option<&str>,
        to_str: Option<&str>,
    ) -> Result<ParsedTransaction> {
        let delegator = from_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid delegator address"))?;

        let validator = to_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid validator address"))?;

        let amount = Self::parse_amount(tx_data["value"].as_str())?;

        // Undelegation typically has a 7-day timelock
        let unlock_at = chrono::Utc::now() + chrono::Duration::days(7);

        let tx = UndelegateTx {
            tx_id: Uuid::new_v4(),
            delegator,
            validator,
            amount,
            unlock_at,
            timestamp: chrono::Utc::now(),
            nonce: tx_data["nonce"].as_u64().unwrap_or(0),
            signature: Self::parse_signature(tx_data),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::Undelegate,
            data: TransactionData::Undelegate(tx),
        })
    }

    /// Parse claim rewards transaction
    fn parse_claim_rewards(
        tx_data: &Value,
        tx_hash: &str,
        from_str: Option<&str>,
    ) -> Result<ParsedTransaction> {
        let claimer = from_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid claimer address"))?;

        let amount = Self::parse_amount(tx_data["value"].as_str())?;

        let reward_type_str = tx_data["rewardType"]
            .as_str()
            .unwrap_or(tx_data["reward_type"].as_str().unwrap_or("block_production"));

        let reward_type = match reward_type_str.to_lowercase().as_str() {
            "block_production" | "block" => RewardType::BlockProduction,
            "message_relay" | "relay" => RewardType::MessageRelay,
            "staking_delegation" | "staking" | "delegation" => RewardType::StakingDelegation,
            "proof_of_delivery" | "pod" => RewardType::ProofOfDelivery,
            _ => RewardType::BlockProduction,
        };

        let from_height = tx_data["fromHeight"].as_u64().unwrap_or(0);
        let to_height = tx_data["toHeight"].as_u64().unwrap_or(from_height);

        let tx = ClaimRewardsTx {
            tx_id: Uuid::new_v4(),
            claimer,
            reward_type,
            amount,
            from_height,
            to_height,
            timestamp: chrono::Utc::now(),
            nonce: tx_data["nonce"].as_u64().unwrap_or(0),
            signature: Self::parse_signature(tx_data),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::ClaimRewards,
            data: TransactionData::ClaimRewards(tx),
        })
    }

    /// Parse slash transaction
    fn parse_slash(tx_data: &Value, tx_hash: &str) -> Result<ParsedTransaction> {
        let validator_str = tx_data["validator"]
            .as_str()
            .ok_or_else(|| Error::validation("Missing validator in slash tx"))?;

        let validator = Uuid::parse_str(validator_str)
            .map(UserId)
            .map_err(|_| Error::validation("Invalid validator address"))?;

        let slash_amount = Self::parse_amount(tx_data["slashAmount"].as_str())?;
        let remaining_stake = Self::parse_amount(tx_data["remainingStake"].as_str())?;

        let reason_str = tx_data["reason"]
            .as_str()
            .unwrap_or("byzantine");

        let reason = match reason_str.to_lowercase().as_str() {
            "double_sign" | "doublesign" => SlashReason::DoubleSign,
            "downtime" => SlashReason::Downtime,
            "invalid_proof" | "invalidproof" => SlashReason::InvalidProof,
            "censorship" => SlashReason::Censorship,
            "byzantine" => SlashReason::Byzantine,
            _ => SlashReason::Byzantine,
        };

        let evidence_hash = tx_data["evidenceHash"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let authorized_by_str = tx_data["authorizedBy"]
            .as_str()
            .unwrap_or("");

        let authorized_by = Uuid::parse_str(authorized_by_str).unwrap_or_else(|_| Uuid::new_v4());

        let tx = SlashTx {
            tx_id: Uuid::new_v4(),
            validator,
            reason,
            slash_amount,
            remaining_stake,
            evidence_hash,
            timestamp: chrono::Utc::now(),
            authorized_by,
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::Slash,
            data: TransactionData::Slash(tx),
        })
    }

    /// Parse block reward transaction
    fn parse_block_reward(tx_data: &Value, tx_hash: &str) -> Result<ParsedTransaction> {
        let block_height = tx_data["blockHeight"]
            .as_u64()
            .ok_or_else(|| Error::validation("Missing block height"))?;

        let proposer_str = tx_data["proposer"]
            .as_str()
            .ok_or_else(|| Error::validation("Missing proposer"))?;

        let proposer = Uuid::parse_str(proposer_str)
            .map(UserId)
            .map_err(|_| Error::validation("Invalid proposer address"))?;

        let total_reward = Self::parse_amount(tx_data["totalReward"].as_str())?;
        let proposer_reward = Self::parse_amount(tx_data["proposerReward"].as_str())?;

        // Parse validator rewards array
        let validator_rewards = if let Some(rewards_array) = tx_data["validatorRewards"].as_array() {
            rewards_array
                .iter()
                .filter_map(|reward| {
                    let validator_str = reward["validator"].as_str()?;
                    let amount_str = reward["amount"].as_str()?;
                    
                    let validator_id = Uuid::parse_str(validator_str)
                        .ok()
                        .map(UserId)?;
                    let amount = Self::parse_amount(Some(amount_str)).ok()?;
                    
                    Some((validator_id, amount))
                })
                .collect()
        } else {
            Vec::new()
        };

        let tx = BlockRewardTx {
            tx_id: Uuid::new_v4(),
            block_height,
            proposer,
            total_reward,
            proposer_reward,
            validator_rewards,
            timestamp: chrono::Utc::now(),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::BlockReward,
            data: TransactionData::BlockReward(tx),
        })
    }

    /// Parse relay payment transaction
    fn parse_relay_payment(
        tx_data: &Value,
        tx_hash: &str,
        from_str: Option<&str>,
        to_str: Option<&str>,
    ) -> Result<ParsedTransaction> {
        let payer = from_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid payer address"))?;

        let relay = to_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid relay address"))?;

        let amount = Self::parse_amount(tx_data["value"].as_str())?;
        let message_count = tx_data["messageCount"].as_u64().unwrap_or(1) as u32;
        let bytes_relayed = tx_data["bytesRelayed"].as_u64().unwrap_or(0);

        let pod_hash = tx_data["podHash"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let tx = RelayPaymentTx {
            tx_id: Uuid::new_v4(),
            payer,
            relay,
            message_count,
            bytes_relayed,
            amount,
            pod_hash,
            timestamp: chrono::Utc::now(),
            nonce: tx_data["nonce"].as_u64().unwrap_or(0),
            signature: Self::parse_signature(tx_data),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::RelayPayment,
            data: TransactionData::RelayPayment(tx),
        })
    }

    /// Parse channel access payment transaction
    fn parse_channel_access(
        tx_data: &Value,
        tx_hash: &str,
        from_str: Option<&str>,
        to_str: Option<&str>,
    ) -> Result<ParsedTransaction> {
        let user = from_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid user address"))?;

        let creator = to_str
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(UserId)
            .ok_or_else(|| Error::validation("Invalid creator address"))?;

        let channel_id_str = tx_data["channelId"]
            .as_str()
            .ok_or_else(|| Error::validation("Missing channel ID"))?;

        let channel_id = Uuid::parse_str(channel_id_str)
            .map_err(|_| Error::validation("Invalid channel ID"))?;

        let amount = Self::parse_amount(tx_data["value"].as_str())?;
        let duration_days = tx_data["durationDays"].as_u64().unwrap_or(30) as u32;

        let tx = ChannelAccessTx {
            tx_id: Uuid::new_v4(),
            user,
            channel_id,
            creator,
            amount,
            duration_days,
            timestamp: chrono::Utc::now(),
            nonce: tx_data["nonce"].as_u64().unwrap_or(0),
            signature: Self::parse_signature(tx_data),
        };

        Ok(ParsedTransaction {
            tx_hash: tx_hash.to_string(),
            tx_type: CurrencyTransactionType::ChannelAccess,
            data: TransactionData::ChannelAccess(tx),
        })
    }

    /// Parse amount from string (supports hex and decimal)
    fn parse_amount(amount_str: Option<&str>) -> Result<u128> {
        let amount_str = amount_str.ok_or_else(|| Error::validation("Missing amount"))?;

        if amount_str.starts_with("0x") {
            u128::from_str_radix(&amount_str[2..], 16)
                .map_err(|_| Error::validation("Invalid hex amount"))
        } else {
            amount_str
                .parse::<u128>()
                .map_err(|_| Error::validation("Invalid decimal amount"))
        }
    }

    /// Parse signature from transaction data
    fn parse_signature(tx_data: &Value) -> Vec<u8> {
        if let Some(sig_str) = tx_data["signature"].as_str() {
            if sig_str.starts_with("0x") {
                hex::decode(&sig_str[2..]).unwrap_or_default()
            } else {
                hex::decode(sig_str).unwrap_or_default()
            }
        } else {
            Vec::new()
        }
    }
}

/// Parsed transaction wrapper
#[derive(Debug, Clone)]
pub struct ParsedTransaction {
    pub tx_hash: String,
    pub tx_type: CurrencyTransactionType,
    pub data: TransactionData,
}

/// Transaction data enum
#[derive(Debug, Clone)]
pub enum TransactionData {
    Transfer(TransferTx),
    Stake(StakeTx),
    Unstake(UnstakeTx),
    Delegate(DelegateTx),
    Undelegate(UndelegateTx),
    ClaimRewards(ClaimRewardsTx),
    Slash(SlashTx),
    BlockReward(BlockRewardTx),
    RelayPayment(RelayPaymentTx),
    ChannelAccess(ChannelAccessTx),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_transfer_transaction() {
        let tx_data = json!({
            "type": "transfer",
            "hash": "0xabcd1234",
            "from": "550e8400-e29b-41d4-a716-446655440000",
            "to": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "value": "1000",
            "fee": "10",
            "nonce": 1,
            "signature": "0xdeadbeef"
        });

        let result = CurrencyTransactionParser::parse_transaction(&tx_data);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.tx_type, CurrencyTransactionType::Transfer);
    }

    #[test]
    fn test_parse_stake_transaction() {
        let tx_data = json!({
            "type": "stake",
            "hash": "0x1234abcd",
            "from": "550e8400-e29b-41d4-a716-446655440000",
            "value": "10000",
            "stakeType": "validator",
            "lockDuration": 30,
            "nonce": 2
        });

        let result = CurrencyTransactionParser::parse_transaction(&tx_data);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.tx_type, CurrencyTransactionType::Stake);
    }

    #[test]
    fn test_parse_amount_hex() {
        let result = CurrencyTransactionParser::parse_amount(Some("0x3e8"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1000);
    }

    #[test]
    fn test_parse_amount_decimal() {
        let result = CurrencyTransactionParser::parse_amount(Some("1000"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1000);
    }
}
