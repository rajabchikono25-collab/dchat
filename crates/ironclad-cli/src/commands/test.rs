//! ironclad test - Run tests

use crate::error::{IroncladError, IroncladResult};
use crate::project::Project;
use colored::*;
use std::process::Command;
use std::time::Instant;

pub async fn run(filter: Option<&str>, verbose: bool) -> IroncladResult<()> {
    let start = Instant::now();
    let project = Project::find()?;

    println!(
        "{} {} {}",
        "🧪".bold(),
        "Testing".cyan().bold(),
        project.config.project.name.green()
    );

    let mut cmd = Command::new("cargo");
    cmd.arg("test").current_dir(&project.root);

    if let Some(f) = filter {
        cmd.arg(f);
    }

    if verbose {
        cmd.arg("--").arg("--nocapture");
    }

    let status = cmd
        .status()
        .map_err(|e| IroncladError::TestFailed(format!("Failed to run cargo test: {}", e)))?;

    let elapsed = start.elapsed();

    if status.success() {
        println!();
        println!(
            "{}",
            format!("✅ Tests passed in {:.2}s", elapsed.as_secs_f64())
                .green()
                .bold()
        );
        Ok(())
    } else {
        Err(IroncladError::TestFailed("Tests failed".to_string()))
    }
}
