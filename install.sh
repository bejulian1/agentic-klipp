#!/usr/bin/env bash
# Installerer Agentic Klipp for denne brukeren under ~/.local: binæren, ikoner,
# .desktop-fil og Shell-utvidelsen, og starter appen. Endrer ingen
# GNOME-innstillinger, så utvidelsen er av til du slår den på.
#
# Bruk: ./install.sh [--bin STI] [--no-start]
#
#   (ingen valg)  Sjekker pakkene, bygger med cargo og installerer.
#   --bin STI     Installerer en ferdig bygd binær, for eksempel fra GitHub
#                 Releases, uten å bygge noe.
#   --no-start    Lar være å starte (eller starte på nytt) Klipp til slutt.
#
# Bygget havner i $CARGO_TARGET_DIR, eller target/ ved siden av dette skriptet.
set -euo pipefail

app_id=io.github.AgenticKlipp
extension=agentic-klipp@klipp.local
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin_src=
start=1

while [ $# -gt 0 ]; do
    case "$1" in
    --bin)
        [ $# -ge 2 ] || { echo "--bin trenger en sti" >&2; exit 2; }
        bin_src=$2
        shift 2
        ;;
    --no-start)
        start=0
        shift
        ;;
    -h | --help)
        sed -n '2,14s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
        exit 0
        ;;
    *)
        echo "Ukjent argument: $1" >&2
        exit 2
        ;;
    esac
done

# Samler opp apt-pakker som mangler, så alt kan installeres med én kommando.
missing=()
need_cmd() { command -v "$1" >/dev/null || missing+=("$2"); }
need_pc() { pkg-config --exists "$1" 2>/dev/null || missing+=("$2"); }

if [ -z "$bin_src" ]; then
    need_cmd cargo cargo
    need_cmd rustc rustc
    need_cmd mold mold
    need_cmd pkg-config pkgconf
    need_cmd glib-compile-resources libglib2.0-dev-bin
    if command -v pkg-config >/dev/null; then
        need_pc 'gtk4 >= 4.22' libgtk-4-dev
        need_pc 'libadwaita-1 >= 1.9' libadwaita-1-dev
    else
        missing+=(libgtk-4-dev libadwaita-1-dev)
    fi
    if [ ${#missing[@]} -gt 0 ]; then
        echo "Disse pakkene mangler for å bygge Klipp:" >&2
        echo >&2
        echo "  sudo apt install ${missing[*]}" >&2
        echo >&2
        echo "Kjør kommandoen over, og så ./install.sh igjen." >&2
        exit 1
    fi

    target_dir=${CARGO_TARGET_DIR:-$here/target}
    echo "Bygger Klipp. Første gang tar det noen minutter ..."
    (cd "$here" && CARGO_TARGET_DIR=$target_dir cargo build --release --locked)
    bin_src=$target_dir/release/klipp
fi
if [ ! -f "$bin_src" ] || [ ! -r "$bin_src" ]; then
    echo "Fant ikke en lesbar binær: $bin_src" >&2
    exit 1
fi

bin_dir=$HOME/.local/bin
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}
hicolor=$data_dir/icons/hicolor
binary=$bin_dir/klipp
desktop=$data_dir/applications/$app_id.desktop
extension_dir=$data_dir/gnome-shell/extensions/$extension

case "$binary" in
*$'\n'*)
    echo "Stien inneholder linjeskift, det støttes ikke i .desktop-filer: $binary" >&2
    exit 1
    ;;
esac

# Verdi i Exec= med anførselstegn, se Desktop Entry-spesifikasjonen. Mellomrom
# er trygge i anførselstegnene. \ " ` $ må beskyttes to ganger, og % én gang.
desktop_exec() {
    local s=$1
    s=${s//\\/\\\\\\\\}
    s=${s//\"/\\\\\"}
    s=${s//\`/\\\\\`}
    s=${s//\$/\\\\\$}
    s=${s//%/%%}
    printf '"%s"' "$s"
}

exec_line=$(desktop_exec "$binary")

# Bytt binæren atomisk, så en Klipp som kjører, ikke krasjer midt i kopieringen.
install -Dm755 "$bin_src" "$binary.tmp"
mv -f "$binary.tmp" "$binary"
install -Dm644 "$here/data/icons/hicolor/scalable/apps/$app_id.svg" \
    "$hicolor/scalable/apps/$app_id.svg"
install -Dm644 "$here/data/icons/hicolor/symbolic/apps/$app_id-symbolic.svg" \
    "$hicolor/symbolic/apps/$app_id-symbolic.svg"
install -Dm644 -t "$extension_dir" \
    "$here/shell-extension/$extension/metadata.json" \
    "$here/shell-extension/$extension/extension.js"

mkdir -p "$(dirname -- "$desktop")"
cat >"$desktop.tmp" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Agentic Klipp
GenericName=Skjermbilde
Comment=Skjermbilder til AI-kodeagenter
Exec=$exec_line
Icon=$app_id
Terminal=false
Categories=Utility;
Keywords=skjermbilde;screenshot;klipp;agent;
StartupNotify=true
StartupWMClass=$app_id
Actions=capture;area;

[Desktop Action capture]
Name=Ta skjermbilde
Exec=$exec_line --capture

[Desktop Action area]
Name=Ta bilde av område
Exec=$exec_line --capture --mode omrade
DESKTOP
mv -f "$desktop.tmp" "$desktop"

if command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache --quiet --ignore-theme-index --force "$hicolor" || true
elif command -v gtk4-update-icon-cache >/dev/null; then
    gtk4-update-icon-cache --quiet --ignore-theme-index --force "$hicolor" || true
fi
if command -v update-desktop-database >/dev/null; then
    update-desktop-database --quiet "$(dirname -- "$desktop")" || true
fi

echo "Agentic Klipp er installert:"
echo "  $binary"
echo "  $desktop"
echo "  $hicolor/scalable/apps/$app_id.svg"
echo "  $hicolor/symbolic/apps/$app_id-symbolic.svg"
echo "  $extension_dir"
if ! gnome-extensions list --enabled 2>/dev/null | grep -qx "$extension"; then
    echo
    echo "Utvidelsen fjerner blitsen og gir Alt+K i alle apper. Trykk Slå på under"
    echo "Innstillinger i Klipp, og logg ut og inn én gang."
fi
case ":$PATH:" in
*":$bin_dir:"*) ;;
*) echo "Merk: $bin_dir er ikke i PATH, så «klipp» virker bare med full sti." ;;
esac

# Start Klipp, eller start den på nytt hvis en eldre versjon kjører.
if [ "$start" = 1 ] && [ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]; then
    if pgrep -u "$(id -u)" -x klipp >/dev/null; then
        "$binary" --quit 2>/dev/null || true
        for _ in 1 2 3 4 5 6 7 8 9 10; do
            pgrep -u "$(id -u)" -x klipp >/dev/null || break
            sleep 0.3
        done
    fi
    if command -v gio >/dev/null; then
        gio launch "$desktop" >/dev/null 2>&1 || setsid "$binary" >/dev/null 2>&1 < /dev/null &
    else
        setsid "$binary" >/dev/null 2>&1 < /dev/null &
    fi
    echo
    echo "Klipp er startet: $("$binary" --version)"
fi
