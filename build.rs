use std::process::Command;

fn main() {
    glib_build_tools::compile_resources(&["data"], "data/klipp.gresource.xml", "klipp.gresource");

    // Byggnummeret er tidspunktet, så et nyere bygg alltid har et høyere nummer.
    println!("cargo:rerun-if-env-changed=KLIPP_BUILD_NUMBER");
    let stamp = std::env::var("KLIPP_BUILD_NUMBER").unwrap_or_else(|_| {
        Command::new("date")
            .env("TZ", "Europe/Oslo")
            .arg("+%Y%m%d.%H%M")
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .expect("kunne ikke hente byggnummer fra date")
    });
    let stamp = stamp.trim();
    assert!(
        stamp.len() == 13
            && stamp.as_bytes()[8] == b'.'
            && stamp
                .bytes()
                .enumerate()
                .all(|(i, c)| i == 8 || c.is_ascii_digit()),
        "KLIPP_BUILD_NUMBER må ha formatet YYYYMMDD.HHMM"
    );
    println!("cargo:rustc-env=KLIPP_BUILD={}", stamp.trim());
    for path in [
        "src",
        "crates",
        "data",
        "Cargo.toml",
        "CHANGELOG.md",
        "build.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
