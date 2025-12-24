//! Project utilities for Ironclad

use std::path::{Path, PathBuf};

use crate::config::IroncladConfig;
use crate::error::{IroncladError, IroncladResult};

/// Represents an Ironclad project
pub struct Project {
    /// Project root directory
    pub root: PathBuf,
    /// Configuration
    pub config: IroncladConfig,
}

impl Project {
    /// Find and load project from current or parent directories
    pub fn find() -> IroncladResult<Self> {
        let current = std::env::current_dir()?;
        Self::find_from(&current)
    }

    /// Find project starting from a specific directory
    pub fn find_from(start: &Path) -> IroncladResult<Self> {
        let mut current = start.to_path_buf();

        loop {
            let config_path = current.join("Ironclad.toml");
            if config_path.exists() {
                let config = IroncladConfig::load(&config_path)?;
                return Ok(Self {
                    root: current,
                    config,
                });
            }

            if !current.pop() {
                return Err(IroncladError::ProjectNotFound(
                    "No Ironclad.toml found in current or parent directories".to_string(),
                ));
            }
        }
    }

    /// Get the path to the built WASM file
    pub fn wasm_path(&self, release: bool) -> PathBuf {
        let profile = if release { "release" } else { "debug" };
        let target = &self.config.build.target;
        let name = self.config.project.name.replace('-', "_");

        self.root
            .join("target")
            .join(target)
            .join(profile)
            .join(format!("{}.wasm", name))
    }

    /// Get the IDL output path
    pub fn idl_path(&self) -> PathBuf {
        let name = &self.config.project.name;
        self.root
            .join(&self.config.idl.output_dir)
            .join(format!("{}.json", name))
    }

    /// Get the Cargo.toml path
    pub fn cargo_toml_path(&self) -> PathBuf {
        self.root.join("Cargo.toml")
    }

    /// Get the src/lib.rs path
    pub fn lib_path(&self) -> PathBuf {
        self.root.join("src").join("lib.rs")
    }

    /// Check if this is a valid Ironclad project
    pub fn is_valid(&self) -> bool {
        self.cargo_toml_path().exists() && self.lib_path().exists()
    }
}
