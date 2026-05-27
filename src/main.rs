use anyhow::anyhow;
use reqwest::Client;
use tokio::runtime::Runtime;

fn main() {
    let rt = Runtime::new().expect("Tokio runtime error here");

    let foo = rt
        .block_on(get_full_ver())
        .expect("Errro getting on full version 2x.xx.xx.8bb5646e0bed");
    println!("get_full_rev: {:?}", rt.block_on(get_full_rev(&foo)));
    println!("get_full_ver: {:?}", rt.block_on(get_full_ver()));
    println!("latest nixospkgs hash: {:?}", rt.block_on(nixospkgs()));

    println!(
        "latest nixospkgs hash: {:?}",
        nix_data_xinux::cache::flakes::uptodate()
    );
}

async fn nixospkgs() -> Result<String, anyhow::Error> {
    nix_data_xinux::cache::flakes::flakespkgs().await
}
async fn get_full_rev(version: &str) -> Result<String, anyhow::Error> {
    let short = version.split('.').last().unwrap();

    let url = format!(
        "https://api.github.com/repos/NixOS/nixpkgs/commits/{}",
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

async fn get_full_ver() -> Result<String, anyhow::Error> {
    let short_version = std::process::Command::new("sh")
        .arg("-c")
        .arg(r"nixos-version | grep -oP '^\d+\.\d+'")
        .output()
        .expect("failed to get nixos-version");
    let url = format!(
        "https://git.oss.uzinfocom.uz/xinux/database/raw/branch/main/nixos-{:?}/nixpkgs.ver",
        String::from_utf8(short_version.stdout)
    );

    // Fallback URL
    let url_unstable =
        "https://git.oss.uzinfocom.uz/xinux/database/raw/branch/main/nixos-unstable/nixpkgs.ver";

    let client = Client::new();

    // Try release channel first
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
