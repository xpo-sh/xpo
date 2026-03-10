use std::io;

pub struct DoctorCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
}

fn color(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

pub fn render_doctor_table(checks: &[DoctorCheck]) -> io::Result<()> {
    let r = "\x1b[0m";
    let b = "\x1b[1m";
    let bd = color(72, 79, 88);
    let tx = color(230, 237, 243);
    let dm = color(139, 148, 158);
    let ac = color(88, 166, 255);
    let ok = color(126, 231, 135);
    let er = color(248, 81, 73);
    let wn = color(224, 175, 104);

    let nw = checks
        .iter()
        .map(|c| c.name.len())
        .max()
        .unwrap_or(10)
        .max(10);
    let dw = checks
        .iter()
        .map(|c| c.detail.len())
        .max()
        .unwrap_or(6)
        .max(6);

    let content_width = 1 + nw + 2 + 8 + 2 + dw + 1;

    let hr = "\u{2500}".repeat(content_width);

    println!();
    println!("  {bd}\u{256d}{hr}\u{256e}{r}");

    let title = "xpo dev doctor";
    let title_pad = content_width - 1 - title.len();
    println!(
        "  {bd}\u{2502}{r} {ac}{b}{title}{r}{}{bd}\u{2502}{r}",
        " ".repeat(title_pad)
    );

    println!("  {bd}\u{251c}{hr}\u{2524}{r}");

    let hdr_pad = content_width - 1 - nw - 2 - 8 - 2 - dw;
    println!(
        "  {bd}\u{2502}{r} {dm}{:<nw$}  {:<8}  {:<dw$}{r}{}{bd}\u{2502}{r}",
        "CHECK",
        "STATUS",
        "DETAIL",
        " ".repeat(hdr_pad),
        nw = nw,
        dw = dw
    );

    for check in checks {
        let (icon, ic) = match &check.status {
            CheckStatus::Pass => ("\u{2713} pass", &ok),
            CheckStatus::Fail => ("\u{2717} fail", &er),
            CheckStatus::Warn => ("! warn", &wn),
        };
        let row_pad = content_width - 1 - nw - 2 - 8 - 2 - check.detail.len();
        println!(
            "  {bd}\u{2502}{r} {tx}{:<nw$}{r}  {ic}{:<8}{r}  {dm}{}{r}{}{bd}\u{2502}{r}",
            check.name,
            icon,
            check.detail,
            " ".repeat(row_pad),
            nw = nw
        );
    }

    println!("  {bd}\u{251c}{hr}\u{2524}{r}");

    let summary = format!(
        "Result: {pass_count}/{total_count} checks passed",
        pass_count = checks
            .iter()
            .filter(|c| matches!(c.status, CheckStatus::Pass))
            .count(),
        total_count = checks.len()
    );
    let sc = if checks.iter().any(|c| matches!(c.status, CheckStatus::Fail)) {
        &er
    } else {
        &ok
    };
    let sum_pad = content_width - 1 - summary.len();
    println!(
        "  {bd}\u{2502}{r} {sc}{summary}{r}{}{bd}\u{2502}{r}",
        " ".repeat(sum_pad)
    );

    if checks.iter().any(|c| matches!(c.status, CheckStatus::Fail)) {
        for check in checks {
            if matches!(check.status, CheckStatus::Fail) {
                let msg = format!("\u{2717} {}", check.name);
                let fail_pad = content_width - 1 - msg.len();
                println!(
                    "  {bd}\u{2502}{r} {er}{msg}{r}{}{bd}\u{2502}{r}",
                    " ".repeat(fail_pad)
                );
            }
        }
    }

    println!("  {bd}\u{2570}{hr}\u{256f}{r}");
    println!();

    Ok(())
}
