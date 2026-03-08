use console::style;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let hosts = std::fs::read_to_string("/etc/hosts")?;
    let xpo_entries: Vec<&str> = hosts.lines().filter(|l| l.ends_with("# xpo")).collect();

    if xpo_entries.is_empty() {
        println!("  {} Nothing to clean up", style("✓").green().bold());
        return Ok(());
    }

    println!();
    println!("  {}", style("xpo dev stop").bold());
    println!();

    for entry in &xpo_entries {
        let domain = entry.split_whitespace().nth(1).unwrap_or("unknown");
        println!("  {} Removing {}", style("○").dim(), domain);
    }

    let filtered: Vec<&str> = hosts.lines().filter(|l| !l.ends_with("# xpo")).collect();
    let new_hosts = filtered.join("\n") + "\n";

    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("sudo")
        .args(["tee", "/etc/hosts"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(new_hosts.as_bytes())?;

    let status = child.wait()?;
    if !status.success() {
        return Err("Failed to update /etc/hosts".into());
    }

    println!(
        "  {} Cleaned up {} domain{}",
        style("✓").green().bold(),
        xpo_entries.len(),
        if xpo_entries.len() == 1 { "" } else { "s" }
    );
    println!();
    Ok(())
}
