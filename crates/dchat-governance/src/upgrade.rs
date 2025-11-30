// Protocol Upgrade Governance
//
// This module implements decentralized protocol upgrade voting,
// hard fork coordination, and backward compatibility management.

use chrono::{DateTime, Duration, Utc};
use dchat_core::{Error, Result, UserId, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Semantic version representation
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(Error::validation(
                "Invalid version format. Expected: major.minor.patch",
            ));
        }

        let major = parts[0]
            .parse()
            .map_err(|_| Error::validation("Invalid major version"))?;
        let minor = parts[1]
            .parse()
            .map_err(|_| Error::validation("Invalid minor version"))?;
        let patch = parts[2]
            .parse()
            .map_err(|_| Error::validation("Invalid patch version"))?;

        Ok(Self {
            major,
            minor,
            patch,
        })
    }

    /// Check if this is a breaking change (major version bump)
    pub fn is_breaking_change(&self, other: &Version) -> bool {
        self.major > other.major
    }

    /// Check if versions are compatible (same major version)
    pub fn is_compatible(&self, other: &Version) -> bool {
        self.major == other.major
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Type of upgrade
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeType {
    /// Minor update (backward compatible)
    SoftFork,
    /// Major update (breaking changes, requires hard fork)
    HardFork,
    /// Emergency security patch
    SecurityPatch,
    /// Feature flag toggle (no code change)
    FeatureToggle { feature: String },
}

/// Code source for upgrade artifacts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeSource {
    /// GitHub repository
    GitHub {
        /// Repository owner (e.g., "dchat-network")
        owner: String,
        /// Repository name (e.g., "dchat")
        repo: String,
        /// Git reference (tag, branch, or commit SHA)
        git_ref: String,
        /// Release tag if this is a GitHub Release
        release_tag: Option<String>,
    },
    /// IPFS content-addressed storage
    Ipfs {
        /// Content identifier (CID)
        cid: String,
    },
    /// BitTorrent magnet link
    BitTorrent {
        /// Magnet URI
        magnet_uri: String,
        /// Info hash
        info_hash: String,
    },
    /// Direct HTTP(S) URL (least preferred, requires hash verification)
    Http {
        /// Download URL
        url: String,
    },
}

/// Artifact information for upgrade binaries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeArtifact {
    /// Artifact name (e.g., "dchat-linux-x86_64")
    pub name: String,
    /// Target platform (e.g., "linux-x86_64", "windows-x86_64", "macos-arm64")
    pub platform: String,
    /// SHA-256 hash of the artifact
    pub sha256: String,
    /// Optional SHA-512 hash for additional verification
    pub sha512: Option<String>,
    /// GPG signature of the artifact (detached signature)
    pub gpg_signature: Option<String>,
    /// Code signing certificate chain (for Windows/macOS)
    pub code_signing_cert: Option<String>,
    /// File size in bytes
    pub size_bytes: u64,
    /// Primary download source
    pub source: CodeSource,
    /// Mirror sources for redundancy
    pub mirrors: Vec<CodeSource>,
}

/// GitHub release information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    /// GitHub release ID
    pub release_id: u64,
    /// Release tag (e.g., "v2.0.0")
    pub tag: String,
    /// Release title
    pub title: String,
    /// Release notes (markdown)
    pub body: String,
    /// Whether this is a prerelease
    pub prerelease: bool,
    /// Whether this is a draft
    pub draft: bool,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Published timestamp
    pub published_at: Option<DateTime<Utc>>,
    /// Associated artifacts
    pub artifacts: Vec<UpgradeArtifact>,
    /// Verified GPG key IDs that signed this release
    pub verified_signers: Vec<String>,
}

/// Upgrade proposal status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeStatus {
    /// Proposal submitted, voting in progress
    Proposed,
    /// Voting passed, awaiting activation
    Approved,
    /// Scheduled for specific block height
    Scheduled { activation_height: u64 },
    /// Upgrade is now active
    Active,
    /// Proposal rejected by vote
    Rejected,
    /// Cancelled by emergency governance action
    Cancelled,
}

/// Network upgrade proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeProposal {
    pub id: Uuid,
    pub proposer: UserId,
    pub upgrade_type: UpgradeType,
    pub current_version: Version,
    pub target_version: Version,

    /// Human-readable title
    pub title: String,
    /// Detailed description and rationale
    pub description: String,
    /// Technical specification URL (e.g., GitHub PR)
    pub spec_url: Option<String>,

    /// GitHub repository source for this upgrade
    pub github_source: Option<CodeSource>,
    /// GitHub release information (if released)
    pub github_release: Option<GitHubRelease>,
    /// Upgrade artifacts for different platforms
    pub artifacts: Vec<UpgradeArtifact>,
    /// Required GPG key IDs for artifact verification
    pub required_signers: Vec<String>,
    /// Minimum number of valid signatures required
    pub min_signatures: u32,

    /// When voting ends
    pub voting_deadline: DateTime<Utc>,
    /// When upgrade activates (if approved)
    pub activation_time: Option<DateTime<Utc>>,
    /// Block height at which to activate
    pub activation_height: Option<u64>,

    /// Current status
    pub status: UpgradeStatus,

    /// Vote tally
    pub votes_for: u64,
    pub votes_against: u64,
    pub quorum_percentage: u32,

    /// Created timestamp
    pub created_at: DateTime<Utc>,

    /// Validator signatures (for hard forks)
    pub validator_signatures: Vec<ValidatorSignature>,
}

/// Validator signature for upgrade approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator_id: UserId,
    pub stake_amount: u64,
    pub signature: Vec<u8>,
    pub signed_at: DateTime<Utc>,
}

/// Fork state tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkState {
    /// Fork identifier
    pub fork_id: String,
    /// Parent version before fork
    pub parent_version: Version,
    /// New version after fork
    pub fork_version: Version,
    /// When fork occurred
    pub fork_height: u64,
    pub fork_time: DateTime<Utc>,

    /// Nodes that followed this fork
    pub supporting_nodes: HashSet<UserId>,
    /// Cumulative stake on this fork
    pub total_stake: u64,

    /// Is this the canonical chain?
    pub is_canonical: bool,
}

/// Upgrade governance manager
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpgradeManager {
    /// Active upgrade proposals
    proposals: HashMap<Uuid, UpgradeProposal>,

    /// Current protocol version
    current_version: Version,

    /// Fork history
    fork_history: Vec<ForkState>,

    /// Total network stake
    total_stake: u64,

    /// Minimum validator approval for hard forks (percentage)
    hard_fork_threshold: u32,

    /// Minimum voting period for upgrades (days)
    min_voting_period_days: i64,
}

impl UpgradeProposal {
    /// Create a new upgrade proposal
    pub fn new(
        proposer: UserId,
        upgrade_type: UpgradeType,
        current_version: Version,
        target_version: Version,
        title: String,
        description: String,
        voting_period_days: i64,
        quorum_percentage: u32,
    ) -> Result<Self> {
        if quorum_percentage > 100 {
            return Err(Error::validation("Quorum cannot exceed 100%"));
        }

        // Validate version progression
        if target_version <= current_version {
            return Err(Error::validation(
                "Target version must be greater than current version",
            ));
        }

        // Hard forks require major version bump
        if matches!(upgrade_type, UpgradeType::HardFork)
            && !target_version.is_breaking_change(&current_version)
        {
            return Err(Error::validation("Hard forks require major version bump"));
        }

        let now = Utc::now();
        let deadline = now + Duration::days(voting_period_days);

        Ok(Self {
            id: Uuid::new_v4(),
            proposer,
            upgrade_type,
            current_version,
            target_version,
            title,
            description,
            spec_url: None,
            github_source: None,
            github_release: None,
            artifacts: Vec::new(),
            required_signers: Vec::new(),
            min_signatures: 1,
            voting_deadline: deadline,
            activation_time: None,
            activation_height: None,
            status: UpgradeStatus::Proposed,
            votes_for: 0,
            votes_against: 0,
            quorum_percentage,
            created_at: now,
            validator_signatures: Vec::new(),
        })
    }

    /// Check if voting is still open
    pub fn is_voting_open(&self) -> bool {
        matches!(self.status, UpgradeStatus::Proposed) && Utc::now() < self.voting_deadline
    }

    /// Check if proposal passes
    pub fn passes(&self, total_stake: u64) -> bool {
        let total_votes = self.votes_for + self.votes_against;
        let required_votes = (total_stake * self.quorum_percentage as u64) / 100;

        total_votes >= required_votes && self.votes_for > self.votes_against
    }

    /// Add validator signature (for hard forks)
    pub fn add_validator_signature(&mut self, signature: ValidatorSignature) -> Result<()> {
        // Check for duplicate
        if self
            .validator_signatures
            .iter()
            .any(|s| s.validator_id == signature.validator_id)
        {
            return Err(Error::validation("Validator already signed"));
        }

        self.validator_signatures.push(signature);
        Ok(())
    }

    /// Calculate validator approval percentage
    pub fn validator_approval_percentage(&self, total_stake: u64) -> u32 {
        let signed_stake: u64 = self
            .validator_signatures
            .iter()
            .map(|s| s.stake_amount)
            .sum();
        ((signed_stake * 100) / total_stake) as u32
    }

    /// Set GitHub source for this upgrade
    pub fn set_github_source(&mut self, owner: String, repo: String, git_ref: String) {
        self.github_source = Some(CodeSource::GitHub {
            owner,
            repo,
            git_ref,
            release_tag: None,
        });
    }

    /// Set GitHub release information
    pub fn set_github_release(&mut self, release: GitHubRelease) -> Result<()> {
        // Update the github_source with the release tag
        if let Some(CodeSource::GitHub { owner, repo, git_ref, .. }) = &self.github_source {
            self.github_source = Some(CodeSource::GitHub {
                owner: owner.clone(),
                repo: repo.clone(),
                git_ref: git_ref.clone(),
                release_tag: Some(release.tag.clone()),
            });
        }
        self.github_release = Some(release);
        Ok(())
    }

    /// Add an upgrade artifact
    pub fn add_artifact(&mut self, artifact: UpgradeArtifact) -> Result<()> {
        // Ensure no duplicate platforms
        if self.artifacts.iter().any(|a| a.platform == artifact.platform && a.name == artifact.name) {
            return Err(Error::validation("Artifact for this platform already exists"));
        }
        self.artifacts.push(artifact);
        Ok(())
    }

    /// Set required signers for artifact verification
    pub fn set_required_signers(&mut self, signers: Vec<String>, min_signatures: u32) -> Result<()> {
        if min_signatures == 0 {
            return Err(Error::validation("At least one signature required"));
        }
        if min_signatures as usize > signers.len() {
            return Err(Error::validation("min_signatures cannot exceed number of signers"));
        }
        self.required_signers = signers;
        self.min_signatures = min_signatures;
        Ok(())
    }

    /// Verify artifact hash
    pub fn verify_artifact_hash(&self, platform: &str, data: &[u8]) -> Result<bool> {
        use sha2::{Sha256, Digest};
        
        let artifact = self.artifacts.iter()
            .find(|a| a.platform == platform)
            .ok_or_else(|| Error::NotFound(format!("No artifact for platform: {}", platform)))?;
        
        let hash = Sha256::digest(data);
        let hex_hash = hex::encode(hash);
        
        Ok(hex_hash == artifact.sha256.to_lowercase())
    }

    /// Get artifact for current platform
    pub fn get_artifact_for_platform(&self, platform: &str) -> Option<&UpgradeArtifact> {
        self.artifacts.iter().find(|a| a.platform == platform)
    }

    /// Check if all required signatures are present
    pub fn has_sufficient_signatures(&self) -> bool {
        if let Some(release) = &self.github_release {
            let valid_signatures = release.verified_signers.iter()
                .filter(|s| self.required_signers.contains(s))
                .count();
            valid_signatures >= self.min_signatures as usize
        } else {
            false
        }
    }

    /// Get GitHub repository URL
    pub fn github_url(&self) -> Option<String> {
        if let Some(CodeSource::GitHub { owner, repo, .. }) = &self.github_source {
            Some(format!("https://github.com/{}/{}", owner, repo))
        } else {
            None
        }
    }

    /// Get GitHub release URL
    pub fn github_release_url(&self) -> Option<String> {
        if let Some(CodeSource::GitHub { owner, repo, release_tag: Some(tag), .. }) = &self.github_source {
            Some(format!("https://github.com/{}/{}/releases/tag/{}", owner, repo, tag))
        } else {
            None
        }
    }
}

impl UpgradeManager {
    /// Create a new upgrade manager
    pub fn new() -> Self {
        let current = Version::parse(PROTOCOL_VERSION).unwrap_or_else(|_| Version::new(0, 1, 0));

        Self {
            proposals: HashMap::new(),
            current_version: current,
            fork_history: Vec::new(),
            total_stake: 0,
            hard_fork_threshold: 67, // 67% validator approval for hard forks
            min_voting_period_days: 14, // Minimum 2 weeks for major upgrades
        }
    }

    /// Submit a new upgrade proposal
    pub fn submit_proposal(&mut self, mut proposal: UpgradeProposal) -> Result<Uuid> {
        // Validate voting period for hard forks
        if matches!(proposal.upgrade_type, UpgradeType::HardFork) {
            let voting_days = (proposal.voting_deadline - proposal.created_at).num_days();
            if voting_days < self.min_voting_period_days {
                return Err(Error::validation(format!(
                    "Hard forks require minimum {} day voting period",
                    self.min_voting_period_days
                )));
            }
        }

        // Ensure current version matches
        if proposal.current_version != self.current_version {
            proposal.current_version = self.current_version.clone();
        }

        let id = proposal.id;
        self.proposals.insert(id, proposal);
        Ok(id)
    }

    /// Cast vote on upgrade proposal
    pub fn cast_upgrade_vote(
        &mut self,
        proposal_id: Uuid,
        _voter: UserId,
        vote_for: bool,
        voting_power: u64,
    ) -> Result<()> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if !proposal.is_voting_open() {
            return Err(Error::validation("Voting is closed"));
        }

        if vote_for {
            proposal.votes_for += voting_power;
        } else {
            proposal.votes_against += voting_power;
        }

        Ok(())
    }

    /// Finalize upgrade proposal voting
    pub fn finalize_proposal(&mut self, proposal_id: Uuid) -> Result<bool> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if !matches!(proposal.status, UpgradeStatus::Proposed) {
            return Err(Error::validation("Proposal already finalized"));
        }

        if Utc::now() < proposal.voting_deadline {
            return Err(Error::validation("Voting period not ended"));
        }

        let passed = proposal.passes(self.total_stake);

        if passed {
            // For hard forks, check validator approval
            if matches!(proposal.upgrade_type, UpgradeType::HardFork) {
                let validator_approval = proposal.validator_approval_percentage(self.total_stake);
                if validator_approval < self.hard_fork_threshold {
                    proposal.status = UpgradeStatus::Rejected;
                    return Ok(false);
                }
            }

            proposal.status = UpgradeStatus::Approved;
            Ok(true)
        } else {
            proposal.status = UpgradeStatus::Rejected;
            Ok(false)
        }
    }

    /// Schedule approved upgrade for activation
    pub fn schedule_upgrade(
        &mut self,
        proposal_id: Uuid,
        activation_height: u64,
        activation_time: DateTime<Utc>,
    ) -> Result<()> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if !matches!(proposal.status, UpgradeStatus::Approved) {
            return Err(Error::validation("Proposal must be approved first"));
        }

        proposal.activation_height = Some(activation_height);
        proposal.activation_time = Some(activation_time);
        proposal.status = UpgradeStatus::Scheduled { activation_height };

        Ok(())
    }

    /// Activate an upgrade at the scheduled height
    pub fn activate_upgrade(&mut self, proposal_id: Uuid, current_height: u64) -> Result<()> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        match proposal.status {
            UpgradeStatus::Scheduled { activation_height } => {
                if current_height < activation_height {
                    return Err(Error::validation("Activation height not reached"));
                }
            }
            _ => return Err(Error::validation("Proposal not scheduled")),
        }

        // Record fork if this is a hard fork
        if matches!(proposal.upgrade_type, UpgradeType::HardFork) {
            let fork = ForkState {
                fork_id: format!("v{}", proposal.target_version),
                parent_version: proposal.current_version.clone(),
                fork_version: proposal.target_version.clone(),
                fork_height: proposal.activation_height.unwrap(),
                fork_time: Utc::now(),
                supporting_nodes: HashSet::new(),
                total_stake: 0,
                is_canonical: true, // Assume canonical if governance passed
            };
            self.fork_history.push(fork);
        }

        // Update current version
        self.current_version = proposal.target_version.clone();
        proposal.status = UpgradeStatus::Active;

        Ok(())
    }

    /// Emergency cancel an upgrade (requires governance vote)
    pub fn cancel_upgrade(&mut self, proposal_id: Uuid) -> Result<()> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        proposal.status = UpgradeStatus::Cancelled;
        Ok(())
    }

    /// Get current protocol version
    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    /// Get all active proposals
    pub fn get_active_proposals(&self) -> Vec<&UpgradeProposal> {
        self.proposals
            .values()
            .filter(|p| p.is_voting_open())
            .collect()
    }

    /// Get upgrade proposal by ID
    pub fn get_proposal(&self, id: &Uuid) -> Option<&UpgradeProposal> {
        self.proposals.get(id)
    }
    
    /// Add validator signature to proposal (for hard forks)
    pub fn add_validator_signature(
        &mut self, 
        proposal_id: &Uuid, 
        signature: ValidatorSignature
    ) -> Result<()> {
        let proposal = self.proposals
            .get_mut(proposal_id)
            .ok_or_else(|| Error::NotFound(format!("Proposal not found: {}", proposal_id)))?;
        
        proposal.add_validator_signature(signature)
    }

    /// Get fork history
    pub fn get_fork_history(&self) -> &[ForkState] {
        &self.fork_history
    }

    /// Check if a peer version is compatible
    pub fn is_compatible_version(&self, peer_version: &Version) -> bool {
        self.current_version.is_compatible(peer_version)
    }

    /// Update total stake
    pub fn update_total_stake(&mut self, new_total: u64) {
        self.total_stake = new_total;
    }

    /// Set hard fork threshold
    pub fn set_hard_fork_threshold(&mut self, threshold: u32) -> Result<()> {
        if threshold > 100 {
            return Err(Error::validation("Threshold cannot exceed 100%"));
        }
        self.hard_fork_threshold = threshold;
        Ok(())
    }

    /// Set GitHub source for a proposal
    pub fn set_proposal_github_source(
        &mut self,
        proposal_id: &Uuid,
        owner: String,
        repo: String,
        git_ref: String,
    ) -> Result<()> {
        let proposal = self.proposals
            .get_mut(proposal_id)
            .ok_or_else(|| Error::NotFound(format!("Proposal not found: {}", proposal_id)))?;
        
        proposal.set_github_source(owner, repo, git_ref);
        Ok(())
    }

    /// Set GitHub release for a proposal
    pub fn set_proposal_github_release(
        &mut self,
        proposal_id: &Uuid,
        release: GitHubRelease,
    ) -> Result<()> {
        let proposal = self.proposals
            .get_mut(proposal_id)
            .ok_or_else(|| Error::NotFound(format!("Proposal not found: {}", proposal_id)))?;
        
        proposal.set_github_release(release)
    }

    /// Add artifact to a proposal
    pub fn add_proposal_artifact(
        &mut self,
        proposal_id: &Uuid,
        artifact: UpgradeArtifact,
    ) -> Result<()> {
        let proposal = self.proposals
            .get_mut(proposal_id)
            .ok_or_else(|| Error::NotFound(format!("Proposal not found: {}", proposal_id)))?;
        
        proposal.add_artifact(artifact)
    }

    /// Get download sources for a proposal (GitHub, IPFS, BitTorrent, etc.)
    pub fn get_proposal_download_sources(&self, proposal_id: &Uuid, platform: &str) -> Result<Vec<CodeSource>> {
        let proposal = self.proposals
            .get(proposal_id)
            .ok_or_else(|| Error::NotFound(format!("Proposal not found: {}", proposal_id)))?;
        
        let mut sources = Vec::new();
        
        // Add GitHub source if available
        if let Some(source) = &proposal.github_source {
            sources.push(source.clone());
        }
        
        // Add artifact-specific sources
        if let Some(artifact) = proposal.get_artifact_for_platform(platform) {
            sources.push(artifact.source.clone());
            sources.extend(artifact.mirrors.clone());
        }
        
        Ok(sources)
    }

    /// Verify an upgrade artifact
    pub fn verify_proposal_artifact(
        &self,
        proposal_id: &Uuid,
        platform: &str,
        data: &[u8],
    ) -> Result<bool> {
        let proposal = self.proposals
            .get(proposal_id)
            .ok_or_else(|| Error::NotFound(format!("Proposal not found: {}", proposal_id)))?;
        
        proposal.verify_artifact_hash(platform, data)
    }

    /// Check if proposal has verified GitHub release with sufficient signatures
    pub fn proposal_has_verified_release(&self, proposal_id: &Uuid) -> Result<bool> {
        let proposal = self.proposals
            .get(proposal_id)
            .ok_or_else(|| Error::NotFound(format!("Proposal not found: {}", proposal_id)))?;
        
        Ok(proposal.has_sufficient_signatures())
    }
}

impl CodeSource {
    /// Create a new GitHub source
    pub fn github(owner: impl Into<String>, repo: impl Into<String>, git_ref: impl Into<String>) -> Self {
        CodeSource::GitHub {
            owner: owner.into(),
            repo: repo.into(),
            git_ref: git_ref.into(),
            release_tag: None,
        }
    }

    /// Create a new GitHub source with release tag
    pub fn github_release(
        owner: impl Into<String>, 
        repo: impl Into<String>, 
        tag: impl Into<String>
    ) -> Self {
        let tag_str = tag.into();
        CodeSource::GitHub {
            owner: owner.into(),
            repo: repo.into(),
            git_ref: tag_str.clone(),
            release_tag: Some(tag_str),
        }
    }

    /// Create a new IPFS source
    pub fn ipfs(cid: impl Into<String>) -> Self {
        CodeSource::Ipfs { cid: cid.into() }
    }

    /// Create a new BitTorrent source
    pub fn bittorrent(magnet_uri: impl Into<String>, info_hash: impl Into<String>) -> Self {
        CodeSource::BitTorrent {
            magnet_uri: magnet_uri.into(),
            info_hash: info_hash.into(),
        }
    }

    /// Create a new HTTP source
    pub fn http(url: impl Into<String>) -> Self {
        CodeSource::Http { url: url.into() }
    }

    /// Get the download URL for this source
    pub fn download_url(&self) -> Option<String> {
        match self {
            CodeSource::GitHub { owner, repo, git_ref, release_tag } => {
                if let Some(tag) = release_tag {
                    // GitHub release download URL
                    Some(format!(
                        "https://github.com/{}/{}/releases/download/{}/",
                        owner, repo, tag
                    ))
                } else {
                    // GitHub archive URL
                    Some(format!(
                        "https://github.com/{}/{}/archive/{}.zip",
                        owner, repo, git_ref
                    ))
                }
            }
            CodeSource::Ipfs { cid } => {
                Some(format!("https://ipfs.io/ipfs/{}", cid))
            }
            CodeSource::BitTorrent { magnet_uri, .. } => {
                Some(magnet_uri.clone())
            }
            CodeSource::Http { url } => {
                Some(url.clone())
            }
        }
    }

    /// Check if this is a content-addressed source (IPFS, BitTorrent)
    pub fn is_content_addressed(&self) -> bool {
        matches!(self, CodeSource::Ipfs { .. } | CodeSource::BitTorrent { .. })
    }

    /// Check if this is a decentralized source
    pub fn is_decentralized(&self) -> bool {
        !matches!(self, CodeSource::Http { .. })
    }
}

impl UpgradeArtifact {
    /// Create a new upgrade artifact
    pub fn new(
        name: impl Into<String>,
        platform: impl Into<String>,
        sha256: impl Into<String>,
        size_bytes: u64,
        source: CodeSource,
    ) -> Self {
        Self {
            name: name.into(),
            platform: platform.into(),
            sha256: sha256.into(),
            sha512: None,
            gpg_signature: None,
            code_signing_cert: None,
            size_bytes,
            source,
            mirrors: Vec::new(),
        }
    }

    /// Add a mirror source
    pub fn add_mirror(&mut self, mirror: CodeSource) {
        self.mirrors.push(mirror);
    }

    /// Set GPG signature
    pub fn set_gpg_signature(&mut self, signature: impl Into<String>) {
        self.gpg_signature = Some(signature.into());
    }

    /// Set SHA-512 hash for additional verification
    pub fn set_sha512(&mut self, hash: impl Into<String>) {
        self.sha512 = Some(hash.into());
    }

    /// Get all download sources (primary + mirrors)
    pub fn all_sources(&self) -> Vec<&CodeSource> {
        let mut sources = vec![&self.source];
        sources.extend(self.mirrors.iter());
        sources
    }
}

impl Default for UpgradeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn test_version_compatibility() {
        let v1 = Version::new(1, 2, 3);
        let v2 = Version::new(1, 3, 0);
        let v3 = Version::new(2, 0, 0);

        assert!(v1.is_compatible(&v2)); // Same major
        assert!(!v1.is_compatible(&v3)); // Different major
        assert!(v3.is_breaking_change(&v1)); // Major version bump
    }

    #[test]
    fn test_create_soft_fork_proposal() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(1, 1, 0);

        let proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Add new feature".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        assert_eq!(proposal.status, UpgradeStatus::Proposed);
        assert!(proposal.is_voting_open());
    }

    #[test]
    fn test_hard_fork_requires_major_version() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(1, 1, 0); // Minor bump, not major

        let result = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current,
            target,
            "Breaking change".to_string(),
            "Description".to_string(),
            14,
            67,
        );

        assert!(result.is_err()); // Should fail
    }

    #[test]
    fn test_hard_fork_with_major_version() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(2, 0, 0); // Major bump

        let result = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current,
            target,
            "Breaking change".to_string(),
            "Description".to_string(),
            14,
            67,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_upgrade_manager_submit_proposal() {
        let mut manager = UpgradeManager::new();
        let proposer = UserId::new();

        let current = manager.current_version().clone();
        let target = Version::new(current.major, current.minor + 1, 0);

        let proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Test upgrade".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        let id = manager.submit_proposal(proposal).unwrap();
        assert!(manager.get_proposal(&id).is_some());
    }

    #[test]
    fn test_vote_and_finalize() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);

        let proposer = UserId::new();

        let current = manager.current_version().clone();
        let target = Version::new(current.major, current.minor + 1, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Test upgrade".to_string(),
            "Description".to_string(),
            1, // 1 day voting period for soft fork (allowed)
            60,
        )
        .unwrap();

        // Set created_at to allow past deadline setting
        proposal.created_at = Utc::now() - Duration::days(2);
        // Set deadline to past (but valid relative to created_at)
        proposal.voting_deadline = Utc::now() - Duration::seconds(1);
        // Pre-populate votes to avoid "Voting is closed" error
        proposal.votes_for = 7000;
        proposal.votes_against = 2000;

        let proposal_id = manager.submit_proposal(proposal).unwrap();

        // Finalize
        let passed = manager.finalize_proposal(proposal_id).unwrap();
        assert!(passed); // 7000 > 2000 and meets 60% quorum
    }

    #[test]
    fn test_validator_signatures() {
        let proposer = UserId::new();
        let validator1 = UserId::new();
        let validator2 = UserId::new();

        let current = Version::new(1, 0, 0);
        let target = Version::new(2, 0, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current,
            target,
            "Hard fork".to_string(),
            "Description".to_string(),
            14,
            67,
        )
        .unwrap();

        let sig1 = ValidatorSignature {
            validator_id: validator1,
            stake_amount: 4000,
            signature: vec![1, 2, 3],
            signed_at: Utc::now(),
        };

        let sig2 = ValidatorSignature {
            validator_id: validator2,
            stake_amount: 3000,
            signature: vec![4, 5, 6],
            signed_at: Utc::now(),
        };

        proposal.add_validator_signature(sig1).unwrap();
        proposal.add_validator_signature(sig2).unwrap();

        assert_eq!(proposal.validator_approval_percentage(10000), 70); // 7000/10000
    }

    #[test]
    fn test_hard_fork_threshold_check() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);
        manager.set_hard_fork_threshold(67).unwrap();

        let proposer = UserId::new();
        let current = manager.current_version().clone();
        let target = Version::new(current.major + 1, 0, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current,
            target,
            "Hard fork".to_string(),
            "Description".to_string(),
            14, // 14 days required for hard forks
            60,
        )
        .unwrap();

        // Add validator signatures (only 60% approval)
        let validator = UserId::new();
        let sig = ValidatorSignature {
            validator_id: validator,
            stake_amount: 6000,
            signature: vec![1, 2, 3],
            signed_at: Utc::now(),
        };
        proposal.add_validator_signature(sig).unwrap();

        // Set created_at to past to allow past deadline
        proposal.created_at = Utc::now() - Duration::days(15);
        proposal.voting_deadline = Utc::now() - Duration::seconds(1);
        proposal.votes_for = 7000;
        proposal.votes_against = 2000;

        let proposal_id = manager.submit_proposal(proposal).unwrap();

        // Should fail due to insufficient validator approval (60% < 67%)
        let passed = manager.finalize_proposal(proposal_id).unwrap();
        assert!(!passed);
    }

    #[test]
    fn test_schedule_and_activate() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);

        let proposer = UserId::new();
        let current = manager.current_version().clone();
        let target = Version::new(current.major, current.minor + 1, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current.clone(),
            target.clone(),
            "Test upgrade".to_string(),
            "Description".to_string(),
            0,
            60,
        )
        .unwrap();

        proposal.voting_deadline = Utc::now() - Duration::seconds(1);
        proposal.votes_for = 7000;
        proposal.votes_against = 2000;

        let proposal_id = manager.submit_proposal(proposal).unwrap();
        manager.finalize_proposal(proposal_id).unwrap();

        // Schedule
        manager
            .schedule_upgrade(proposal_id, 1000, Utc::now() + Duration::hours(1))
            .unwrap();

        // Activate
        manager.activate_upgrade(proposal_id, 1000).unwrap();

        assert_eq!(manager.current_version(), &target);
    }

    #[test]
    fn test_fork_history_tracking() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);

        let proposer = UserId::new();
        let current = manager.current_version().clone();
        let target = Version::new(current.major + 1, 0, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current.clone(),
            target.clone(),
            "Hard fork".to_string(),
            "Description".to_string(),
            14, // 14 days required for hard forks
            60,
        )
        .unwrap();

        // Add sufficient validator signatures
        for i in 0..7 {
            let validator = UserId::new();
            let sig = ValidatorSignature {
                validator_id: validator,
                stake_amount: 1000,
                signature: vec![i],
                signed_at: Utc::now(),
            };
            proposal.add_validator_signature(sig).unwrap();
        }

        // Set created_at to past to allow past deadline
        proposal.created_at = Utc::now() - Duration::days(15);
        proposal.voting_deadline = Utc::now() - Duration::seconds(1);
        proposal.votes_for = 7000;
        proposal.votes_against = 2000;

        let proposal_id = manager.submit_proposal(proposal).unwrap();
        manager.finalize_proposal(proposal_id).unwrap();
        manager
            .schedule_upgrade(proposal_id, 1000, Utc::now())
            .unwrap();
        manager.activate_upgrade(proposal_id, 1000).unwrap();

        // Check fork history
        let forks = manager.get_fork_history();
        assert_eq!(forks.len(), 1);
        assert_eq!(forks[0].fork_version, target);
        assert_eq!(forks[0].parent_version, current);
        assert!(forks[0].is_canonical);
    }

    #[test]
    fn test_github_source_creation() {
        let source = CodeSource::github("dchat-network", "dchat", "v2.0.0");
        
        if let CodeSource::GitHub { owner, repo, git_ref, release_tag } = &source {
            assert_eq!(owner, "dchat-network");
            assert_eq!(repo, "dchat");
            assert_eq!(git_ref, "v2.0.0");
            assert!(release_tag.is_none());
        } else {
            panic!("Expected GitHub source");
        }
        
        assert!(source.is_decentralized());
        assert!(!source.is_content_addressed());
        
        let url = source.download_url().unwrap();
        assert!(url.contains("github.com"));
        assert!(url.contains("dchat-network/dchat"));
    }

    #[test]
    fn test_github_release_source() {
        let source = CodeSource::github_release("dchat-network", "dchat", "v2.0.0");
        
        if let CodeSource::GitHub { release_tag, .. } = &source {
            assert_eq!(release_tag.as_ref().unwrap(), "v2.0.0");
        } else {
            panic!("Expected GitHub source");
        }
        
        let url = source.download_url().unwrap();
        assert!(url.contains("releases/download/v2.0.0"));
    }

    #[test]
    fn test_ipfs_source() {
        let source = CodeSource::ipfs("QmXoypizjW3WknFiJnKLwHCnL72vedxjQkDDP1mXWo6uco");
        
        assert!(source.is_decentralized());
        assert!(source.is_content_addressed());
        
        let url = source.download_url().unwrap();
        assert!(url.contains("ipfs.io/ipfs/"));
    }

    #[test]
    fn test_bittorrent_source() {
        let source = CodeSource::bittorrent(
            "magnet:?xt=urn:btih:abc123",
            "abc123"
        );
        
        assert!(source.is_decentralized());
        assert!(source.is_content_addressed());
    }

    #[test]
    fn test_upgrade_artifact_creation() {
        let source = CodeSource::github_release("dchat-network", "dchat", "v2.0.0");
        let mut artifact = UpgradeArtifact::new(
            "dchat-linux-x86_64",
            "linux-x86_64",
            "a1b2c3d4e5f6...",
            1024 * 1024 * 50, // 50 MB
            source,
        );
        
        // Add mirrors
        artifact.add_mirror(CodeSource::ipfs("Qm123..."));
        artifact.add_mirror(CodeSource::http("https://releases.dchat.io/v2.0.0/dchat-linux-x86_64"));
        
        assert_eq!(artifact.all_sources().len(), 3);
        assert_eq!(artifact.platform, "linux-x86_64");
    }

    #[test]
    fn test_proposal_with_github_source() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(1, 1, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Add new feature".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        // Set GitHub source
        proposal.set_github_source(
            "dchat-network".to_string(),
            "dchat".to_string(),
            "v1.1.0".to_string(),
        );
        
        assert!(proposal.github_source.is_some());
        assert_eq!(proposal.github_url(), Some("https://github.com/dchat-network/dchat".to_string()));
    }

    #[test]
    fn test_proposal_with_release_and_artifacts() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(1, 1, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Add new feature".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        proposal.set_github_source(
            "dchat-network".to_string(),
            "dchat".to_string(),
            "v1.1.0".to_string(),
        );

        // Create release
        let release = GitHubRelease {
            release_id: 12345,
            tag: "v1.1.0".to_string(),
            title: "dchat v1.1.0".to_string(),
            body: "Release notes...".to_string(),
            prerelease: false,
            draft: false,
            created_at: Utc::now(),
            published_at: Some(Utc::now()),
            artifacts: Vec::new(),
            verified_signers: vec!["key123".to_string(), "key456".to_string()],
        };
        
        proposal.set_github_release(release).unwrap();
        assert!(proposal.github_release.is_some());
        
        // Add artifact
        let artifact = UpgradeArtifact::new(
            "dchat-linux-x86_64",
            "linux-x86_64",
            "abc123def456",
            50_000_000,
            CodeSource::github_release("dchat-network", "dchat", "v1.1.0"),
        );
        
        proposal.add_artifact(artifact).unwrap();
        assert_eq!(proposal.artifacts.len(), 1);
        
        // Check URL
        assert_eq!(
            proposal.github_release_url(),
            Some("https://github.com/dchat-network/dchat/releases/tag/v1.1.0".to_string())
        );
    }

    #[test]
    fn test_artifact_verification() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(1, 1, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Test upgrade".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        // SHA-256 of "hello world"
        let expected_hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        
        let artifact = UpgradeArtifact::new(
            "test-artifact",
            "linux-x86_64",
            expected_hash,
            11,
            CodeSource::http("https://example.com/test"),
        );
        
        proposal.add_artifact(artifact).unwrap();
        
        // Verify with correct data
        let result = proposal.verify_artifact_hash("linux-x86_64", b"hello world").unwrap();
        assert!(result);
        
        // Verify with incorrect data
        let result = proposal.verify_artifact_hash("linux-x86_64", b"wrong data").unwrap();
        assert!(!result);
    }

    #[test]
    fn test_required_signers() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(1, 1, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Test upgrade".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        // Set required signers (2 of 3 required)
        proposal.set_required_signers(
            vec!["key1".to_string(), "key2".to_string(), "key3".to_string()],
            2,
        ).unwrap();

        // Without release, should not have sufficient signatures
        assert!(!proposal.has_sufficient_signatures());

        // Add release with 2 valid signers
        let release = GitHubRelease {
            release_id: 123,
            tag: "v1.1.0".to_string(),
            title: "Test".to_string(),
            body: "".to_string(),
            prerelease: false,
            draft: false,
            created_at: Utc::now(),
            published_at: Some(Utc::now()),
            artifacts: Vec::new(),
            verified_signers: vec!["key1".to_string(), "key2".to_string()],
        };
        
        proposal.set_github_release(release).unwrap();
        assert!(proposal.has_sufficient_signatures());
    }

    #[test]
    fn test_manager_github_operations() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);

        let proposer = UserId::new();
        let current = manager.current_version().clone();
        let target = Version::new(current.major, current.minor + 1, 0);

        let proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Test upgrade".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        let proposal_id = manager.submit_proposal(proposal).unwrap();

        // Set GitHub source via manager
        manager.set_proposal_github_source(
            &proposal_id,
            "dchat-network".to_string(),
            "dchat".to_string(),
            "v1.1.0".to_string(),
        ).unwrap();

        // Add artifact via manager
        let artifact = UpgradeArtifact::new(
            "dchat-linux-x86_64",
            "linux-x86_64",
            "abc123",
            50_000_000,
            CodeSource::github_release("dchat-network", "dchat", "v1.1.0"),
        );
        
        manager.add_proposal_artifact(&proposal_id, artifact).unwrap();

        // Get download sources
        let sources = manager.get_proposal_download_sources(&proposal_id, "linux-x86_64").unwrap();
        assert!(!sources.is_empty());
    }
}
