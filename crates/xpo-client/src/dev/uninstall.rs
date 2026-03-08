use crate::dev::ca;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::process::{Command, Stdio};

fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("  {spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb.set_message(msg.to_string());
    pb
}

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
    let sp = spinner("Removing port forwarding (sudo)...");
    remove_port_forwarding_platform()?;
    sp.finish_and_clear();
    done("Port forwarding removed");
    Ok(())
}

fn step_untrust_ca() -> Result<(), Box<dyn std::error::Error>> {
    if !is_ca_trusted() {
        done("CA not in trust store");
        return Ok(());
    }

    let sp = spinner("Removing CA from trust store (sudo)...");
    untrust_ca_platform()?;
    sp.finish_and_clear();
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
    let output = Command::new("sudo")
        .args([
            "security",
            "delete-certificate",
            "-c",
            "xpo.sh Development CA",
            "/Library/Keychains/System.keychain",
        ])
        .output()?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to untrust CA: {err}").into());
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
    let output = Command::new("sudo")
        .args(["pfctl", "-a", "com.apple/xpo", "-F", "all"])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .output()?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to flush pf rules: {err}").into());
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
