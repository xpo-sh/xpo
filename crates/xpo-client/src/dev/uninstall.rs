use crate::dev::ca;
use console::style;
use std::process::Command;

fn done(msg: &str) {
    println!("  {} {msg}", style("✓").green().bold());
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("  {}", style("xpo dev uninstall").bold());
    println!();

    step_remove_port_forwarding()?;
    step_untrust_ca()?;
    step_remove_ca()?;

    println!();
    println!("  {} Uninstalled.", style("✓").green().bold());
    println!();
    Ok(())
}

fn step_remove_port_forwarding() -> Result<(), Box<dyn std::error::Error>> {
    println!("  {} Removing port forwarding...", style("○").dim());
    remove_port_forwarding_platform()?;
    done("Port forwarding removed");
    Ok(())
}

fn step_untrust_ca() -> Result<(), Box<dyn std::error::Error>> {
    if !is_ca_trusted() {
        done("CA not in trust store");
        return Ok(());
    }

    println!("  {} Removing CA from trust store...", style("○").dim());
    untrust_ca_platform()?;
    done("CA removed from trust store");
    Ok(())
}

fn step_remove_ca() -> Result<(), Box<dyn std::error::Error>> {
    let dir = ca::ca_dir();
    if !dir.exists() {
        done("CA files not found");
        return Ok(());
    }

    std::fs::remove_dir_all(&dir)?;
    done("CA files deleted");
    Ok(())
}

// --- Check functions ---

#[cfg(target_os = "macos")]
fn is_ca_trusted() -> bool {
    use std::process::Stdio;
    Command::new("security")
        .args([
            "find-certificate",
            "-c",
            "xpo.sh Development CA",
            "/Library/Keychains/System.keychain",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn is_ca_trusted() -> bool {
    std::path::Path::new("/usr/local/share/ca-certificates/xpo-ca.crt").exists()
}

// --- Remove functions ---

#[cfg(target_os = "macos")]
fn untrust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("sudo")
        .args([
            "security",
            "delete-certificate",
            "-c",
            "xpo.sh Development CA",
            "/Library/Keychains/System.keychain",
        ])
        .status()?;

    if !status.success() {
        return Err("Failed to untrust CA".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn untrust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let _ = Command::new("sudo")
        .args(["rm", "-f", "/usr/local/share/ca-certificates/xpo-ca.crt"])
        .output()?;

    let _ = Command::new("sudo")
        .args(["update-ca-certificates", "--fresh"])
        .output()?;

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn untrust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "  {} Remove CA manually from your trust store",
        style("!").yellow().bold()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    use std::process::Stdio;
    let status = Command::new("sudo")
        .args(["pfctl", "-a", "com.apple/xpo", "-F", "all"])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status()?;

    if !status.success() {
        return Err("Failed to flush pf rules".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    for (from, to) in [(443, 10443), (80, 10080)] {
        let _ = Command::new("sudo")
            .args([
                "iptables",
                "-t",
                "nat",
                "-D",
                "OUTPUT",
                "-o",
                "lo",
                "-p",
                "tcp",
                "--dport",
                &from.to_string(),
                "-j",
                "REDIRECT",
                "--to-port",
                &to.to_string(),
            ])
            .output();
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remove_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
