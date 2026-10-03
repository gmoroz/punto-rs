#!/bin/sh
set -eu

source_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary=${1:-"$source_dir/punto-rs"}
bin_dir="$HOME/.local/bin"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/punto-rs"
unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

if [ ! -f "$binary" ]; then
    echo "Binary not found: $binary" >&2
    exit 1
fi

install -d "$bin_dir" "$config_dir" "$unit_dir"
install -m644 "$source_dir/config/punto-rs.conf" "$config_dir/config.conf.example"
if [ ! -e "$config_dir/config.conf" ] && [ ! -L "$config_dir/config.conf" ]; then
    install -m644 "$source_dir/config/punto-rs.conf" "$config_dir/config.conf"
fi
install -m644 "$source_dir/systemd/punto-rs.service" "$unit_dir/punto-rs.service"
# Замена inode позволяет обновлять файл, пока предыдущий бинарник ещё выполняется.
temporary=$(mktemp -p "$bin_dir")
trap 'rm -f "$temporary"' EXIT HUP INT TERM
install -m755 "$binary" "$temporary"
mv -f "$temporary" "$bin_dir/punto-rs"
echo 'Files installed; existing config preserved. Service state has not been changed.'
