// Rewards CLI Command Handlers
//
// Dedicated handler functions for rewards subcommands.
// Extracted from main.rs for better maintainability.

use dchat_blockchain::currency_chain::{CurrencyChainClient, CurrencyChainConfig};
use dchat_core::error::{Error, Result};
use dchat_core::UserId;
use uuid::Uuid;

/// Format token amounts for display (assumes 6 decimal places)
fn format_tokens(amount: u64) -> String {
    let whole = amount / 1_000_000;
    let frac = amount % 1_000_000;
    if frac == 0 {
        format!("{}", whole)
    } else {
        format!("{}.{:06}", whole, frac)
            .trim_end_matches('0')
            .to_string()
    }
}

/// Get currency chain RPC URL from environment
fn get_currency_rpc_url() -> String {
    std::env::var("DCHAT_CURRENCY_RPC_URL")
        .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string())
}

/// Initialize currency chain client
fn init_currency_chain() -> Result<CurrencyChainClient> {
    let rpc_url = get_currency_rpc_url();
    let chain_config = CurrencyChainConfig {
        rpc_url,
        ..Default::default()
    };
    CurrencyChainClient::new(chain_config)
        .map_err(|e| Error::chain(format!("Failed to connect to currency chain: {}", e)))
}

/// Handle `dchat rewards claim` command
pub async fn handle_claim(user_id: String) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    println!("\n🎁 Claiming Rewards:");
    println!("══════════════════════════════════════════════════════════");
    println!("User ID: {}", user_id);

    match init_currency_chain() {
        Ok(currency_chain) => {
            // First check pending rewards
            match currency_chain.get_wallet(&uid) {
                Ok(Some(wallet)) => {
                    if wallet.rewards_pending == 0 {
                        println!();
                        println!("Pending Rewards: 0 DCHAT");
                        println!();
                        println!("No rewards to claim at this time.");
                        println!("💡 Earn rewards by staking tokens or running a relay node!");
                    } else {
                        println!();
                        println!(
                            "Pending Rewards: {} DCHAT",
                            format_tokens(wallet.rewards_pending)
                        );

                        match currency_chain.claim_rewards(&uid) {
                            Ok(tx_hash) => {
                                println!();
                                println!("✅ Rewards claimed successfully!");
                                println!("   Transaction: {}", tx_hash);
                                println!(
                                    "   Amount: {} DCHAT",
                                    format_tokens(wallet.rewards_pending)
                                );
                            }
                            Err(e) => {
                                println!();
                                println!("❌ Failed to claim rewards: {}", e);
                            }
                        }
                    }
                }
                Ok(None) => {
                    println!();
                    println!("Pending Rewards: 0 DCHAT");
                    println!();
                    println!("No rewards to claim at this time.");
                    println!("💡 Earn rewards by staking tokens or running a relay node!");
                }
                Err(e) => {
                    println!();
                    println!("⚠️  Unable to query rewards: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to connect to currency chain: {}", e);
            println!();
            println!("⚠️  Cannot connect to currency chain: {}", e);
        }
    }

    Ok(())
}

/// Handle `dchat rewards history` command
pub async fn handle_history(user_id: String, limit: u32) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    println!("\n📜 Reward History (last {}):", limit);
    println!("══════════════════════════════════════════════════════════");
    println!("{:<24} {:<20} {:>15}", "Date", "Type", "Amount");
    println!("{}", "-".repeat(65));

    match init_currency_chain() {
        Ok(currency_chain) => {
            match currency_chain
                .get_reward_history(&uid, limit as usize)
                .await
            {
                Ok(history) if history.is_empty() => {
                    println!("No reward history found.");
                    println!();
                    println!("Rewards are distributed:");
                    println!("  • Staking rewards: Every epoch (~24 hours)");
                    println!("  • Relay rewards: Per message delivered");
                    println!("  • Validator rewards: Per block produced");
                }
                Ok(history) => {
                    for entry in history {
                        let date = chrono::DateTime::from_timestamp(entry.timestamp, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        println!(
                            "{:<24} {:<20} {:>12} DCHAT",
                            date,
                            entry.reward_type,
                            format_tokens(entry.amount)
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to query reward history: {}", e);
                    println!("⚠️  Unable to query history: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to connect to currency chain: {}", e);
            println!("⚠️  Cannot connect to currency chain: {}", e);
        }
    }

    Ok(())
}

/// Handle `dchat rewards pending` command
pub async fn handle_pending(user_id: String) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    println!("\n⏳ Pending Rewards:");
    println!("══════════════════════════════════════════════════════════");
    println!("User ID: {}", user_id);
    println!();

    match init_currency_chain() {
        Ok(currency_chain) => {
            match currency_chain.get_pending_rewards_breakdown(&uid).await {
                Ok(breakdown) => {
                    println!(
                        "Staking Rewards:    {} DCHAT",
                        format_tokens(breakdown.staking_rewards)
                    );
                    println!(
                        "Relay Rewards:      {} DCHAT",
                        format_tokens(breakdown.relay_rewards)
                    );
                    println!(
                        "Referral Rewards:   {} DCHAT",
                        format_tokens(breakdown.referral_rewards)
                    );
                    println!("──────────────────────────");
                    let total = breakdown.staking_rewards
                        + breakdown.relay_rewards
                        + breakdown.referral_rewards;
                    println!("Total Pending:      {} DCHAT", format_tokens(total));
                    println!();
                    if total > 0 {
                        println!("Claim with: dchat rewards claim --user-id {}", user_id);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to query pending rewards: {}", e);
                    // Fallback to wallet query
                    match currency_chain.get_wallet(&uid) {
                        Ok(Some(wallet)) => {
                            println!(
                                "Staking Rewards:    {} DCHAT",
                                format_tokens(wallet.rewards_pending)
                            );
                            println!("Relay Rewards:      0 DCHAT");
                            println!("Referral Rewards:   0 DCHAT");
                            println!("──────────────────────────");
                            println!(
                                "Total Pending:      {} DCHAT",
                                format_tokens(wallet.rewards_pending)
                            );
                            println!();
                            if wallet.rewards_pending > 0 {
                                println!("Claim with: dchat rewards claim --user-id {}", user_id);
                            }
                        }
                        _ => {
                            println!("⚠️  Unable to query rewards: {}", e);
                        }
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to connect to currency chain: {}", e);
            println!("⚠️  Cannot connect to currency chain: {}", e);
        }
    }

    Ok(())
}

/// Handle `dchat rewards breakdown` command
pub async fn handle_breakdown(user_id: String) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    println!("\n📊 Reward Breakdown:");
    println!("══════════════════════════════════════════════════════════");
    println!("User ID: {}", user_id);
    println!();

    match init_currency_chain() {
        Ok(currency_chain) => match currency_chain.get_all_time_rewards(&uid).await {
            Ok(breakdown) => {
                let total = breakdown.staking
                    + breakdown.relaying
                    + breakdown.referrals
                    + breakdown.governance;
                let staking_pct = if total > 0 {
                    breakdown.staking as f64 / total as f64 * 100.0
                } else {
                    0.0
                };
                let relaying_pct = if total > 0 {
                    breakdown.relaying as f64 / total as f64 * 100.0
                } else {
                    0.0
                };
                let referral_pct = if total > 0 {
                    breakdown.referrals as f64 / total as f64 * 100.0
                } else {
                    0.0
                };
                let governance_pct = if total > 0 {
                    breakdown.governance as f64 / total as f64 * 100.0
                } else {
                    0.0
                };

                println!("All-Time Earnings:");
                println!(
                    "  Staking:     {} DCHAT ({:.1}%)",
                    format_tokens(breakdown.staking),
                    staking_pct
                );
                println!(
                    "  Relaying:    {} DCHAT ({:.1}%)",
                    format_tokens(breakdown.relaying),
                    relaying_pct
                );
                println!(
                    "  Referrals:   {} DCHAT ({:.1}%)",
                    format_tokens(breakdown.referrals),
                    referral_pct
                );
                println!(
                    "  Governance:  {} DCHAT ({:.1}%)",
                    format_tokens(breakdown.governance),
                    governance_pct
                );
                println!("──────────────────────────");
                println!("  Total:       {} DCHAT", format_tokens(total));
                println!();
                println!("Current APY Estimate:");
                println!("  Staking APY:    {:.1}%", breakdown.current_staking_apy);
                println!(
                    "  Combined APY:   ~{:.1}% (with active relaying)",
                    breakdown.combined_apy_estimate
                );
            }
            Err(e) => {
                tracing::warn!("Failed to query reward breakdown: {}", e);
                println!("All-Time Earnings:");
                println!("  Staking:     0 DCHAT (0%)");
                println!("  Relaying:    0 DCHAT (0%)");
                println!("  Referrals:   0 DCHAT (0%)");
                println!("  Governance:  0 DCHAT (0%)");
                println!("──────────────────────────");
                println!("  Total:       0 DCHAT");
                println!();
                println!("⚠️  Could not retrieve full breakdown: {}", e);
            }
        },
        Err(e) => {
            tracing::warn!("Failed to connect to currency chain: {}", e);
            println!("⚠️  Cannot connect to currency chain: {}", e);
        }
    }

    Ok(())
}

/// Handle `dchat rewards compound` command
pub async fn handle_compound(user_id: String, enable: bool) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    println!("\n🔄 Auto-Compound Settings:");
    println!("══════════════════════════════════════════════════════════");
    println!("User ID: {}", user_id);
    println!(
        "Auto-Compound: {}",
        if enable { "ENABLED" } else { "DISABLED" }
    );

    match init_currency_chain() {
        Ok(currency_chain) => match currency_chain.set_auto_compound(&uid, enable).await {
            Ok(_) => {
                if enable {
                    println!();
                    println!("✅ Auto-compounding enabled!");
                    println!("   Rewards will be automatically restaked for maximum returns.");
                } else {
                    println!();
                    println!("✅ Auto-compounding disabled!");
                    println!("   Rewards will accumulate as claimable balance.");
                }
            }
            Err(e) => {
                println!();
                println!("❌ Failed to update auto-compound setting: {}", e);
            }
        },
        Err(e) => {
            tracing::warn!("Failed to connect to currency chain: {}", e);
            println!("⚠️  Cannot connect to currency chain: {}", e);
        }
    }

    Ok(())
}
