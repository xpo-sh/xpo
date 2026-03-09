use std::io::Write;

const REPO: &str = "xpo-sh/xpo";

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let current = env!("CARGO_PKG_VERSION");
    println!(
        "  {} Current version: {}",
        console::style("→").dim(),
        console::style(format!("v{current}")).cyan()
    );

    print!("  {} Checking for updates...", console::style("○").dim());
    std::io::stdout().flush()?;

    let latest = fetch_latest_version().await?;
    let latest_clean = latest.trim_start_matches('v');

    if latest_clean == current {
        println!(
            "\r\x1b[2K  {} Already up to date.",
            console::style("✓").green().bold()
        );
        return Ok(());
    }

    println!(
        "\r\x1b[2K  {} New version available: {}",
        console::style("✓").green().bold(),
        console::style(&latest).cyan().bold()
    );

    let target = detect_target()?;
    let url = format!("https://github.com/{REPO}/releases/download/{latest}/xpo-{target}.tar.gz");

    print!("  {} Downloading...", console::style("○").dim());
    std::io::stdout().flush()?;

    let bytes = reqwest::get(&url)
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    println!(
        "\r\x1b[2K  {} Downloaded ({:.1} MB)",
        console::style("✓").green().bold(),
        bytes.len() as f64 / 1_048_576.0
    );

    let current_exe = std::env::current_exe()?;
    let tmpdir = tempfile::tempdir()?;
    let archive_path = tmpdir.path().join("xpo.tar.gz");
    std::fs::write(&archive_path, &bytes)?;

    let output = std::process::Command::new("tar")
        .args([
            "xzf",
            archive_path.to_str().unwrap(),
            "-C",
            tmpdir.path().to_str().unwrap(),
        ])
        .output()?;

    if !output.status.success() {
        return Err("Failed to extract archive".into());
    }

    let new_binary = tmpdir.path().join("xpo");
    if !new_binary.exists() {
        return Err("Binary not found in archive".into());
    }

    print!("  {} Installing...", console::style("○").dim());
    std::io::stdout().flush()?;

    let installed = try_install(&new_binary, &current_exe)?;
    if !installed {
        let status = std::process::Command::new("sudo")
            .args([
                "cp",
                new_binary.to_str().unwrap(),
                current_exe.to_str().unwrap(),
            ])
            .status()?;
        if !status.success() {
            return Err("Failed to install binary".into());
        }
    }

    println!(
        "\r\x1b[2K  {} Installed to {}",
        console::style("✓").green().bold(),
        current_exe.display()
    );

    println!(
        "\n  {} {}",
        console::style("✓").green().bold(),
        console::style(format!("Updated to {latest}!")).bold()
    );

    Ok(())
}

fn try_install(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    match std::fs::copy(src, dst) {
        Ok(_) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755));
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(false),
        Err(e) => Err(e.into()),
    }
}

async fn fetch_latest_version() -> Result<String, Box<dyn std::error::Error>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases");
    let resp: Vec<serde_json::Value> = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "xpo-cli")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    resp.first()
        .and_then(|r| r["tag_name"].as_str())
        .map(String::from)
        .ok_or_else(|| "No releases found".into())
}

fn detect_target() -> Result<String, Box<dyn std::error::Error>> {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    let target_arch = match arch {
        "x86_64" | "amd64" => "x86_64",
        "aarch64" | "arm64" => "aarch64",
        _ => return Err(format!("Unsupported architecture: {arch}").into()),
    };

    let target_os = match os {
        "macos" => "apple-darwin",
        "linux" => "unknown-linux-musl",
        _ => return Err(format!("Unsupported OS: {os}").into()),
    };

    Ok(format!("{target_arch}-{target_os}"))
}
