use crate::CACHEDIR;
use anyhow::{anyhow, Context, Result};
use log::{debug, info};
use reqwest::Client;
use serde::Deserialize;
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::Command,
};

use super::nixos::nixospkgs;

#[derive(Debug, Deserialize)]
struct ProfilePkgsRoot {
    elements: HashMap<String, ProfilePkgOut>,
}

#[derive(Debug, Deserialize)]
struct ProfilePkgOut {
    #[serde(rename = "attrPath")]
    attrpath: Option<String>,
    #[serde(rename = "originalUrl")]
    originalurl: Option<String>,
    #[serde(rename = "storePaths")]
    storepaths: Vec<String>,
}

/// Struct containing information about a package installed with `nix profile`.
#[derive(Debug)]
pub struct ProfilePkg {
    pub name: String,
    pub originalurl: String,
}

/// Returns a list of all packages installed with `nix profile` with their name.
/// Does not include individual version.
pub fn getprofilepkgs() -> Result<HashMap<String, ProfilePkg>> {
    if !Path::new(&format!(
        "{}/.nix-profile/manifest.json",
        std::env::var("HOME")?
    ))
    .exists()
    {
        return Ok(HashMap::new());
    }
    let file = File::open(&format!(
        "{}/.nix-profile/manifest.json",
        std::env::var("HOME")?
    ))?;
    let profileroot: ProfilePkgsRoot = serde_json::from_reader(file)?;

    let mut out = HashMap::new();
    for pkg in profileroot.elements.values() {
        if let (Some(attrpath), Some(originalurl)) = (pkg.attrpath.clone(), pkg.originalurl.clone())
        {
            let attr = if attrpath.starts_with("legacyPackages") {
                attrpath
                    .split('.')
                    .collect::<Vec<_>>()
                    .get(2..)
                    .context("Failed to get legacyPackage attribute")?
                    .join(".")
            } else {
                format!("{}#{}", originalurl, attrpath)
            };
            if let Some(first) = pkg.storepaths.get(0) {
                let ver = first
                    .get(44..)
                    .context("Failed to get pkg name from store path")?;
                out.insert(
                    attr,
                    ProfilePkg {
                        name: ver.to_string(),
                        originalurl,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// Returns a list of all packages installed with `nix profile` with their name and version.
/// Takes a bit longer than [getprofilepkgs()].
pub async fn getprofilepkgs_versioned() -> Result<HashMap<String, String>> {
    if !Path::new(&format!(
        "{}/.nix-profile/manifest.json",
        std::env::var("HOME")?
    ))
    .exists()
    {
        return Ok(HashMap::new());
    }
    let profilepkgs = getprofilepkgs()?;

    // println!("{profilepkgs:?}");

    let latestpkgs = if Path::new(&format!("{}/nixpkgs.db", &*CACHEDIR)).exists() {
        format!("{}/nixpkgs.db", &*CACHEDIR)
    } else {
        // Change to something else if overridden
        nixpkgslatest().await?
    };
    let mut out = HashMap::new();
    let pool = SqlitePool::connect(&format!("sqlite://{}", latestpkgs)).await?;
    for (pkg, _v) in profilepkgs {
        let versions: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT version FROM pkgs WHERE attribute = $1
            "#,
        )
        .bind(&pkg)
        .fetch_all(&pool)
        .await?;
        if !versions.is_empty() {
            out.insert(pkg, versions.get(0).unwrap().0.to_string());
        }
    }
    Ok(out)
}

async fn get_full_rev(version: &str) -> Result<String> {
    let short = version.split('.').last().unwrap();

    let url = format!(
        "https://api.github.com/repos/xinux-org/nixpkgs/commits/{}",
        short
    );

    let client = Client::new();

    let resp = client
        .get(url)
        .header("User-Agent", "rust-reqwest")
        .send()
        .await?;

    let json: serde_json::Value = resp.json().await?;
    let full = json["sha"].as_str().unwrap().to_string();
    Ok(full)
}

async fn get_full_ver() -> Result<String> {
    let short_version = std::process::Command::new("sh")
        .arg("-c")
        .arg(r"nixos-version | grep -oP '^\d+\.\d+'")
        .output()
        .expect("failed to get nixos-version");
    let url = format!(
        "https://raw.githubusercontent.com/xinux-org/database/refs/heads/main/nixos-{:?}/nixpkgs.ver",
        String::from_utf8(short_version.stdout)
    );

    // Fallback url
    let url_unstable = "https://raw.githubusercontent.com/xinux-org/database/refs/heads/main/nixpkgs-unstable/nixpkgs.ver";

    let client = Client::new();

    let primary = client
        .get(url)
        .header("User-Agent", "rust-reqwest")
        .send()
        .await;

    match primary {
        Ok(resp) if resp.status().is_success() => {
            return Ok(resp.text().await?);
        }
        _ => {
            eprintln!("Primary version fetch failed, trying unstable...");
        }
    }

    // Fallback: nixos-unstable
    let fallback = client
        .get(url_unstable)
        .header("User-Agent", "rust-reqwest")
        .send()
        .await?;

    if !fallback.status().is_success() {
        return Err(anyhow!(
            "Failed to fetch version from both release and unstable channels"
        ));
    }

    Ok(fallback.text().await?)
}

/// Downloads a list of available package versions `packages.db`
/// and returns the path to the file.
pub async fn nixpkgslatest() -> Result<String> {
    // If cache directory doesn't exist, create it
    if !std::path::Path::new(&*CACHEDIR).exists() {
        std::fs::create_dir_all(&*CACHEDIR)?;
    }

    let mut nixpkgsver = None;
    let mut pinned = false;

    let mut latestnixpkgsver = String::new();

    let ver = std::process::Command::new("sh")
        .arg("-c")
        .arg(r"nixos-version | grep -oP '^\d+\.\d+'")
        .output()
        .expect("failed to get nixos-version");
    let ver_string = String::from_utf8(ver.stdout)?;
    nixpkgsver = Some(ver_string.trim());
    latestnixpkgsver = get_full_rev(&get_full_ver().await?).await?;

    pinned = false;

    if !pinned {
        println!("&nixpkgsver url: {:?}", &nixpkgsver);
        let verurl = if let Some(v) = &nixpkgsver {
            format!(
                "https://raw.githubusercontent.com/xinux-org/database/refs/heads/main/nixos-{}/nixpkgs.ver",
                v
            )
        } else {
            String::from("https://raw.githubusercontent.com/xinux-org/database/main/nixpkgs-unstable/nixpkgs.ver")
        };
        debug!("Checking nixpkgs version");
        let resp = reqwest::get(&verurl).await;

        let resp = if let Ok(r) = resp {
            r
        } else {
            // Internet connection failed
            // Check if we can use the old database
            let dbpath = format!("{}/nixpkgs.db", &*CACHEDIR);
            if Path::new(&dbpath).exists() {
                info!("Using old database");
                return Ok(dbpath);
            } else {
                return Err(anyhow!("Could not find latest nixpkgs version"));
            }
        };
        println!(
            "responce STATUS: {:?}",
            get_full_rev(&get_full_ver().await?).await?
        );

        latestnixpkgsver = get_full_rev(&get_full_ver().await?).await?;
        debug!("Latest nixpkgs version: {}", latestnixpkgsver);
    }

    // Check if latest version is already downloaded
    if let Ok(prevver) = fs::read_to_string(&format!("{}/nixpkgs.ver", &*CACHEDIR)) {
        if prevver == latestnixpkgsver && Path::new(&format!("{}/nixpkgs.db", &*CACHEDIR)).exists()
        {
            debug!("No new version of nixpkgs found");
            return Ok(format!("{}/nixpkgs.db", &*CACHEDIR));
        }
    }
    pinned = true;

    let url = if pinned {
        format!(
            "https://github.com/xinux-org/registry/raw/refs/heads/main/nixpkgs-unstable/{}.json.br",
            latestnixpkgsver
        )
    } else if let Some(v) = &nixpkgsver {
        format!(
            "https://raw.githubusercontent.com/xinux-org/database/main/{}/nixpkgs_versions.db.br",
            v
        )
    } else {
        String::from("https://raw.githubusercontent.com/xinux-org/database/main/nixpkgs-unstable/nixpkgs_versions.db.br")
    };

    debug!("Downloading nix-data database");
    // Disable auto-decompression to handle manual brotli decompression
    let client = reqwest::Client::builder().no_brotli().build()?;
    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        return Err(anyhow!(
            "Failed to download .json.br (status {})",
            resp.status()
        ));
    }

    if resp.status().is_success() {
        debug!("Writing nix-data database");
        {
            let bytes = resp.bytes().await?;
            if bytes.is_empty() {
                return Err(anyhow!("Downloaded .br file is empty"));
            }

            let mut decompressed = Vec::new();
            let mut br = brotli::Decompressor::new(bytes.as_ref(), 4096);
            br.read_to_end(&mut decompressed)
                .context("Failed to decompress brotli data")?;

            if decompressed.is_empty() {
                return Err(anyhow!("Brotli decompression resulted in empty data"));
            }

            // SQLite
            let dbpath = format!("{}/nixpkgs.db", &*CACHEDIR);
            let mut out = File::create(&dbpath).context("Failed to create database file")?;
            out.write_all(&decompressed)
                .context("Failed to write decompressed database to file")?;
        }
        debug!("Writing nix-data version");
        // Write version downloaded to file
        File::create(format!("{}/nixpkgs.ver", &*CACHEDIR))?
            .write_all(latestnixpkgsver.as_bytes())?;
    } else {
        return Err(anyhow!("Failed to download latest nixpkgs.db.br"));
    }
    Ok(format!("{}/nixpkgs.db", &*CACHEDIR))
}

pub async fn unavailablepkgs() -> Result<HashMap<String, String>> {
    let nixpath = Command::new("nix")
        .arg("eval")
        .arg("nixpkgs#path")
        .output()?
        .stdout;
    let nixpath = String::from_utf8(nixpath)?;
    let nixpath = nixpath.trim();

    let aliases = Command::new("nix-instantiate")
        .arg("--eval")
        .arg("-E")
        .arg(&format!("with import {} {{}}; builtins.attrNames ((self: super: lib.optionalAttrs config.allowAliases (import {}/pkgs/top-level/aliases.nix lib self super)) {{}} {{}})", nixpath, nixpath))
        .arg("--json")
        .output()?;
    let aliasstr = String::from_utf8(aliases.stdout)?;
    let aliasesout: HashSet<String> = serde_json::from_str(&aliasstr)?;

    let flakespkgs = getprofilepkgs()?;
    let mut unavailable = HashMap::new();
    for pkg in flakespkgs.keys() {
        if aliasesout.contains(pkg) && Command::new("nix-instantiate")
                .arg("--eval")
                .arg("-E")
                .arg(&format!("with import {} {{}}; builtins.tryEval ((self: super: lib.optionalAttrs config.allowAliases (import {}/pkgs/top-level/aliases.nix lib self super)) {{}} {{}}).{}", nixpath, nixpath, pkg))
                .output()?.status.success() {
            let out = Command::new("nix-instantiate")
                .arg("--eval")
                .arg("-E")
                .arg(&format!("with import {} {{}}; ((self: super: lib.optionalAttrs config.allowAliases (import {}/pkgs/top-level/aliases.nix lib self super)) {{}} {{}}).{}", nixpath, nixpath, pkg))
                .output()?;
            let err = String::from_utf8(out.stderr)?;
            let err = err.strip_prefix("error: ").unwrap_or(&err).trim();
            unavailable.insert(pkg.to_string(), err.to_string());
        }
    }

    let nixospkgs = nixospkgs().await?;
    let pool = SqlitePool::connect(&format!("sqlite://{}", nixospkgs)).await?;

    for pkg in flakespkgs.keys() {
        let (x, broken, insecure): (String, u8, u8) =
            sqlx::query_as("SELECT attribute,broken,insecure FROM meta WHERE attribute = $1")
                .bind(pkg)
                .fetch_one(&pool)
                .await?;
        if &x != pkg {
            unavailable.insert(
                pkg.to_string(),
                String::from("Package not found in newer version of nixpkgs"),
            );
        } else if broken == 1 {
            unavailable.insert(pkg.to_string(), String::from("Package is marked as broken"));
        } else if insecure == 1 {
            unavailable.insert(
                pkg.to_string(),
                String::from("Package is marked as insecure"),
            );
        }
    }
    Ok(unavailable)
}
