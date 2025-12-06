use crate::HOME;
use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

/// Refreshes desktop icons for applications installed with Nix
pub fn refreshicons() -> Result<()> {
    let desktoppath = &format!("{}/.local/share/applications", &*HOME);
    let iconpath = &format!("{}/.local/share/icons/nixrefresh.png", &*HOME);
    fs::create_dir_all(desktoppath)?;
    fs::create_dir_all(&format!("{}/.local/share/icons", &*HOME))?;

    // Clean up old files
    for filename in (fs::read_dir(desktoppath)?).flatten() {
        if filename.file_type()?.is_file()
            && fs::read_to_string(filename.path())?.lines().next() == Some("# Nix Desktop Entry")
        {
            fs::remove_file(filename.path())?;
        }
    }

    for filename in
        (fs::read_dir(&format!("{}/.nix-profile/share/applications", &*HOME))?).flatten()
    {
        let filepath = filename.path().to_str().context("file path")?.to_string();
        let localpath = format!(
            "{}/{}",
            desktoppath,
            filename.file_name().to_str().context("file name")?
        );
        if Path::new(&localpath).exists() {
            fs::remove_file(&localpath)?;
        }
        fs::copy(&filepath, &localpath)?;
        // Write "# Nix Desktop Entry" to the top of the file
        let mut file = File::open(&localpath)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        contents = format!("# Nix Desktop Entry\n{}", contents);
        fs::remove_file(&localpath)?;
        let mut file = File::create(&localpath)?;
        file.write_all(contents.as_bytes())?;
        let mut perms = fs::metadata(&localpath)?.permissions();
        perms.set_readonly(true);
        fs::set_permissions(&localpath, perms)?;
    }

    if Path::new(iconpath).exists() {
        fs::remove_file(iconpath)?;
    }
    File::create(iconpath)?;
    if Path::new(iconpath).exists() {
        fs::remove_file(iconpath)?;
    }

    Ok(())
}

pub async fn get_full_ver() -> Result<String> {
    // returns full nixos version of system 25.11.asdasd.asd
    let short_version = std::process::Command::new("sh")
        .arg("-c")
        .arg(r"nixos-version | grep -oP '^\d+\.\d+'")
        .output()
        .expect("failed to get nixos-version");
    let v = String::from_utf8(short_version.stdout)?;
    let url = format!(
        "https://raw.githubusercontent.com/xinux-org/database/refs/heads/main/nixos-{}/nixpkgs.ver",
        v.trim()
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
            eprintln!("Primary nixpkgs.ver fetch failed, trying unstable...");
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
            "Failed to fetch version from both release and unstable channel versions"
        ));
    }

    Ok(fallback.text().await?)
}
