use reqwest;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};
use tokio;

#[derive(Debug, Deserialize, Serialize)]
struct ModEntry {
    name: String,
    filename: String,
    url: String,
    version: String,
}

#[tokio::main]
async fn main() {
    let config_path = "mod-updater.conf";
    let modlist_path = "modlist.json";

    let config = fs::read_to_string(config_path).expect("Erreur de lecture du fichier de config");
    let modlist_content =
        fs::read_to_string(modlist_path).expect("Erreur de lecture de la modlist");

    let mods: Vec<ModEntry> =
        serde_json::from_str(&modlist_content).expect("Erreur de parsing JSON");

    for mod_entry in mods {
        match check_for_update(&mod_entry).await {
            Some(new_version) if new_version != mod_entry.version => {
                println!(
                    "Mise à jour trouvée pour {}: {} -> {}",
                    mod_entry.name, mod_entry.version, new_version
                );
                download_and_replace_mod(&mod_entry).await;
            }
            _ => println!("{} est à jour.", mod_entry.name),
        }
    }
}

async fn check_for_update(mod_entry: &ModEntry) -> Option<String> {
    // Implémentation de la vérification des mises à jour
    // Exemple: récupérer la dernière version depuis CurseForge
    None // Placeholder, à implémenter
}

async fn download_and_replace_mod(mod_entry: &ModEntry) {
    println!("Téléchargement de {}...", mod_entry.name);
    let response = reqwest::get(&mod_entry.url)
        .await
        .expect("Échec du téléchargement");
    let bytes = response
        .bytes()
        .await
        .expect("Impossible de récupérer les données");
    fs::write(Path::new("mods").join(&mod_entry.filename), bytes)
        .expect("Erreur d'écriture du fichier");
    println!("{} mis à jour avec succès!", mod_entry.name);
}
