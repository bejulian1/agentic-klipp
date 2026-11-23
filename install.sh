#!/usr/bin/env bash
# Installerer Agentic Screenshot for denne brukeren under ~/.local: binæren, ikoner,
# .desktop-fil og Shell-utvidelsen, og starter appen. En ny installasjon endrer
# ingen GNOME-innstillinger, så utvidelsen er av til du slår den på. Ved
# oppgradering fra Agentic Klipp fjernes den gamle installasjonen, og var den
# gamle utvidelsen på, slås den nye på i stedet.
#
# Bruk: ./install.sh [--bin STI] [--no-start]
#
#   (ingen valg)  Sjekker pakkene, bygger med cargo og installerer.
#   --bin STI     Installerer en ferdig bygd binær, for eksempel fra GitHub
#                 Releases, uten å bygge noe.
#   --no-start    Lar være å starte (eller starte på nytt) appen til slutt.
#
# Bygget havner i $CARGO_TARGET_DIR, eller target/ ved siden av dette skriptet.
set -euo pipefail

app_id=io.github.AgenticScreenshot
extension=agentic-screenshot@bejulian1.github.io
name="Agentic Screenshot"
# Prosessnavnet er de 15 første tegnene i binærnavnet, slik pgrep ser det.
process=agentic-screens
# Navnene fra Agentic Klipp 1.x, som ryddes bort ved oppgradering.
old_app_id=io.github.AgenticKlipp
old_extension=agentic-klipp@klipp.local
old_name="Agentic Klipp"
old_process=klipp
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
        sed -n '2,/^[^#]/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
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
        echo "Disse pakkene mangler for å bygge $name:" >&2
        echo >&2
        echo "  sudo apt install ${missing[*]}" >&2
        echo >&2
        echo "Kjør kommandoen over, og så ./install.sh igjen." >&2
        exit 1
    fi

    target_dir=${CARGO_TARGET_DIR:-$here/target}
    echo "Bygger $name. Første gang tar det noen minutter ..."
    (cd "$here" && CARGO_TARGET_DIR=$target_dir cargo build --release --locked)
    bin_src=$target_dir/release/agentic-screenshot
fi
if [ ! -f "$bin_src" ] || [ ! -r "$bin_src" ]; then
    echo "Fant ikke en lesbar binær: $bin_src" >&2
    exit 1
fi

bin_dir=$HOME/.local/bin
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}
config_dir=${XDG_CONFIG_HOME:-$HOME/.config}
hicolor=$data_dir/icons/hicolor
binary=$bin_dir/agentic-screenshot
desktop=$data_dir/applications/$app_id.desktop
extension_dir=$data_dir/gnome-shell/extensions/$extension
autostart=$config_dir/autostart/$app_id.desktop
old_binary=$bin_dir/klipp
old_desktop=$data_dir/applications/$old_app_id.desktop
old_extension_dir=$data_dir/gnome-shell/extensions/$old_extension
old_autostart=$config_dir/autostart/$old_app_id.desktop

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

# Listene over påslåtte og avslåtte utvidelser i GNOME Shell. UUID-ene her har
# verken anførselstegn eller komma, så GVariant-teksten kan endres direkte.
shell_settings() { command -v gsettings >/dev/null && gsettings list-keys org.gnome.shell >/dev/null 2>&1; }
shell_list_has() {
    local list
    list=$(gsettings get org.gnome.shell "$1" 2>/dev/null) || return 1
    case $list in
    *"'$2'"*) return 0 ;;
    *) return 1 ;;
    esac
}
shell_list_add() {
    local list
    list=$(gsettings get org.gnome.shell "$1") || return 1
    case $list in
    *"'$2'"*) return 0 ;;
    "@as []" | "[]") list="['$2']" ;;
    *) list="${list%]}, '$2']" ;;
    esac
    gsettings set org.gnome.shell "$1" "$list"
}
shell_list_remove() {
    local list
    list=$(gsettings get org.gnome.shell "$1") || return 1
    case $list in
    *"'$2'"*) ;;
    *) return 0 ;;
    esac
    list=${list//", '$2'"/}
    list=${list//"'$2', "/}
    list=${list//"'$2'"/}
    [ "$list" != "[]" ] || list="@as []"
    gsettings set org.gnome.shell "$1" "$list"
}

# Autostart fra Agentic Klipp får nytt navn og starter den nye binæren. Resten av
# fila, for eksempel X-GNOME-Autostart-enabled, beholdes.
move_autostart() {
    local line value program args
    while IFS= read -r line || [ -n "$line" ]; do
        case $line in
        Exec=*)
            value=${line#Exec=}
            case $value in
            \"*) args=${value#\"*\"} ;;
            *) args=${value#"${value%% *}"} ;;
            esac
            program=${value%"$args"}
            program=${program#\"}
            program=${program%\"}
            if [ "${program##*/}" = "$old_process" ]; then
                line="Exec=$exec_line$args"
            fi
            ;;
        *)
            line=${line//"$old_app_id"/"$app_id"}
            line=${line//"$old_name"/"$name"}
            ;;
        esac
        printf '%s\n' "$line"
    done <"$old_autostart" >"$autostart.tmp"
    mv -f "$autostart.tmp" "$autostart"
    rm -f -- "$old_autostart"
}

# Oppgradering fra Agentic Klipp: stopp den gamle appen og fjern filene den
# installerte. Innstillinger, hylle og bilder flytter appen selv ved første start.
upgrading=0
if [ -e "$old_binary" ] || [ -e "$old_desktop" ] || [ -e "$old_extension_dir" ] || [ -e "$old_autostart" ]; then
    upgrading=1
fi
old_enabled=0
if [ "$upgrading" = 1 ]; then
    echo "Erstatter $old_name med $name ..."
    if pgrep -u "$(id -u)" -x "$old_process" >/dev/null; then
        if [ -x "$old_binary" ]; then
            timeout 5 "$old_binary" --quit >/dev/null 2>&1 || true
        fi
        for _ in 1 2 3 4 5 6 7 8 9 10; do
            pgrep -u "$(id -u)" -x "$old_process" >/dev/null || break
            sleep 0.3
        done
        if pgrep -u "$(id -u)" -x "$old_process" >/dev/null; then
            pkill -TERM -u "$(id -u)" -x "$old_process" || true
            sleep 1
        fi
    fi
    if shell_settings && shell_list_has enabled-extensions "$old_extension"; then
        old_enabled=1
        # GNOME slår av den gamle utvidelsen med en gang, så Alt+K blir ledig.
        shell_list_remove enabled-extensions "$old_extension" || true
    fi
    rm -rf -- "$old_extension_dir"
    rm -f -- "$old_binary" "$old_desktop" \
        "$hicolor/scalable/apps/$old_app_id.svg" \
        "$hicolor/symbolic/apps/$old_app_id-symbolic.svg"
fi

# Bytt binæren atomisk, så en app som kjører, ikke krasjer midt i kopieringen.
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
Name=$name
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
Exec=$exec_line
Icon=$app_id
Terminal=false
Categories=Utility;
Keywords=screenshot;agentic;klipp;agent;skjermbilde;skärmbild;skærmbillede;bildschirmfoto;capture;captura;schermata;schermafdruk;
StartupNotify=true
StartupWMClass=$app_id
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
Exec=$exec_line --capture

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
Exec=$exec_line --capture --mode omrade
DESKTOP
mv -f "$desktop.tmp" "$desktop"

if [ -f "$old_autostart" ] && [ ! -e "$autostart" ]; then
    move_autostart
fi
rm -f -- "$old_autostart"
if [ "$old_enabled" = 1 ]; then
    shell_list_remove disabled-extensions "$extension" || true
    shell_list_add enabled-extensions "$extension" || true
fi

if command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache --quiet --ignore-theme-index --force "$hicolor" || true
elif command -v gtk4-update-icon-cache >/dev/null; then
    gtk4-update-icon-cache --quiet --ignore-theme-index --force "$hicolor" || true
fi
if command -v update-desktop-database >/dev/null; then
    update-desktop-database --quiet "$(dirname -- "$desktop")" || true
fi

echo "$name er installert:"
echo "  $binary"
echo "  $desktop"
echo "  $hicolor/scalable/apps/$app_id.svg"
echo "  $hicolor/symbolic/apps/$app_id-symbolic.svg"
echo "  $extension_dir"
if [ -e "$autostart" ]; then
    echo "  $autostart"
fi
if [ "$upgrading" = 1 ]; then
    echo
    echo "$old_name er fjernet. Innstillingene, hylla og bildene dine blir med:"
    echo "appen flytter dem selv første gang $name starter."
    echo
    echo "VIKTIG: Logg ut og inn én gang. GNOME på Wayland laster den omdøpte"
    echo "utvidelsen først ved neste innlogging. Til da tar GNOME bildene med blits."
    if [ "$old_enabled" = 0 ]; then
        echo "Slå på utvidelsen under Innstillinger i $name før du logger ut."
    fi
elif ! shell_settings || ! shell_list_has enabled-extensions "$extension"; then
    echo
    echo "Utvidelsen fjerner blitsen og gir Alt+K i alle apper. Trykk Slå på under"
    echo "Innstillinger i $name, og logg ut og inn én gang."
fi
case ":$PATH:" in
*":$bin_dir:"*) ;;
*) echo "Merk: $bin_dir er ikke i PATH, så «agentic-screenshot» virker bare med full sti." ;;
esac

# Start appen, eller start den på nytt hvis en eldre versjon kjører.
if [ "$start" = 1 ] && [ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]; then
    if pgrep -u "$(id -u)" -x "$process" >/dev/null; then
        timeout 5 "$binary" --quit 2>/dev/null || true
        for _ in 1 2 3 4 5 6 7 8 9 10; do
            pgrep -u "$(id -u)" -x "$process" >/dev/null || break
            sleep 0.3
        done
    fi
    if command -v gio >/dev/null; then
        gio launch "$desktop" >/dev/null 2>&1 || setsid "$binary" >/dev/null 2>&1 < /dev/null &
    else
        setsid "$binary" >/dev/null 2>&1 < /dev/null &
    fi
    echo
    echo "$name er startet: $("$binary" --version)"
fi
