pub mod gossip;
pub mod package;

pub use package::{
    AutoUpdateConfig, DistributionError, DownloadSource, PackageManager, PackageMetadata,
    PackageType, Result, SourceType,
};

pub use gossip::{GossipDiscovery, UpdateScheduler, VersionAnnouncement};
