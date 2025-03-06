use config::Config;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = "mod-updater.ini";
    let modlist_path = "modlist.json";

    // Lecture du fichier de configuration (inutilisé pour l'instant)
    let _config = read_config(config_path)?;

    // Lecture de la liste des mods
    let modlist_content = fs::read_to_string(modlist_path)?;
    let mut mods: Vec<ModEntry> = serde_json::from_str(&modlist_content)?;

    // Vérification et mise à jour des mods
    for mod_entry in &mut mods {
        let mod_path = PathBuf::from("mods").join(&mod_entry.filename);

        // Si le mod est manquant, le télécharger
        if !mod_path.exists() {
            println!(
                "{} est manquant. Téléchargement en cours...",
                mod_entry.name
            );
            match download_mod(mod_entry).await {
                Ok(()) => println!("{} téléchargé avec succès.", mod_entry.name),
                Err(e) => eprintln!("Échec du téléchargement de {} : {}", mod_entry.name, e),
            }
            continue;
        }

        // Vérification des mises à jour
        match check_for_update(mod_entry).await {
            Ok(Some(new_version)) if new_version != mod_entry.version => {
                println!(
                    "Mise à jour trouvée pour {} : {} -> {}",
                    mod_entry.name, mod_entry.version, new_version
                );
                match download_mod(mod_entry).await {
                    Ok(()) => {
                        println!("{} mis à jour avec succès.", mod_entry.name);
                        mod_entry.version = new_version; // Mise à jour dynamique de la version
                    }
                    Err(e) => eprintln!("Échec de la mise à jour de {} : {}", mod_entry.name, e),
                }
            }
            Ok(_) => println!("{} est à jour.", mod_entry.name),
            Err(e) => eprintln!(
                "Erreur lors de la vérification des mises à jour pour {} : {}",
                mod_entry.name, e
            ),
        }
    }

    // Sauvegarde de la liste des mods mise à jour
    let updated_modlist = serde_json::to_string_pretty(&mods)?;
    fs::write(modlist_path, updated_modlist)?;
    println!("Liste des mods mise à jour sauvegardée dans {modlist_path}");

    Ok(())
}

/// Lit et parse le fichier de configuration.
fn read_config(path: &str) -> Result<UpdaterConfig, Box<dyn std::error::Error>> {
    let settings = Config::builder()
        .add_source(config::File::with_name(path))
        .build()?;
    settings.try_deserialize().map_err(Into::into)
}

/// Vérifie les mises à jour pour un mod donné, Modrinth en priorité, puis `CurseForge`.
async fn check_for_update(
    mod_entry: &ModEntry,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if is_modrinth_url(&mod_entry.url) {
        match check_for_update_on_modrinth(mod_entry).await {
            Ok(version) => Ok(version),
            Err(e) if e.to_string().contains("404") => {
                println!(
                    "Mod {} non trouvé sur Modrinth, tentative sur CurseForge...",
                    mod_entry.name
                );
                check_for_update_on_curseforge(mod_entry).await
            }
            Err(e) => Err(e),
        }
    } else if is_curseforge_url(&mod_entry.url) {
        check_for_update_on_curseforge(mod_entry).await
    } else {
        Err(format!(
            "URL non reconnue pour {} : {}",
            mod_entry.name, mod_entry.url
        )
        .into())
    }
}

/// Vérifie les mises à jour sur Modrinth.
async fn check_for_update_on_modrinth(
    mod_entry: &ModEntry,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let modrinth_id = extract_modrinth_id(&mod_entry.url)?;

    let client = reqwest::Client::new();
    let url = format!("https://api.modrinth.com/v2/project/{modrinth_id}/version");
    let response = client.get(&url).send().await?;
    let status = response.status();

    if status.is_success() {
        let versions: Vec<serde_json::Value> = response.json().await?;
        let mut sorted_versions = versions;
        sorted_versions.sort_by(|a, b| {
            let date_a = a["date_published"].as_str().unwrap_or("");
            let date_b = b["date_published"].as_str().unwrap_or("");
            date_b.cmp(date_a)
        });

        if let Some(latest_version) = sorted_versions.first() {
            let latest_version_number = latest_version["version_number"]
                .as_str()
                .unwrap_or("")
                .to_string();
            return Ok(Some(latest_version_number));
        }
    } else if status == reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "Mod {} non trouvé sur Modrinth (slug: {})",
            mod_entry.name, modrinth_id
        )
        .into());
    } else {
        return Err(format!(
            "Échec de la vérification Modrinth pour {} : Statut {}",
            mod_entry.name, status
        )
        .into());
    }
    Ok(None)
}

/// Extrait le slug ou l'ID `CurseForge` depuis l'URL.
fn extract_curseforge_slug(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    if parts.len() >= 2 && parts[parts.len() - 2] == "projects" {
        Ok(parts[parts.len() - 1].to_string())
    } else {
        Err("URL CurseForge invalide".into())
    }
}

/// Mappe un slug `CurseForge` à un ID numérique via l'API de recherche.
async fn get_mod_id_from_slug(
    slug: &str,
    mod_name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.curse.tools/v1/cf/mods")
        .query(&[("slug", slug)])
        .send()
        .await?;

    if response.status().is_success() {
        let search_results: serde_json::Value = response.json().await?;
        if let Some(data) = search_results.get("data").and_then(|d| d.as_array()) {
            for result in data {
                if result["slug"].as_str().unwrap_or("").to_lowercase() == slug.to_lowercase() {
                    return Ok(result["id"].to_string());
                }
            }
        }
        Err(
            format!("Aucun mod trouvé sur CurseForge pour le slug {slug} et le nom {mod_name}")
                .into(),
        )
    } else {
        Err(format!(
            "Échec de la recherche CurseForge pour le slug {} : Statut {}",
            slug,
            response.status()
        )
        .into())
    }
}

/// Vérifie les mises à jour sur `CurseForge`.
async fn check_for_update_on_curseforge(
    mod_entry: &ModEntry,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let slug_or_id = extract_curseforge_slug(&mod_entry.url)?;
    let mod_id = if slug_or_id.parse::<u32>().is_ok() {
        slug_or_id
    } else {
        get_mod_id_from_slug(&slug_or_id, &mod_entry.name).await?
    };

    let client = reqwest::Client::new();
    let url = format!("https://api.curse.tools/v1/cf/mods/{mod_id}/files/latest");
    let response = client.get(&url).send().await?;

    if response.status().is_success() {
        let file_info: serde_json::Value = response.json().await?;
        if let Some(data) = file_info.get("data") {
            if let Some(file_name) = data.get("fileName").and_then(|v| v.as_str()) {
                if file_name.ends_with(".jar") {
                    let without_jar = &file_name[..file_name.len() - 4];
                    let parts: Vec<&str> = without_jar.split('-').collect();
                    if let Some(version) = parts.last() {
                        return Ok(Some((*version).to_string()));
                    }
                }
            }
        }
        Err(format!(
            "Impossible d'extraire la version pour {} depuis CurseForge",
            mod_entry.name
        )
        .into())
    } else {
        Err(format!(
            "Échec de la vérification CurseForge pour {} : Statut {}",
            mod_entry.name,
            response.status()
        )
        .into())
    }
}

/// Télécharge un mod depuis la source appropriée.
async fn download_mod(mod_entry: &ModEntry) -> Result<(), Box<dyn std::error::Error>> {
    if is_modrinth_url(&mod_entry.url) {
        match download_from_modrinth(mod_entry).await {
            Ok(()) => Ok(()),
            Err(e) if e.to_string().contains("404") => {
                println!(
                    "Mod {} non trouvé sur Modrinth, tentative sur CurseForge...",
                    mod_entry.name
                );
                download_from_curseforge(mod_entry).await
            }
            Err(e) => Err(e),
        }
    } else if is_curseforge_url(&mod_entry.url) {
        download_from_curseforge(mod_entry).await
    } else {
        Err(format!(
            "URL non reconnue pour {} : {}",
            mod_entry.name, mod_entry.url
        )
        .into())
    }
}

/// Télécharge un mod depuis Modrinth.
async fn download_from_modrinth(mod_entry: &ModEntry) -> Result<(), Box<dyn std::error::Error>> {
    let modrinth_id = extract_modrinth_id(&mod_entry.url)?;

    let client = reqwest::Client::new();
    let url = format!("https://api.modrinth.com/v2/project/{modrinth_id}/version");
    let response = client.get(&url).send().await?;
    let status = response.status(); // Stocker le statut avant de déplacer response

    if status.is_success() {
        let versions: Vec<serde_json::Value> = response.json().await?;
        let mut sorted_versions = versions;
        sorted_versions.sort_by(|a, b| {
            let date_a = a["date_published"].as_str().unwrap_or("");
            let date_b = b["date_published"].as_str().unwrap_or("");
            date_b.cmp(date_a)
        });

        if let Some(latest_version) = sorted_versions.first() {
            let download_url = latest_version["files"][0]["url"]
                .as_str()
                .ok_or("URL de téléchargement non trouvée")?;
            let response = reqwest::get(download_url).await?;
            if response.status().is_success() {
                let bytes = response.bytes().await?;
                let mods_dir = PathBuf::from("mods");
                if !mods_dir.exists() {
                    fs::create_dir(&mods_dir)?;
                }
                let mod_path = mods_dir.join(&mod_entry.filename);
                fs::write(&mod_path, bytes)?;
                return Ok(());
            }
        }
    } else if status == reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "Mod {} non trouvé sur Modrinth (slug: {})",
            mod_entry.name, modrinth_id
        )
        .into());
    }
    Err(format!(
        "Échec du téléchargement Modrinth pour {} : Statut {}",
        mod_entry.name, status
    )
    .into())
}

/// Télécharge un mod depuis `CurseForge`.
async fn download_from_curseforge(mod_entry: &ModEntry) -> Result<(), Box<dyn std::error::Error>> {
    let slug_or_id = extract_curseforge_slug(&mod_entry.url)?;
    let mod_id = if slug_or_id.parse::<u32>().is_ok() {
        slug_or_id
    } else {
        get_mod_id_from_slug(&slug_or_id, &mod_entry.name).await?
    };

    let client = reqwest::Client::new();
    let url = format!("https://api.curse.tools/v1/cf/mods/{mod_id}/files/latest");
    let response = client.get(&url).send().await?;
    let status = response.status(); // Stocker le statut avant de déplacer response

    if status.is_success() {
        let file_info: serde_json::Value = response.json().await?;
        let download_url = file_info["data"]["downloadUrl"]
            .as_str()
            .ok_or("URL de téléchargement non trouvée")?;
        let response = reqwest::get(download_url).await?;
        if response.status().is_success() {
            let bytes = response.bytes().await?;
            let mods_dir = PathBuf::from("mods");
            if !mods_dir.exists() {
                fs::create_dir(&mods_dir)?;
            }
            let mod_path = mods_dir.join(&mod_entry.filename);
            fs::write(&mod_path, bytes)?;
            return Ok(());
        }
    }
    Err(format!(
        "Échec du téléchargement CurseForge pour {} : Statut {}",
        mod_entry.name, status
    )
    .into())
}

/// Extrait l’ID ou le slug Modrinth depuis l’URL.
fn extract_modrinth_id(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    if parts.len() >= 2 && parts[parts.len() - 2] == "mod" {
        Ok(parts[parts.len() - 1].to_string())
    } else {
        Err("URL Modrinth invalide".into())
    }
}

/// Vérifie si une URL est une URL Modrinth.
fn is_modrinth_url(url: &str) -> bool {
    url.contains("modrinth.com")
}

/// Vérifie si une URL est une URL `CurseForge`.
fn is_curseforge_url(url: &str) -> bool {
    url.contains("curseforge.com")
}
