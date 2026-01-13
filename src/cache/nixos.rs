use crate::CACHEDIR;
use crate::utils::get_full_ver;
use anyhow::{Context, Result, anyhow};
use log::debug;
use sqlx::{Row, Sqlite, SqlitePool, migrate::MigrateDatabase};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

use super::{channel, flakes};

/// Downloads the latest `packages.json` for the system from the NixOS cache and returns the path to an SQLite database `nixospkgs.db` which contains package data.
/// Will only work on NixOS systems.
pub async fn nixospkgs() -> Result<String> {
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

    if let Ok(prevver) = fs::read_to_string(format!("{}/nixospkgs.ver", &*CACHEDIR))
        && prevver == latestnixpkgsver.clone()
        && Path::new(&format!("{}/nixospkgs.db", &*CACHEDIR)).exists()
    {
        debug!("No new version of flakespkgs found");
        return Ok(format!("{}/nixospkgs.db", &*CACHEDIR));
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

    let dbfile = format!("{}/nixospkgs.db", &*CACHEDIR);
    let mut out = File::create(&dbfile).context("Failed to create database file")?;
    out.write_all(&pkgsout)
        .context("Failed to write decompressed nixospkgs.db to file")?;

    debug!("Writing nixospkgs.db latest version");
    File::create(format!("{}/nixospkgs.ver", &*CACHEDIR))?
        .write_all(latestnixpkgsver.as_bytes())?;

    Ok(format!("{}/nixospkgs.db", &*CACHEDIR))
}

/// Downloads the latest 'options.json' for the system from the NixOS cache and returns the path to the file.
/// Will only work on NixOS systems.
pub fn nixosoptions() -> Result<String> {
    let versionout = Command::new("nixos-version").output()?;
    let mut version = &String::from_utf8(versionout.stdout)?[0..5];

    // If cache directory doesn't exist, create it
    if !std::path::Path::new(&*CACHEDIR).exists() {
        std::fs::create_dir_all(&*CACHEDIR)?;
    }

    let verurl = format!("https://channels.nixos.org/nixos-{}", version);
    debug!("Checking NixOS version");
    let resp = reqwest::blocking::get(&verurl)?;
    let latestnixosver = if resp.status().is_success() {
        resp.url()
            .path_segments()
            .context("No path segments found")?
            .next_back()
            .context("Last element not found")?
            .to_string()
    } else {
        let resp = reqwest::blocking::get("https://channels.nixos.org/nixos-unstable")?;
        if resp.status().is_success() {
            version = "unstable";
            resp.url()
                .path_segments()
                .context("No path segments found")?
                .next_back()
                .context("Last element not found")?
                .to_string()
        } else {
            return Err(anyhow!("Could not find latest NixOS version"));
        }
    };
    debug!("Latest NixOS version: {}", latestnixosver);

    let url = format!(
        "https://channels.nixos.org/nixos-{}/options.json.br",
        version
    );

    // Download file with reqwest blocking
    let client = reqwest::blocking::Client::builder().brotli(true).build()?;
    let mut resp = client.get(url).send()?;
    if resp.status().is_success() {
        let mut out = File::create(format!("{}/nixosoptions.json", &*CACHEDIR))?;
        resp.copy_to(&mut out)?;
        // Write version downloaded to file
        File::create(format!("{}/nixosoptions.ver", &*CACHEDIR))?
            .write_all(latestnixosver.as_bytes())?;
    } else {
        return Err(anyhow!("Failed to download latest options.json"));
    }

    Ok(format!("{}/nixosoptions.json", &*CACHEDIR))
}

pub(super) enum NixosType {
    Flake,
    Legacy,
}

pub(super) async fn getnixospkgs(
    paths: &[&str],
    nixos: NixosType,
) -> Result<HashMap<String, String>> {
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
    debug!("getnixospkgs: {:?}", pkgs);
    let pkgsdb = match nixos {
        NixosType::Flake => flakes::flakespkgs().await?,
        NixosType::Legacy => channel::legacypkgs().await?,
    };
    let mut out = HashMap::new();
    let pool = SqlitePool::connect(&format!("sqlite://{}", pkgsdb)).await?;
    for pkg in pkgs {
        let mut sqlout = sqlx::query(
            r#"
            SELECT version FROM pkgs WHERE attribute = $1
            "#,
        )
        .bind(&pkg)
        .fetch_all(&pool)
        .await?;
        if sqlout.len() == 1 {
            let row = sqlout.pop().unwrap();
            let version: String = row.get("version");
            out.insert(pkg, version);
        }
    }
    Ok(out)
}

pub(super) async fn createdb(dbfile: &str, pkgjson: &HashMap<String, String>) -> Result<()> {
    let db = format!("sqlite://{}", dbfile);
    if Path::new(dbfile).exists() {
        fs::remove_file(dbfile)?;
    }
    Sqlite::create_database(&db).await?;
    let pool = SqlitePool::connect(&db).await?;
    sqlx::query(
        r#"
            CREATE TABLE "pkgs" (
                "attribute"	TEXT NOT NULL UNIQUE,
                "version"	TEXT,
                PRIMARY KEY("attribute")
            )
            "#,
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        r#"
        CREATE UNIQUE INDEX "attributes" ON "pkgs" ("attribute")
        "#,
    )
    .execute(&pool)
    .await?;

    let mut wtr = csv::Writer::from_writer(vec![]);
    for (pkg, version) in pkgjson {
        wtr.serialize((pkg.to_string(), version.to_string()))?;
    }
    let data = String::from_utf8(wtr.into_inner()?)?;
    let mut cmd = Command::new("sqlite3")
        .arg("-csv")
        .arg(dbfile)
        .arg(".import '|cat -' pkgs")
        .stdin(Stdio::piped())
        .spawn()?;
    let cmd_stdin = cmd.stdin.as_mut().unwrap();
    cmd_stdin.write_all(data.as_bytes())?;
    let _status = cmd.wait()?;
    Ok(())
}
