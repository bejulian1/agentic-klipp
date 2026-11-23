#!/usr/bin/env bash
# Pakker en allerede bygd Linux x86_64-binær. Ingen tester eller byggemapper tas med.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
binary=${1:-${CARGO_TARGET_DIR:-$here/target}/release/agentic-screenshot}
output=${2:-$here/dist}
[[ -x "$binary" ]] || { echo "Fant ikke kjørbar binær: $binary" >&2; exit 1; }
version=$(sed -n '/^\[package\]/,/^\[/s/^version = "\([^"]*\)"/\1/p' "$here/Cargo.toml")
info=$("$binary" --version)
# Ordet for bygg følger språket, så bare navnet, versjonen og nummeret sjekkes.
[[ "$info" == "agentic-screenshot $version ("*")" ]] || { echo "Binær og Cargo.toml har ulik versjon" >&2; exit 1; }
build=${info##* }
build=${build%)}
[[ "$build" =~ ^[0-9]{8}\.[0-9]{4}$ ]] || { echo "Ugyldig byggnummer: $info" >&2; exit 1; }
[[ $(uname -m) == x86_64 ]] || { echo "Pakkeskriptet støtter x86_64" >&2; exit 1; }
mkdir -p "$output"
output=$(cd -- "$output" && pwd)
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
name=agentic-screenshot-$version-linux-x86_64
bundle=$stage/$name
mkdir -p "$bundle"
install -m755 "$binary" "$bundle/agentic-screenshot"
install -m755 "$here/install.sh" "$here/uninstall.sh" "$bundle/"
cp -a "$here/data" "$here/shell-extension" "$bundle/"
cp -a "$here/docs" "$bundle/"
install -m644 "$here/README.md" "$here/CHANGELOG.md" "$here/CHANGELOG.nb.md" "$here/CONTRIBUTORS.md" "$here/LICENSE" "$bundle/"
printf '%s\n' "$info" > "$bundle/BUILD.txt"
install -m755 "$binary" "$output/agentic-screenshot"
tar -C "$stage" -czf "$output/$name.tar.gz" "$name"

deb=$stage/deb
install -Dm755 "$binary" "$deb/usr/bin/agentic-screenshot"
install -Dm644 "$here/data/icons/hicolor/scalable/apps/io.github.AgenticScreenshot.svg" "$deb/usr/share/icons/hicolor/scalable/apps/io.github.AgenticScreenshot.svg"
install -Dm644 "$here/data/icons/hicolor/symbolic/apps/io.github.AgenticScreenshot-symbolic.svg" "$deb/usr/share/icons/hicolor/symbolic/apps/io.github.AgenticScreenshot-symbolic.svg"
extension=agentic-screenshot@bejulian1.github.io
mkdir -p "$deb/usr/share/gnome-shell/extensions/$extension" "$deb/usr/share/applications" "$deb/usr/share/doc/agentic-screenshot" "$deb/DEBIAN"
install -m644 "$here/shell-extension/$extension/extension.js" "$here/shell-extension/$extension/metadata.json" "$deb/usr/share/gnome-shell/extensions/$extension/"
install -m644 "$here/CHANGELOG.md" "$here/CHANGELOG.nb.md" "$here/CONTRIBUTORS.md" "$here/LICENSE" "$deb/usr/share/doc/agentic-screenshot/"
cat > "$deb/usr/share/applications/io.github.AgenticScreenshot.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Agentic Screenshot
GenericName=Screenshot
GenericName[nb]=Skjermbilde
GenericName[sv]=Skärmbild
GenericName[da]=Skærmbillede
GenericName[de]=Bildschirmfoto
GenericName[fr]=Capture d’écran
GenericName[es]=Captura de pantalla
GenericName[it]=Schermata
GenericName[pt_BR]=Captura de tela
GenericName[nl]=Schermafdruk
Comment=Screenshots for AI coding agents
Comment[nb]=Skjermbilder til AI-kodeagenter
Comment[sv]=Skärmbilder till AI-kodagenter
Comment[da]=Skærmbilleder til AI-kodeagenter
Comment[de]=Bildschirmfotos für KI-Coding-Agenten
Comment[fr]=Captures d’écran pour les agents de code IA
Comment[es]=Capturas de pantalla para agentes de código con IA
Comment[it]=Schermate per agenti di programmazione IA
Comment[pt_BR]=Capturas de tela para agentes de código com IA
Comment[nl]=Schermafdrukken voor AI-codeeragents
Exec=/usr/bin/agentic-screenshot
Icon=io.github.AgenticScreenshot
Terminal=false
Categories=Utility;
Keywords=screenshot;agentic;klipp;agent;skjermbilde;skärmbild;skærmbillede;bildschirmfoto;capture;captura;schermata;schermafdruk;
StartupNotify=true
StartupWMClass=io.github.AgenticScreenshot
Actions=capture;area;

[Desktop Action capture]
Name=Take Screenshot
Name[nb]=Ta skjermbilde
Name[sv]=Ta skärmbild
Name[da]=Tag skærmbillede
Name[de]=Bildschirmfoto aufnehmen
Name[fr]=Prendre une capture d’écran
Name[es]=Hacer una captura de pantalla
Name[it]=Acquisisci schermata
Name[pt_BR]=Fazer uma captura de tela
Name[nl]=Schermafdruk maken
Exec=/usr/bin/agentic-screenshot --capture

[Desktop Action area]
Name=Capture Area
Name[nb]=Ta bilde av område
Name[sv]=Ta skärmbild av område
Name[da]=Tag skærmbillede af område
Name[de]=Bereich aufnehmen
Name[fr]=Capturer une zone
Name[es]=Capturar un área
Name[it]=Acquisisci un’area
Name[pt_BR]=Capturar uma área
Name[nl]=Gebied vastleggen
Exec=/usr/bin/agentic-screenshot --capture --mode omrade
DESKTOP
cat > "$deb/DEBIAN/control" <<CONTROL
Package: agentic-screenshot
Version: $version+$build
Section: utils
Priority: optional
Architecture: amd64
Maintainer: bejulian1 <350748356+bejulian1@users.noreply.github.com>
Depends: libgtk-4-1 (>= 4.22), libadwaita-1-0 (>= 1.9), libglib2.0-0t64 (>= 2.80), libc6 (>= 2.39), libgcc-s1, libpango-1.0-0, libpangocairo-1.0-0, libcairo2, libgdk-pixbuf-2.0-0
Conflicts: agentic-klipp
Replaces: agentic-klipp
Breaks: agentic-klipp
Homepage: https://github.com/bejulian1/agentic-screenshot
Description: Screenshot tool for Ubuntu and GNOME (formerly Agentic Klipp)
 Screenshots, shelf, and annotation tools for AI coding agents.
 Built for Ubuntu 26.04, GNOME 50, and Wayland. Replaces the agentic-klipp
 package; settings and the shelf move over the first time the app starts.
CONTROL
deb_name=agentic-screenshot_${version}+${build}_amd64.deb
dpkg-deb --root-owner-group --build "$deb" "$output/$deb_name"
(cd -- "$output" && sha256sum agentic-screenshot "$name.tar.gz" "$deb_name" > SHA256SUMS)
printf 'Pakket %s\n%s\n' "$info" "$output"
