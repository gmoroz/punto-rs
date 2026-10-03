#!/bin/sh
set -eu
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary=${1:-"$source_dir/target/debug/punto-rs"}
destination=$(mktemp -d)
trap 'rm -rf "$destination"' EXIT HUP INT TERM
export HOME="$destination"
unset XDG_CONFIG_HOME
config="$destination/.config/punto-rs/config.conf"
sh "$source_dir/scripts/install.sh" "$binary"
# Без -c демон обязан найти конфиг пользователя в ~/.config.
"$destination/.local/bin/punto-rs" --check-config
printf '\n# Local setting retained on upgrade\n' >> "$config"
cp "$config" "$destination/expected.conf"
sh "$source_dir/scripts/install.sh" "$binary"
cmp "$destination/expected.conf" "$config"
cmp "$source_dir/config/punto-rs.conf" "$config.example"
cmp "$source_dir/systemd/punto-rs.service" "$destination/.config/systemd/user/punto-rs.service"
echo 'PASS: installation and update preserve local config'
