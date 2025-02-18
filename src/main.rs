use reqwest;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tokio;
use config::Config;
use std::time::Duration;

#[derive(Debug, Deserialize, Serialize)]
struct ModEntry {
    authors: Option<Vec<String>>,
    filename: String,
    name: String,
    url: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct UpdaterConfig {
    #[serde(rename = "type")]
    _service_type: Option<String>,
    _path: Option<String>,
    _args: Option<String>,
    curse_api_key: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = "mod-updater.ini";
    let modlist_path = "modlist.json";

    // Read and parse the configuration file
    let config = read_config(config_path)?;

    // Read and parse the modlist
    let modlist_content = fs::read_to_string(modlist_path)?;
    let mods: Vec<ModEntry> = serde_json::from_str(&modlist_content)?;

    // Check for updates for each mod
    for mod_entry in mods {
        let mod_path = PathBuf::from("mods").join(&mod_entry.filename);

        // If the mod is missing, download it
        if !mod_path.exists() {
            println!("{} is missing. Downloading...", mod_entry.name);
            if let Err(e) = download_mod(&mod_entry, &config.curse_api_key).await {
                eprintln!("Failed to download {}: {}", mod_entry.name, e);
            }
            continue;
        }

        // Check for updates based on the source (Modrinth or CurseForge)
        match check_for_update(&mod_entry, &config.curse_api_key).await {
            Ok(Some(new_version)) if new_version != mod_entry.version => {
                println!(
                    "Update found for {}: {} -> {}",
                    mod_entry.name, mod_entry.version, new_version
                );
                if let Err(e) = download_mod(&mod_entry, &config.curse_api_key).await {
                    eprintln!("Failed to update {}: {}", mod_entry.name, e);
                }
            }
            Ok(_) => println!("{} is up to date.", mod_entry.name),
            Err(e) => eprintln!(
                "Error checking for updates for {}: {}",
                mod_entry.name, e
            ),
        }
    }

    Ok(())
}

/// Reads and parses the configuration file.
fn read_config(path: &str) -> Result<UpdaterConfig, Box<dyn std::error::Error>> {
    let settings = Config::builder()
        .add_source(config::File::with_name(path))
        .build()?;

    settings.try_deserialize().map_err(Into::into)
}

/// Checks for updates for a given mod.
async fn check_for_update(
    mod_entry: &ModEntry,
    curse_api_key: &Option<String>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if is_modrinth_url(&mod_entry.url) {
        check_for_update_on_modrinth(&mod_entry).await
    } else if is_curseforge_url(&mod_entry.url) {
        check_for_update_on_curseforge(&mod_entry, curse_api_key).await
    } else {
        Err("Unrecognized URL (must be CurseForge or Modrinth)".into())
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

        // Sort versions by date (newest first)
        let mut sorted_versions = versions;
        sorted_versions.sort_by(|a, b| {
            let date_a = a["date_published"].as_str().unwrap_or("");
            let date_b = b["date_published"].as_str().unwrap_or("");
            date_b.cmp(&date_a)
        });

        // Get the latest version
        if let Some(latest_version) = sorted_versions.first() {
            let latest_version_number = latest_version["version_number"]
                .as_str()
                .unwrap_or("")
                .to_string();
            return Ok(Some(latest_version_number));
        }
    }

    Ok(None)
}

/// Checks for updates for a given mod on CurseForge.
async fn check_for_update_on_curseforge(
    mod_entry: &ModEntry,
    curse_api_key: &Option<String>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let curseforge_id = extract_curseforge_id(&mod_entry.url)?;

    let client = reqwest::Client::new();
    let mut request = client
        .get(&format!(
            "https://api.curse.tools/v1/cf/mods/{}/files/latest",
            curseforge_id
        ));

    if let Some(api_key) = curse_api_key {
        request = request.header("x-api-key", api_key);
    }

    let response = request.send().await?;

    if response.status().is_success() {
        let file_info: serde_json::Value = response.json().await?;

        if let Some(latest_version) = file_info["data"]["gameVersions"].as_array() {
            if let Some(latest_file) = latest_version.first() {
                let latest_version_number = latest_file.as_str().unwrap_or("").to_string();
                return Ok(Some(latest_version_number));
            }
        }
    } else {
        eprintln!(
            "Failed to fetch version info for {}: {} - {}",
            mod_entry.name,
            response.status(),
            response.text().await?
        );
    }

    Ok(None)
}

/// Downloads a mod from the appropriate source (Modrinth or CurseForge).
async fn download_mod(
    mod_entry: &ModEntry,
    curse_api_key: &Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_modrinth_url(&mod_entry.url) {
        download_from_modrinth(&mod_entry).await
    } else if is_curseforge_url(&mod_entry.url) {
        download_from_curseforge(&mod_entry, curse_api_key).await
    } else {
        Err("Unrecognized URL (must be CurseForge or Modrinth)".into())
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

        // Sort versions by date (newest first)
        let mut sorted_versions = versions;
        sorted_versions.sort_by(|a, b| {
            let date_a = a["date_published"].as_str().unwrap_or("");
            let date_b = b["date_published"].as_str().unwrap_or("");
            date_b.cmp(&date_a)
        });

        // Get the latest version's download URL
        if let Some(latest_version) = sorted_versions.first() {
            let download_url = latest_version["files"][0]["url"]
                .as_str()
                .ok_or("Download URL not found")?;

            // Download the mod file with retries
            let mut retries = 3;
            while retries > 0 {
                let response = reqwest::get(download_url).await?;
                if response.status().is_success() {
                    let bytes = response.bytes().await?;
                    let mods_dir = PathBuf::from("mods");
                    if !mods_dir.exists() {
                        fs::create_dir(&mods_dir)?;
                    }

                    let mod_path = mods_dir.join(&mod_entry.filename);
                    fs::write(&mod_path, bytes)?;

                    println!("{} downloaded successfully from Modrinth!", mod_entry.name);
                    return Ok(());
                } else {
                    retries -= 1;
                    tokio::time::sleep(Duration::from_secs(2)).await; // Wait before retrying
                }
            }

            return Err("Failed to download after retries".into());
        }
    }

    Err("Failed to fetch version information".into())
}

/// Downloads a mod from CurseForge.
async fn download_from_curseforge(
    mod_entry: &ModEntry,
    curse_api_key: &Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let curseforge_id = extract_curseforge_id(&mod_entry.url)?;

    // Fetch the latest version download URL from CurseForge API
    let client = reqwest::Client::new();
    let mut request = client
        .get(&format!(
            "https://api.curse.tools/v1/cf/mods/{}/files/latest",
            curseforge_id
        ));

    if let Some(api_key) = curse_api_key {
        request = request.header("x-api-key", api_key);
    }

    let response = request.send().await?;

    if response.status().is_success() {
        let file_info: serde_json::Value = response.json().await?;

        // Get the download URL
        let download_url = file_info["data"]["downloadUrl"]
            .as_str()
            .ok_or("Download URL not found")?;

        // Download the mod file with retries
        let mut retries = 3;
        while retries > 0 {
            let response = reqwest::get(download_url).await?;
            if response.status().is_success() {
                let bytes = response.bytes().await?;
                let mods_dir = PathBuf::from("mods");
                if !mods_dir.exists() {
                    fs::create_dir(&mods_dir)?;
                }

                let mod_path = mods_dir.join(&mod_entry.filename);
                fs::write(&mod_path, bytes)?;

                println!("{} downloaded successfully from CurseForge!", mod_entry.name);
                return Ok(());
            } else {
                retries -= 1;
                tokio::time::sleep(Duration::from_secs(2)).await; // Wait before retrying
            }
        }

        return Err("Failed to download after retries".into());
    }

    Err("Failed to fetch version information".into())
}

/// Extracts the Modrinth project ID or slug from the URL.
fn extract_modrinth_id(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    if parts.len() >= 2 && parts[parts.len() - 2] == "mod" {
        Ok(parts[parts.len() - 1].to_string())
    } else {
        Err("Invalid Modrinth URL".into())
    }
}

/// Extracts the CurseForge project ID from the URL.
fn extract_curseforge_id(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    if parts.len() >= 2 && parts[parts.len() - 2] == "projects" {
        Ok(parts[parts.len() - 1].to_string())
    } else {
        Err("Invalid CurseForge URL".into())
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