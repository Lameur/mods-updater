use reqwest;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tokio;

#[derive(Debug, Deserialize, Serialize)]
struct ModEntry {
    authors: Option<Vec<String>>, // Make `authors` optional
    filename: String,
    name: String,
    url: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct UpdaterConfig {
    #[serde(rename = "type")]
    _service_type: Option<String>, // Prefix with an underscore
    _path: Option<String>,         // Prefix with an underscore
    _args: Option<String>,         // Prefix with an underscore
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = "mod-updater.ini";
    let modlist_path = "modlist.json";

    // Read and parse the configuration file (intentionally unused for now)
    let _config = read_config(config_path)?;

    // Read and parse the modlist
    let modlist_content = fs::read_to_string(modlist_path)?;
    let mods: Vec<ModEntry> = serde_json::from_str(&modlist_content)?;

    // Check for updates for each mod
    for mod_entry in mods {
        let mod_path = PathBuf::from("mods").join(&mod_entry.filename);

        // If the mod is missing, download it
        if !mod_path.exists() {
            println!("{} est absent. Téléchargement...", mod_entry.name);
            download_mod(&mod_entry).await?;
            continue;
        }

        // Check for updates based on the source (Modrinth only)
        match check_for_update(&mod_entry).await {
            Ok(Some(new_version)) if new_version != mod_entry.version => {
                println!(
                    "Mise à jour trouvée pour {}: {} -> {}",
                    mod_entry.name, mod_entry.version, new_version
                );
                download_mod(&mod_entry).await?;
            }
            Ok(_) => println!("{} est à jour.", mod_entry.name),
            Err(e) => eprintln!("Erreur lors de la vérification de {}: {}", mod_entry.name, e),
        }
    }

    Ok(())
}

/// Reads and parses the configuration file.
fn read_config(path: &str) -> Result<UpdaterConfig, Box<dyn std::error::Error>> {
    let settings = config::Config::builder()
        .add_source(config::File::with_name(path))
        .build()?;

    settings.try_deserialize().map_err(Into::into)
}

/// Checks for updates for a given mod.
async fn check_for_update(mod_entry: &ModEntry) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if is_modrinth_url(&mod_entry.url) {
        check_for_update_on_modrinth(&mod_entry).await
    } else if is_curseforge_url(&mod_entry.url) {
        // Skip CurseForge mods since we don't have an API key
        println!("Skipping {} (CurseForge mod, no API key)", mod_entry.name);
        Ok(None)
    } else {
        Err("URL non reconnue (doit être CurseForge ou Modrinth)".into())
    }
}

/// Checks for updates for a given mod on Modrinth.
async fn check_for_update_on_modrinth(
    mod_entry: &ModEntry,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let modrinth_id = extract_modrinth_id(&mod_entry.url)?;

    // Fetch the latest version from Modrinth API
    let client = reqwest::Client::new();
    let response = client
        .get(&format!(
            "https://api.modrinth.com/v2/project/{}/version",
            modrinth_id
        ))
        .send()
        .await?;

    if response.status().is_success() {
        let versions: Vec<serde_json::Value> = response.json().await?;

        // Get the latest version
        if let Some(latest_version) = versions.first() {
            let latest_version_number = latest_version["version_number"]
                .as_str()
                .unwrap_or("")
                .to_string();
            return Ok(Some(latest_version_number));
        }
    }

    Ok(None)
}

/// Downloads a mod from the appropriate source (Modrinth only).
async fn download_mod(mod_entry: &ModEntry) -> Result<(), Box<dyn std::error::Error>> {
    if is_modrinth_url(&mod_entry.url) {
        download_from_modrinth(&mod_entry).await
    } else if is_curseforge_url(&mod_entry.url) {
        // Skip CurseForge mods since we don't have an API key
        println!("Skipping {} (CurseForge mod, no API key)", mod_entry.name);
        Ok(())
    } else {
        Err("URL non reconnue (doit être CurseForge ou Modrinth)".into())
    }
}

/// Downloads a mod from Modrinth.
async fn download_from_modrinth(mod_entry: &ModEntry) -> Result<(), Box<dyn std::error::Error>> {
    let modrinth_id = extract_modrinth_id(&mod_entry.url)?;

    // Fetch the latest version download URL from Modrinth API
    let client = reqwest::Client::new();
    let response = client
        .get(&format!(
            "https://api.modrinth.com/v2/project/{}/version",
            modrinth_id
        ))
        .send()
        .await?;

    if response.status().is_success() {
        let versions: Vec<serde_json::Value> = response.json().await?;

        // Get the latest version's download URL
        if let Some(latest_version) = versions.first() {
            let download_url = latest_version["files"][0]["url"]
                .as_str()
                .ok_or("URL de téléchargement introuvable")?;

            // Download the mod file
            let response = reqwest::get(download_url).await?;
            if !response.status().is_success() {
                return Err(format!("Échec du téléchargement: {}", response.status()).into());
            }

            let bytes = response.bytes().await?;
            let mods_dir = PathBuf::from("mods");
            if !mods_dir.exists() {
                fs::create_dir(&mods_dir)?;
            }

            let mod_path = mods_dir.join(&mod_entry.filename);
            fs::write(&mod_path, bytes)?;

            println!("{} téléchargé avec succès depuis Modrinth!", mod_entry.name);
            return Ok(());
        }
    }

    Err("Impossible de récupérer les informations de la version".into())
}

/// Extracts the Modrinth project ID or slug from the URL.
fn extract_modrinth_id(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Example URL: https://modrinth.com/mod/ai-improvements
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    if parts.len() >= 2 && parts[parts.len() - 2] == "mod" {
        Ok(parts[parts.len() - 1].to_string())
    } else {
        Err("URL de Modrinth invalide".into())
    }
}

/// Checks if a URL is a Modrinth URL.
fn is_modrinth_url(url: &str) -> bool {
    url.contains("modrinth.com")
}

/// Checks if a URL is a CurseForge URL.
fn is_curseforge_url(url: &str) -> bool {
    url.contains("curseforge.com")
}