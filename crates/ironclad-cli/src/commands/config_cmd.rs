//! ironclad config - Configuration management

use crate::config::IroncladConfig;
use crate::error::{IroncladError, IroncladResult};
use crate::project::Project;
use colored::*;

pub async fn run_show() -> IroncladResult<()> {
    let project = Project::find()?;

    println!("{} {}", "⚙️".bold(), "Configuration".cyan().bold());
    println!();

    let config_path = project.root.join("Ironclad.toml");
    println!("  File: {}", config_path.display().to_string().dimmed());
    println!();

    println!("  {}", "[project]".yellow());
    println!("    name = \"{}\"", project.config.project.name);
    println!("    version = \"{}\"", project.config.project.version);

    if let Some(id) = &project.config.project.program_id {
        println!("    program_id = \"{}\"", id);
    }

    if let Some(desc) = &project.config.project.description {
        println!("    description = \"{}\"", desc);
    }

    println!();
    println!("  {}", "[build]".yellow());
    println!("    target = \"{}\"", project.config.build.target);
    println!("    generate_idl = {}", project.config.build.generate_idl);
    println!("    verify_schema = {}", project.config.build.verify_schema);

    if !project.config.build.rustflags.is_empty() {
        println!("    rustflags = {:?}", project.config.build.rustflags);
    }

    println!();
    println!("  {}", "[networks]".yellow());
    for (name, config) in &project.config.networks {
        println!("    [networks.{}]", name);
        println!("      url = \"{}\"", config.url);

        if let Some(ws) = &config.ws_url {
            println!("      ws_url = \"{}\"", ws);
        }

        if let Some(kp) = &config.keypair {
            println!("      keypair = \"{}\"", kp);
        }
    }

    Ok(())
}

pub async fn run_set(key: &str, value: &str) -> IroncladResult<()> {
    let project = Project::find()?;
    let mut config = project.config.clone();

    let parts: Vec<&str> = key.split('.').collect();

    match parts.as_slice() {
        ["project", "name"] => config.project.name = value.to_string(),
        ["project", "version"] => config.project.version = value.to_string(),
        ["project", "program_id"] => config.project.program_id = Some(value.to_string()),
        ["project", "description"] => config.project.description = Some(value.to_string()),
        ["build", "target"] => config.build.target = value.to_string(),
        ["build", "generate_idl"] => {
            config.build.generate_idl = value
                .parse()
                .map_err(|_| IroncladError::ConfigError("Expected true or false".to_string()))?;
        }
        ["build", "verify_schema"] => {
            config.build.verify_schema = value
                .parse()
                .map_err(|_| IroncladError::ConfigError("Expected true or false".to_string()))?;
        }
        _ => {
            return Err(IroncladError::ConfigError(format!(
                "Unknown config key: {}. Try: project.name, project.version, build.target, etc.",
                key
            )));
        }
    }

    let config_path = project.root.join("Ironclad.toml");
    config.save(&config_path)?;

    println!("  {} Set {} = {}", "✓".green(), key.cyan(), value.green());

    Ok(())
}

pub async fn run_get(key: &str) -> IroncladResult<()> {
    let project = Project::find()?;

    let parts: Vec<&str> = key.split('.').collect();

    let value: String = match parts.as_slice() {
        ["project", "name"] => project.config.project.name.clone(),
        ["project", "version"] => project.config.project.version.clone(),
        ["project", "program_id"] => project
            .config
            .project
            .program_id
            .clone()
            .unwrap_or_default(),
        ["project", "description"] => project
            .config
            .project
            .description
            .clone()
            .unwrap_or_default(),
        ["build", "target"] => project.config.build.target.clone(),
        ["build", "generate_idl"] => project.config.build.generate_idl.to_string(),
        ["build", "verify_schema"] => project.config.build.verify_schema.to_string(),
        _ => {
            return Err(IroncladError::ConfigError(format!(
                "Unknown config key: {}",
                key
            )));
        }
    };

    println!("{}", value);

    Ok(())
}

pub async fn run_init() -> IroncladResult<()> {
    let cwd = std::env::current_dir()?;
    let config_path = cwd.join("Ironclad.toml");

    if config_path.exists() {
        return Err(IroncladError::ConfigError(
            "Ironclad.toml already exists".to_string(),
        ));
    }

    // Try to infer project name from Cargo.toml
    let project_name = if let Ok(cargo_toml) = std::fs::read_to_string(cwd.join("Cargo.toml")) {
        cargo_toml
            .lines()
            .find(|l| l.starts_with("name"))
            .and_then(|l| l.split('=').nth(1))
            .map(|s| s.trim().trim_matches('"').to_string())
            .unwrap_or_else(|| "my-program".to_string())
    } else {
        "my-program".to_string()
    };

    let config = IroncladConfig::default_with_name(&project_name);
    config.save(&config_path)?;

    println!("{} {}", "✓".green(), "Created Ironclad.toml".cyan());

    Ok(())
}
