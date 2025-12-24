//! ironclad clean - Clean build artifacts

use crate::error::IroncladResult;
use crate::project::Project;
use colored::*;
use std::fs;

pub async fn run(verbose: bool) -> IroncladResult<()> {
    let project = Project::find()?;

    println!(
        "{} {}",
        "🧹".bold(),
        "Cleaning build artifacts...".cyan().bold()
    );

    let mut cleaned = 0u64;

    // Clean target directory
    let target_dir = project.root.join("target");
    if target_dir.exists() {
        let size = dir_size(&target_dir);

        if verbose {
            println!("  {} Removing target/ ({} bytes)", "→".dimmed(), size);
        }

        fs::remove_dir_all(&target_dir)?;
        cleaned += size;
    }

    // Clean generated files
    let generated_dir = project.root.join("generated");
    if generated_dir.exists() {
        let size = dir_size(&generated_dir);

        if verbose {
            println!("  {} Removing generated/ ({} bytes)", "→".dimmed(), size);
        }

        fs::remove_dir_all(&generated_dir)?;
        cleaned += size;
    }

    // Clean IDL if it exists
    let idl_path = project.idl_path();
    if idl_path.exists() {
        let size = fs::metadata(&idl_path)?.len();

        if verbose {
            println!("  {} Removing {}", "→".dimmed(), idl_path.display());
        }

        fs::remove_file(&idl_path)?;
        cleaned += size;
    }

    println!();
    println!(
        "{} {}",
        "✅".green(),
        format!("Cleaned {} bytes", cleaned).green().bold()
    );

    Ok(())
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut size = 0;

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                size += dir_size(&path);
            } else if let Ok(metadata) = fs::metadata(&path) {
                size += metadata.len();
            }
        }
    }

    size
}
