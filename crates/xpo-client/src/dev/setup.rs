use crate::dev::ca;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
#[cfg(target_os = "macos")]
use std::path::PathBuf;
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

fn done_dim(msg: &str, detail: &str) {
    println!(
        "  {} {msg} {}",
        style("✓").green().bold(),
        style(detail).dim()
    );
}

#[cfg(target_os = "macos")]
pub fn pf_token_path() -> PathBuf {
    xpo_core::config::Config::dir().join("pf_token")
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("  {}", style("xpo dev setup").bold());
    println!();

    step_generate_ca()?;
    step_trust_ca()?;
    step_port_forwarding()?;

    println!();
    println!(
        "  {} Run: {}",
        style("Setup complete!").green().bold(),
        style("xpo dev 3000 -n myapp").cyan()
    );
    println!();
    Ok(())
}

fn step_generate_ca() -> Result<(), Box<dyn std::error::Error>> {
    if ca::ca_exists() {
        done_dim("Root CA exists", &ca::ca_cert_path().display().to_string());
    } else {
        let sp = spinner("Generating root CA...");
        ca::generate_ca()?;
        sp.finish_and_clear();
        done_dim(
            "Root CA created (P-256 ECDSA, 10yr)",
            &ca::ca_cert_path().display().to_string(),
        );
    }
    Ok(())
}

fn step_trust_ca() -> Result<(), Box<dyn std::error::Error>> {
    if is_ca_trusted() {
        done("CA trusted in system keychain");
    } else {
        println!("  {} Trusting CA in system keychain...", style("○").dim());
        trust_ca_platform()?;
        done("CA trusted in system keychain");
    }
    Ok(())
}

fn step_port_forwarding() -> Result<(), Box<dyn std::error::Error>> {
    if is_port_forwarding_active() {
        done_dim("Port forwarding active", "443→10443, 80→10080");
    } else {
        setup_port_forwarding_platform()?;
        done_dim("Port forwarding active", "443→10443, 80→10080");
    }
    Ok(())
}

// --- Check functions (no sudo needed) ---

#[cfg(target_os = "macos")]
pub(crate) fn is_ca_trusted() -> bool {
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

#[cfg(target_os = "linux")]
pub(crate) fn is_ca_trusted() -> bool {
    let (cert_path, _) = linux_ca_paths();
    std::path::Path::new(cert_path).exists()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn is_ca_trusted() -> bool {
    false
}

pub(crate) fn pf_output_has_forwarding(output: &[u8]) -> bool {
    let s = String::from_utf8_lossy(output);
    s.contains("10443") && s.contains("10080")
}

#[cfg(target_os = "macos")]
pub(crate) fn verify_pf_runtime_state() -> bool {
    let output = Command::new("sudo")
        .args(["-n", "pfctl", "-sn", "-a", "com.xpo"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(o) => pf_output_has_forwarding(&o.stdout),
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(crate) fn auto_reload_pf() -> Result<(), Box<dyn std::error::Error>> {
    let _ = Command::new("sudo")
        .args(["-n", "pfctl", "-e"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let status = Command::new("sudo")
        .args(["-n", "pfctl", "-f", "/etc/pf.conf"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err("pfctl reload failed".into())
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn is_port_forwarding_active() -> bool {
    let anchor_exists = std::path::Path::new("/etc/pf.anchors/com.xpo").exists();
    let pf_configured = std::fs::read_to_string("/etc/pf.conf")
        .map(|c| c.contains("rdr-anchor \"com.xpo\""))
        .unwrap_or(false);
    let runtime_active = verify_pf_runtime_state();
    anchor_exists && pf_configured && runtime_active
}

#[cfg(target_os = "linux")]
pub(crate) fn is_port_forwarding_active() -> bool {
    Command::new("sudo")
        .args([
            "iptables",
            "-t",
            "nat",
            "-C",
            "OUTPUT",
            "-o",
            "lo",
            "-p",
            "tcp",
            "--dport",
            "443",
            "-j",
            "REDIRECT",
            "--to-port",
            "10443",
        ])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn is_port_forwarding_active() -> bool {
    false
}

// --- Setup functions (sudo needed) ---

#[cfg(target_os = "macos")]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("sudo")
        .args([
            "security",
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-k",
            "/Library/Keychains/System.keychain",
        ])
        .arg(ca::ca_cert_path())
        .status()?;

    if !status.success() {
        return Err("Failed to trust CA".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_ca_paths() -> (&'static str, &'static str) {
    if std::path::Path::new("/etc/pki/ca-trust/source/anchors").exists() {
        (
            "/etc/pki/ca-trust/source/anchors/xpo-ca.pem",
            "update-ca-trust",
        )
    } else {
        (
            "/usr/local/share/ca-certificates/xpo-ca.crt",
            "update-ca-certificates",
        )
    }
}

#[cfg(target_os = "linux")]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    let (cert_dest, update_cmd) = linux_ca_paths();

    let output = Command::new("sudo")
        .args(["cp", &ca::ca_cert_path().to_string_lossy(), cert_dest])
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to copy CA cert to {cert_dest}: {err}").into());
    }

    let output = Command::new("sudo")
        .args([update_cmd])
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to run {update_cmd}: {err}").into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn trust_ca_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "  {} Manual trust required: add {} to your trust store",
        style("!").yellow().bold(),
        ca::ca_cert_path().display()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    let anchor_content = "\
rdr pass on lo0 inet proto tcp from any to any port 443 -> 127.0.0.1 port 10443\n\
rdr pass on lo0 inet proto tcp from any to any port 80 -> 127.0.0.1 port 10080\n";

    let anchor_path = "/etc/pf.anchors/com.xpo";
    let tmp_anchor = std::env::temp_dir().join("xpo-anchor");
    std::fs::write(&tmp_anchor, anchor_content)?;

    let output = Command::new("sudo")
        .args(["cp", &tmp_anchor.to_string_lossy(), anchor_path])
        .stderr(Stdio::piped())
        .output()?;
    let _ = std::fs::remove_file(&tmp_anchor);
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to write anchor file: {err}").into());
    }

    let pf_conf = std::fs::read_to_string("/etc/pf.conf").unwrap_or_default();
    let new_conf = build_pf_conf_with_anchor(&pf_conf);
    let tmp = std::env::temp_dir().join("xpo-pf.conf");
    std::fs::write(&tmp, &new_conf)?;

    let output = Command::new("sudo")
        .args(["cp", &tmp.to_string_lossy(), "/etc/pf.conf"])
        .stderr(Stdio::piped())
        .output()?;
    let _ = std::fs::remove_file(&tmp);

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to update /etc/pf.conf: {err}").into());
    }

    let output = Command::new("sudo")
        .args(["pfctl", "-E"])
        .stderr(Stdio::piped())
        .output()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(token) = parse_pf_token(&stderr) {
        let token_path = pf_token_path();
        std::fs::write(&token_path, token.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600))?;
        }
    }

    let output = Command::new("sudo")
        .args(["pfctl", "-f", "/etc/pf.conf"])
        .stderr(Stdio::piped())
        .output()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success()
        && !stderr.contains("ALTQ")
        && !stderr.contains("already enabled")
        && !stderr.contains("pf already enabled")
    {
        return Err(format!("Failed to reload pf rules: {stderr}").into());
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn parse_pf_token(stderr: &str) -> Option<u64> {
    for line in stderr.lines() {
        if let Some(rest) = line.strip_prefix("Token : ") {
            return rest.trim().parse().ok();
        }
    }
    None
}

#[cfg(any(target_os = "macos", test))]
fn build_pf_conf_with_anchor(pf_conf: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut skip = false;
    let mut inserted_rdr = pf_conf.contains("rdr-anchor \"com.xpo\"");
    let mut inserted_load = pf_conf.contains("load anchor \"com.xpo\"");
    let mut last_rdr_anchor_idx: Option<usize> = None;

    for line in pf_conf.lines() {
        if line.contains("# xpo-start") {
            skip = true;
            continue;
        }
        if line.contains("# xpo-end") {
            skip = false;
            continue;
        }
        if skip {
            continue;
        }
        lines.push(line);
        if line.starts_with("rdr-anchor") {
            last_rdr_anchor_idx = Some(lines.len() - 1);
        }
    }

    if !inserted_rdr {
        let rdr_line = "rdr-anchor \"com.xpo\"";
        if let Some(idx) = last_rdr_anchor_idx {
            lines.insert(idx + 1, rdr_line);
        } else {
            lines.push(rdr_line);
        }
        inserted_rdr = true;
    }

    if !inserted_load {
        lines.push("load anchor \"com.xpo\" from \"/etc/pf.anchors/com.xpo\"");
        inserted_load = true;
    }

    let _ = (inserted_rdr, inserted_load);
    lines.join("\n") + "\n"
}

#[cfg(target_os = "linux")]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    for (from, to) in [(443, 10443), (80, 10080)] {
        let check = Command::new("sudo")
            .args([
                "iptables",
                "-t",
                "nat",
                "-C",
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
            .stderr(Stdio::null())
            .status()?;

        if !check.success() {
            let output = Command::new("sudo")
                .args([
                    "iptables",
                    "-t",
                    "nat",
                    "-A",
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
                .output()?;

            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to add iptables rule for port {from}: {err}").into());
            }
        }
    }

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn setup_port_forwarding_platform() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "  {} Port forwarding not supported on this platform",
        style("!").yellow().bold()
    );
    println!("     Proxy will listen on :10443 and :10080 directly");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_old_xpo_block_to_anchor() {
        let old_pf = "\
scrub-anchor \"com.apple/*\"
nat-anchor \"com.apple/*\"
rdr-anchor \"com.apple/*\"
# xpo-start
rdr pass on lo0 inet proto tcp from any to any port 443 -> 127.0.0.1 port 10443
rdr pass on lo0 inet proto tcp from any to any port 80 -> 127.0.0.1 port 10080
# xpo-end
dummynet-anchor \"com.apple/*\"
anchor \"com.apple/*\"
load anchor \"com.apple\" from \"/etc/pf.anchors/com.apple\"";

        let result = build_pf_conf_with_anchor(old_pf);

        assert!(
            !result.contains("# xpo-start"),
            "old markers should be stripped"
        );
        assert!(
            !result.contains("# xpo-end"),
            "old markers should be stripped"
        );
        assert!(
            !result.contains("rdr pass on lo0"),
            "old inline rules should be stripped"
        );
        assert!(
            result.contains("rdr-anchor \"com.xpo\""),
            "should have anchor ref"
        );
        assert!(
            result.contains("load anchor \"com.xpo\" from \"/etc/pf.anchors/com.xpo\""),
            "should have load anchor"
        );
    }

    #[test]
    fn anchor_inserted_after_last_rdr_anchor() {
        let pf = "\
scrub-anchor \"com.apple/*\"
nat-anchor \"com.apple/*\"
rdr-anchor \"com.apple/*\"
dummynet-anchor \"com.apple/*\"
anchor \"com.apple/*\"
load anchor \"com.apple\" from \"/etc/pf.anchors/com.apple\"";

        let result = build_pf_conf_with_anchor(pf);
        let lines: Vec<&str> = result.lines().collect();

        let apple_rdr_idx = lines
            .iter()
            .position(|l| *l == "rdr-anchor \"com.apple/*\"")
            .unwrap();
        let xpo_rdr_idx = lines
            .iter()
            .position(|l| *l == "rdr-anchor \"com.xpo\"")
            .unwrap();

        assert_eq!(
            xpo_rdr_idx,
            apple_rdr_idx + 1,
            "xpo anchor should follow apple rdr-anchor"
        );
    }

    #[test]
    fn idempotent_does_not_duplicate() {
        let pf = "\
rdr-anchor \"com.apple/*\"
rdr-anchor \"com.xpo\"
anchor \"com.apple/*\"
load anchor \"com.xpo\" from \"/etc/pf.anchors/com.xpo\"";

        let result = build_pf_conf_with_anchor(pf);
        let rdr_count = result.matches("rdr-anchor \"com.xpo\"").count();
        let load_count = result.matches("load anchor \"com.xpo\"").count();
        assert_eq!(rdr_count, 1, "should have exactly 1 rdr-anchor");
        assert_eq!(load_count, 1, "should have exactly 1 load anchor");
    }

    #[test]
    fn no_rdr_anchor_appends_at_end() {
        let pf = "anchor \"com.apple/*\"";
        let result = build_pf_conf_with_anchor(pf);

        assert!(result.contains("rdr-anchor \"com.xpo\""));
        assert!(result.contains("load anchor \"com.xpo\""));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_pf_token_valid() {
        assert_eq!(parse_pf_token("Token : 12345\n"), Some(12345));
        assert_eq!(
            parse_pf_token("pf enabled\nToken : 42\nsome other line"),
            Some(42)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_pf_token_missing() {
        assert_eq!(parse_pf_token("pf enabled\n"), None);
        assert_eq!(parse_pf_token(""), None);
    }

    #[test]
    fn verify_pf_runtime_state_parses_anchor_output() {
        let output_with_rules = b"rdr pass on lo0 inet proto tcp from any to any port = 443 -> 127.0.0.1 port 10443\nrdr pass on lo0 inet proto tcp from any to any port = 80 -> 127.0.0.1 port 10080\n";
        assert!(pf_output_has_forwarding(output_with_rules));

        let output_empty = b"";
        assert!(!pf_output_has_forwarding(output_empty));

        let output_only_https =
            b"rdr pass on lo0 inet proto tcp from any to any port = 443 -> 127.0.0.1 port 10443\n";
        assert!(
            !pf_output_has_forwarding(output_only_https),
            "must have BOTH 10443 and 10080"
        );

        let output_other =
            b"rdr pass on lo0 inet proto tcp from any to any port = 443 -> 127.0.0.1 port 8443\n";
        assert!(!pf_output_has_forwarding(output_other));
    }
}
