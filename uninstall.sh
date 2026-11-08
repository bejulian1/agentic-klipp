#!/usr/bin/env bash
# Fjerner det install.sh la inn. Rører ikke skjermbildene dine, innstillingene
# dine eller GNOME-oppsettet.
set -euo pipefail

app_id=io.github.AgenticKlipp
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}
config_dir=${XDG_CONFIG_HOME:-$HOME/.config}
hicolor=$data_dir/icons/hicolor
desktop=$data_dir/applications/$app_id.desktop

rm -rf -- "$data_dir/gnome-shell/extensions/agentic-klipp@klipp.local"
rm -f -- "$HOME/.local/bin/klipp" "$desktop" \
    "$hicolor/scalable/apps/$app_id.svg" \
    "$hicolor/symbolic/apps/$app_id-symbolic.svg"

# Var dette de siste ikonene, skal heller ikke bufferen bli liggende.
if [ -d "$hicolor" ] && [ -z "$(find "$hicolor" -type f ! -name icon-theme.cache -print -quit)" ]; then
    rm -f -- "$hicolor/icon-theme.cache"
    find "$hicolor" -depth -type d -empty -delete
elif [ -f "$hicolor/icon-theme.cache" ]; then
    if command -v gtk-update-icon-cache >/dev/null; then
        gtk-update-icon-cache --quiet --ignore-theme-index --force "$hicolor" || true
    elif command -v gtk4-update-icon-cache >/dev/null; then
        gtk4-update-icon-cache --quiet --ignore-theme-index --force "$hicolor" || true
    fi
fi
if [ -d "$data_dir/applications" ] && command -v update-desktop-database >/dev/null; then
    update-desktop-database --quiet "$data_dir/applications" || true
fi

echo "Agentic Klipp er fjernet."
echo "Skjermbildene og innstillingene er ikke rørt: $config_dir/klipp/"
if [ -e "$config_dir/autostart/$app_id.desktop" ]; then
    echo "Autostart finnes fortsatt. Fjern den med:"
    printf '  rm %q\n' "$config_dir/autostart/$app_id.desktop"
fi
