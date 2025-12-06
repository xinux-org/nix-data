use crate::utils::get_full_ver;
use crate::CACHEDIR;
use anyhow::{Context, Result};
use log::debug;
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::Command,
};

use super::{
    nixos::{self, getnixospkgs, nixospkgs},
    // NixPkg,
};

/// Gets a list of all packages in the NixOS system with their name and version.
/// Can be used to find what versions of system packages are currently installed.
/// Will only work on NixOS systems.
pub async fn flakespkgs() -> Result<String> {
    // If cache directory doesn't exist, create it
    if !std::path::Path::new(&*CACHEDIR).exists() {
        std::fs::create_dir_all(&*CACHEDIR)?;
    }

    // we will have internet before install something
    // let mut pinned = false;
    // returns 2x.xx
    let ver = std::process::Command::new("sh")
        .arg("-c")
        .arg(r"nixos-version | grep -oP '^\d+\.\d+'")
        .output()
        .expect("failed to get nixos-version");
    let ver_string = String::from_utf8(ver.stdout)?;

    // hash of commit like: // 2x.xx
    let latestnixpkgsver = get_full_ver().await?;

    // Check if latest version is already downloaded
    // update flakespkgs.ver
    // Write SYSTEM nixos version and it will be used as
    // an old system version on comparing nixospkgs.ver
    let versionout = Command::new("nixos-version").arg("--json").output()?;
    let version: HashMap<String, String> = serde_json::from_slice(&versionout.stdout)?;
    let nixosversion = version
        .get("nixosVersion")
        .context("No NixOS version found")?;
    debug!("Writing flakespkgs.ver version");
    File::create(format!("{}/flakespkgs.ver", &*CACHEDIR))?.write_all(&nixosversion.as_bytes())?;

    if let Ok(prevver) = fs::read_to_string(&format!("{}/flakespkgs.ver", &*CACHEDIR)) {
        if prevver == latestnixpkgsver.clone()
            && Path::new(&format!("{}/flakespkgs.db", &*CACHEDIR)).exists()
        {
            debug!("No new version of flakespkgs found");
            return Ok(format!("{}/flakespkgs.db", &*CACHEDIR));
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
        debug!("response getting latest unstable pkgs: {:?}", resp.status());
        if resp.status().is_success() {
            let r = resp.bytes().await?;
            debug!("Downloaded");
            let mut br = brotli::Decompressor::new(r.as_ref(), 4096);
            br.read_to_end(&mut pkgsout)?;
            debug!("Decompressed");
        }
    }

    let dbfile = format!("{}/flakespkgs.db", &*CACHEDIR);
    let mut out = File::create(&dbfile).context("Failed to create database file")?;
    out.write_all(&pkgsout)
        .context("Failed to write decompressed database to file")?;

    Ok(format!("{}/flakespkgs.db", &*CACHEDIR))
}

/// Returns a list of all installed system packages with their attribute and version
/// The input `paths` should be the paths to the `configuration.nix` files containing `environment.systemPackages`
pub async fn getflakepkgs(paths: &[&str]) -> Result<HashMap<String, String>> {
    // update flakespkgs.ver
    // Write SYSTEM nixos version and it will be used as
    // an old system version on comparing nixospkgs.ver
    let versionout = Command::new("nixos-version").arg("--json").output()?;
    let version: HashMap<String, String> = serde_json::from_slice(&versionout.stdout)?;
    let nixosversion = version
        .get("nixosVersion")
        .context("No NixOS version found")?;
    if nixosversion == &get_full_ver().await?
        && Path::new(&format!("{}/flakespkgs.db", &*CACHEDIR)).exists()
    {
        debug!("Writing new flakespkgs.ver after rebuild");
        File::create(format!("{}/flakespkgs.ver", &*CACHEDIR))?
            .write_all(&nixosversion.as_bytes())?;
    }
    getnixospkgs(paths, nixos::NixosType::Flake).await
}

pub fn uptodate() -> Result<Option<(String, String)>> {
    // returns old and new flake versions.
    let flakesver = fs::read_to_string(&format!("{}/flakespkgs.ver", &*CACHEDIR))?;
    let nixosver = fs::read_to_string(&format!("{}/nixospkgs.ver", &*CACHEDIR))?;
    let flakeslast = flakesver
        .split('.')
        .collect::<Vec<_>>()
        .last()
        .context("Invalid version")?
        .to_string();
    let nixoslast = nixosver
        .split('.')
        .collect::<Vec<_>>()
        .last()
        .context("Invalid version")?
        .to_string();
    if !nixoslast.starts_with(&flakeslast) {
        Ok(Some((flakesver, nixosver)))
    } else {
        Ok(None)
    }
}

pub async fn unavailablepkgs(paths: &[&str]) -> Result<HashMap<String, String>> {
    let versionout = Command::new("nixos-version").arg("--json").output()?;
    let version: HashMap<String, String> = serde_json::from_slice(&versionout.stdout)?;
    let nixpath = if let Some(rev) = version.get("nixpkgsRevision") {
        Command::new("nix")
            .arg("eval")
            .arg(&format!("nixpkgs/{}#path", rev))
            .output()?
            .stdout
    } else {
        Command::new("nix")
            .arg("eval")
            .arg("nixpkgs#path")
            .output()?
            .stdout
    };
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

    let pkgs = {
        let mut allpkgs: HashSet<String> = HashSet::new();
        for path in paths {
            if let Ok(filepkgs) = nix_editor::read::getarrvals(
                &fs::read_to_string(path)?,
                "environment.systemPackages",
            ) {
                let filepkgset = filepkgs
                    .into_iter()
                    .map(|x| x.strip_prefix("pkgs.").unwrap_or(&x).to_string())
                    .collect::<HashSet<_>>();
                allpkgs = allpkgs.union(&filepkgset).map(|x| x.to_string()).collect();
            }
        }
        allpkgs
    };

    let mut unavailable = HashMap::new();
    for pkg in pkgs {
        if aliasesout.contains(&pkg) && Command::new("nix-instantiate")
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
            unavailable.insert(pkg, err.to_string());
        }
    }

    let profilepkgs = getflakepkgs(paths).await?;
    let nixospkgs = nixospkgs().await?;
    let pool = SqlitePool::connect(&format!("sqlite://{}", nixospkgs)).await?;

    for (pkg, _) in profilepkgs {
        let (x, broken, insecure): (String, u8, u8) =
            sqlx::query_as("SELECT attribute,broken,insecure FROM meta WHERE attribute = $1")
                .bind(&pkg)
                .fetch_one(&pool)
                .await?;
        if x != pkg {
            unavailable.insert(
                pkg,
                String::from("Package not found in newer version of nixpkgs"),
            );
        } else if broken == 1 {
            unavailable.insert(pkg, String::from("Package is marked as broken"));
        } else if insecure == 1 {
            unavailable.insert(pkg, String::from("Package is marked as insecure"));
        }
    }
    Ok(unavailable)
}
