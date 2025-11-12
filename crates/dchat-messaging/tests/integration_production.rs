//! Integration tests for production dependency injection
//! 
//! Verifies that production builds use real implementations, not mocks.

#[cfg(test)]
mod tests {
    use dchat_messaging::{ChainStakingVerifier, ChannelAccessManager};
    use std::sync::Arc;

    #[test]
    #[cfg(not(feature = "test-mocks"))]
    fn test_production_staking_verifier_wired() {
        // This test ensures ChainStakingVerifier is properly wired
        // It should compile in release builds
        
        std::env::set_var("CURRENCY_CHAIN_RPC", "http://localhost:8545");
        
        let staking_verifier = ChainStakingVerifier::from_env();
        assert!(staking_verifier.is_ok(), "ChainStakingVerifier should be constructible");
        
        let verifier = Arc::new(staking_verifier.unwrap());
        let channel_manager = ChannelAccessManager::with_staking_verifier(verifier);
        
        // Verify manager was created successfully
        assert_eq!(channel_manager.get_members(&dchat_core::types::ChannelId::new()).len(), 0);
    }
    
    #[test]
    #[cfg(feature = "test-mocks")]
    fn test_mock_only_available_in_test_mode() {
        // This test verifies MockStakingVerifier is only available with test-mocks feature
        use dchat_messaging::MockStakingVerifier;
        
        let _mock = MockStakingVerifier::new();
        // If this compiles, we're in test mode - good!
    }
    
    #[test]
    fn test_production_config_values() {
        use dchat_messaging::RateLimitConfig;
        
        let config = RateLimitConfig::production();
        
        // Verify production defaults are conservative
        assert_eq!(config.global_limit, 10000);
        assert_eq!(config.per_user_limit, 100);
        assert_eq!(config.max_queue_size, 100000);
        
        // Should be secure defaults
        assert!(config.per_user_limit < 1000, "Per-user limit should prevent spam");
        assert!(config.max_queue_size < 1000000, "Queue should have bounded memory");
    }
}
