//! Build script for dchat-messaging
//!
//! SECURITY: Prevents test-mocks feature from being enabled in release builds.
//! This ensures MockStakingVerifier and other test doubles cannot be accidentally
//! or maliciously used in production deployments.

fn main() {
    // SECURITY CHECK: Prevent test-mocks feature in release builds
    //
    // The test-mocks feature enables mock implementations that bypass
    // security checks (like stake verification). If enabled in production,
    // an attacker could:
    // - Send messages without valid stake
    // - Bypass rate limiting
    // - Access restricted channels
    //
    // This compile-time check ensures the feature is never active in release builds.
    
    #[cfg(all(feature = "test-mocks", not(debug_assertions)))]
    {
        compile_error!(
            "SECURITY ERROR: The 'test-mocks' feature cannot be enabled in release builds!\n\
            \n\
            The test-mocks feature enables mock implementations that bypass security checks.\n\
            If you're seeing this error:\n\
            \n\
            1. If building for production: Remove '--features test-mocks' from your build command\n\
            2. If running tests: Use 'cargo test' which builds in debug mode\n\
            3. If you need mocks in release: This is a security violation - reconsider your approach\n\
            \n\
            For more information, see SECURITY_AUDIT_2025-12-08.md\n"
        );
    }
    
    // Also emit a warning if test-mocks is enabled even in debug builds
    #[cfg(feature = "test-mocks")]
    {
        println!("cargo:warning=test-mocks feature is enabled - mock verifiers are active");
        println!("cargo:warning=DO NOT use this build in production!");
    }
}
