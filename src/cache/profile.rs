use crate::utils::get_full_ver;
use crate::CACHEDIR;
use anyhow::{Context, Result};
use log::debug;
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
    let file = File::open(format!(
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

/// Downloads a list of available package versions `packages.db`
/// and returns the path to the file.
pub async fn nixpkgslatest() -> Result<String> {
    // If cache directory doesn't exist, create it
    if !std::path::Path::new(&*CACHEDIR).exists() {
        std::fs::create_dir_all(&*CACHEDIR)?;
    }

    // we will have internet before install something
    // returns 2x.xx
    let ver = std::process::Command::new("sh")
        .arg("-c")
        .arg(r"nixos-version | grep -oP '^\d+\.\d+'")
        .output()
        .expect("failed to get nixos-version");
    let ver_string = String::from_utf8(ver.stdout)?;

    // hash of commit like: 25.11.asdasd.asd
    let latestnixpkgsver = get_full_ver().await?;

    if let Ok(prevver) = fs::read_to_string(format!("{}/nixpkgs.ver", &*CACHEDIR)) {
        if prevver == latestnixpkgsver.clone()
            && Path::new(&format!("{}/nixpkgs.db", &*CACHEDIR)).exists()
        {
            debug!("No new version of nixpkgs.db found");
            return Ok(format!("{}/nixpkgs.db", &*CACHEDIR));
        }
    }
    let mut url = format!(
        "https://raw.githubusercontent.com/xinux-org/database/main/nixos-{}/nixpkgs.db.br",
        ver_string.trim(),
    );
    // println!("{}", url);
    let mut resp = reqwest::get(&url).await?;
    let mut pkgsout: Vec<u8> = Vec::new();

    if resp.status().is_success() {
        debug!(
            "response getting {:?} pkgs: {:?}",
            ver_string.trim(),
            resp.status()
        );
        let r = resp.bytes().await?;
        // println!("Downloaded");
        let mut br = brotli::Decompressor::new(r.as_ref(), 4096);

        br.read_to_end(&mut pkgsout)
            .context("Failed to decompress brotli data")?;
        debug!("Decompressed");
    } else {
        url = "https://raw.githubusercontent.com/xinux-org/database/main/nixos-unstable/nixpkgs.db.br".to_string();
        debug!("{}", url);
        resp = reqwest::get(url).await?;
        debug!("response getting latest nixpkgs-unstable pkgs: {:?}", resp.status());
        if resp.status().is_success() {
            let r = resp.bytes().await?;
            debug!("Downloaded");
            let mut br = brotli::Decompressor::new(r.as_ref(), 4096);
            br.read_to_end(&mut pkgsout)?;
            debug!("Decompressed");
        }
    }

    let dbfile = format!("{}/nixpkgs.db", &*CACHEDIR);
    let mut out = File::create(&dbfile).context("Failed to create database file")?;
    out.write_all(&pkgsout)
        .context("Failed to write decompressed nixpkgs.db to file")?;

    debug!("Writing nixpkgs.db latest version");
    File::create(format!("{}/nixpkgs.ver", &*CACHEDIR))?.write_all(latestnixpkgsver.as_bytes())?;

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
