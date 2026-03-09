pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "\n  {} This will remove all xpo data from your system.\n",
        console::style("⚠").yellow().bold()
    );

    let items = vec!["Yes, uninstall xpo", "No, cancel"];
    let selection =
        dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("  Are you sure?")
            .items(&items)
            .default(1)
            .interact()
            .unwrap_or(1);

    if selection != 0 {
        println!(
            "  {} Cancelled.",
            console::style("→").dim()
        );
        return Ok(());
    }

    println!();

    if crate::dev::ca::ca_exists() {
        print_step("Removing dev setup (CA, trust, port forwarding)...");
        match crate::dev::uninstall::run() {
            Ok(()) => print_done("Dev setup removed"),
            Err(e) => print_warn(&format!("Dev cleanup partial: {e}")),
        }
    } else {
        print_skip("No dev setup found");
    }

    let xpo_dir = xpo_core::config::Config::dir();
    if xpo_dir.exists() {
        print_step("Removing ~/.xpo/ ...");
        std::fs::remove_dir_all(&xpo_dir)?;
        print_done("~/.xpo/ removed");
    } else {
        print_skip("~/.xpo/ not found");
    }

    println!(
        "\n  {} {}",
        console::style("✓").green().bold(),
        console::style("xpo data removed!").bold()
    );

    let exe = std::env::current_exe().ok();
    let exe_path = exe
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "xpo".to_string());

    println!(
        "\n  {} To remove the binary, run:",
        console::style("→").dim()
    );
    println!(
        "    {}",
        console::style(format!("sudo rm {exe_path}")).dim()
    );
    println!(
        "\n  {} Or if installed via a package manager:",
        console::style("→").dim()
    );
    println!("    {}", console::style("brew uninstall xpo").dim());
    println!("    {}", console::style("cargo uninstall xpo").dim());
    println!(
        "    {}",
        console::style("npm uninstall -g @xposh/cli").dim()
    );
    println!();

    Ok(())
}

fn print_step(msg: &str) {
    println!("  {} {msg}", console::style("○").dim());
}

fn print_done(msg: &str) {
    println!(
        "  {} {msg}",
        console::style("✓").green().bold()
    );
}

fn print_warn(msg: &str) {
    println!(
        "  {} {msg}",
        console::style("⚠").yellow().bold()
    );
}

fn print_skip(msg: &str) {
    println!(
        "  {} {msg}",
        console::style("–").dim()
    );
}
