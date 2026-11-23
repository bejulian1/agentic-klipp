#!/usr/bin/env bash
# Fjerner det install.sh la inn, også en eldre installasjon av Agentic Klipp.
# Rører ikke skjermbildene dine, innstillingene dine eller GNOME-oppsettet.
set -euo pipefail

app_id=io.github.AgenticScreenshot
old_app_id=io.github.AgenticKlipp
data_dir=${XDG_DATA_HOME:-$HOME/.local/share}
config_dir=${XDG_CONFIG_HOME:-$HOME/.config}
hicolor=$data_dir/icons/hicolor

rm -rf -- "$data_dir/gnome-shell/extensions/agentic-screenshot@bejulian1.github.io" \
    "$data_dir/gnome-shell/extensions/agentic-klipp@klipp.local"
for id in "$app_id" "$old_app_id"; do
    rm -f -- "$data_dir/applications/$id.desktop" \
        "$hicolor/scalable/apps/$id.svg" \
        "$hicolor/symbolic/apps/$id-symbolic.svg"
done
rm -f -- "$HOME/.local/bin/agentic-screenshot" "$HOME/.local/bin/klipp"

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

echo "Agentic Screenshot er fjernet."
echo "Skjermbildene og innstillingene er ikke rørt: $config_dir/agentic-screenshot/"
if [ -d "$config_dir/klipp" ]; then
    echo "Innstillinger fra Agentic Klipp ligger fortsatt i $config_dir/klipp/"
fi
for id in "$app_id" "$old_app_id"; do
    if [ -e "$config_dir/autostart/$id.desktop" ]; then
        echo "Autostart finnes fortsatt. Fjern den med:"
        printf '  rm %q\n' "$config_dir/autostart/$id.desktop"
    fi
done
