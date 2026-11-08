use std::process::Command;

fn main() {
    glib_build_tools::compile_resources(&["data"], "data/klipp.gresource.xml", "klipp.gresource");

    // Byggnummeret er tidspunktet, så et nyere bygg alltid har et høyere nummer.
    let stamp = Command::new("date")
        .arg("+%Y%m%d.%H%M")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .unwrap_or_default();
    println!("cargo:rustc-env=KLIPP_BUILD={}", stamp.trim());
    for path in ["src", "crates", "data", "Cargo.toml"] {
        println!("cargo:rerun-if-changed={path}");
    }
}
