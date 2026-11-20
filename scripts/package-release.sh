#!/usr/bin/env bash
# Pakker en allerede bygd Linux x86_64-binær. Ingen tester eller byggemapper tas med.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
binary=${1:-${CARGO_TARGET_DIR:-$here/target}/release/klipp}
output=${2:-$here/dist}
[[ -x "$binary" ]] || { echo "Fant ikke kjørbar binær: $binary" >&2; exit 1; }
version=$(sed -n '/^\[package\]/,/^\[/s/^version = "\([^"]*\)"/\1/p' "$here/Cargo.toml")
info=$("$binary" --version)
[[ "$info" == "klipp $version (bygg "* ]] || { echo "Binær og Cargo.toml har ulik versjon" >&2; exit 1; }
build=${info##*bygg }
build=${build%)}
[[ $(uname -m) == x86_64 ]] || { echo "Pakkeskriptet støtter x86_64" >&2; exit 1; }
mkdir -p "$output"
output=$(cd -- "$output" && pwd)
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
name=agentic-klipp-$version-linux-x86_64
bundle=$stage/$name
mkdir -p "$bundle"
install -m755 "$binary" "$bundle/klipp"
install -m755 "$here/install.sh" "$here/uninstall.sh" "$bundle/"
cp -a "$here/data" "$here/shell-extension" "$bundle/"
cp -a "$here/docs" "$bundle/"
install -m644 "$here/README.md" "$here/CHANGELOG.md" "$here/CONTRIBUTORS.md" "$here/LICENSE" "$bundle/"
printf '%s\n' "$info" > "$bundle/BUILD.txt"
install -m755 "$binary" "$output/klipp"
tar -C "$stage" -czf "$output/$name.tar.gz" "$name"

deb=$stage/deb
install -Dm755 "$binary" "$deb/usr/bin/klipp"
install -Dm644 "$here/data/icons/hicolor/scalable/apps/io.github.AgenticKlipp.svg" "$deb/usr/share/icons/hicolor/scalable/apps/io.github.AgenticKlipp.svg"
install -Dm644 "$here/data/icons/hicolor/symbolic/apps/io.github.AgenticKlipp-symbolic.svg" "$deb/usr/share/icons/hicolor/symbolic/apps/io.github.AgenticKlipp-symbolic.svg"
mkdir -p "$deb/usr/share/gnome-shell/extensions/agentic-klipp@klipp.local" "$deb/usr/share/applications" "$deb/usr/share/doc/agentic-klipp" "$deb/DEBIAN"
install -m644 "$here/shell-extension/agentic-klipp@klipp.local/extension.js" "$here/shell-extension/agentic-klipp@klipp.local/metadata.json" "$deb/usr/share/gnome-shell/extensions/agentic-klipp@klipp.local/"
install -m644 "$here/CHANGELOG.md" "$here/CONTRIBUTORS.md" "$here/LICENSE" "$deb/usr/share/doc/agentic-klipp/"
cat > "$deb/usr/share/applications/io.github.AgenticKlipp.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Agentic Klipp
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
Exec=/usr/bin/klipp
Icon=io.github.AgenticKlipp
Terminal=false
Categories=Utility;
Keywords=screenshot;klipp;agent;skjermbilde;skärmbild;skærmbillede;bildschirmfoto;capture;captura;schermata;schermafdruk;
StartupNotify=true
StartupWMClass=io.github.AgenticKlipp
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
Exec=/usr/bin/klipp --capture

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
Exec=/usr/bin/klipp --capture --mode omrade
DESKTOP
cat > "$deb/DEBIAN/control" <<CONTROL
Package: agentic-klipp
Version: $version+$build
Section: utils
Priority: optional
Architecture: amd64
Maintainer: bejulian1 <350748356+bejulian1@users.noreply.github.com>
Depends: libgtk-4-1 (>= 4.22), libadwaita-1-0 (>= 1.9), libglib2.0-0t64 (>= 2.80), libc6 (>= 2.39), libgcc-s1, libpango-1.0-0, libpangocairo-1.0-0, libcairo2, libgdk-pixbuf-2.0-0
Homepage: https://github.com/bejulian1/agentic-klipp
Description: Screenshot tool for Ubuntu and GNOME
 Screenshots, shelf, and annotation tools for AI coding agents.
 Built for Ubuntu 26.04, GNOME 50, and Wayland.
CONTROL
dpkg-deb --root-owner-group --build "$deb" "$output/agentic-klipp_${version}+${build}_amd64.deb"
(cd -- "$output" && sha256sum klipp "$name.tar.gz" "agentic-klipp_${version}+${build}_amd64.deb" > SHA256SUMS)
printf 'Pakket %s\n%s\n' "$info" "$output"
