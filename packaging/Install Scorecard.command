#!/bin/sh
# Install Scorecard.command — double-clickable installer for the
# Scorecard command-line tools (`sc` and `sc-mcp`).
#
# Installs to /usr/local/bin (asks for the password via sudo), or falls
# back to ~/.local/bin when admin rights are unavailable. Safe to rerun:
# it only overwrites those two files and prints the installed version.
# SCORECARD_DEST overrides the target directory (used for testing).
set -eu

src=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
dest="${SCORECARD_DEST:-/usr/local/bin}"
home_bin="$HOME/.local/bin"

for tool in sc sc-mcp; do
  if [ ! -f "$src/$tool" ]; then
    echo "error: $tool not found next to this script (expected in $src)" >&2
    exit 1
  fi
done

install_to() {
  dir=$1
  use_sudo=$2
  if [ "$use_sudo" = true ]; then
    sudo -p "Password for %u to install Scorecard to $dir: " mkdir -p "$dir" \
      && sudo cp "$src/sc" "$src/sc-mcp" "$dir/"
  else
    mkdir -p "$dir" && cp "$src/sc" "$src/sc-mcp" "$dir/"
  fi
}

if install_to "$dest" true 2>/dev/null; then
  echo "Installed sc and sc-mcp to $dest."
else
  echo "Could not write to $dest; installing to $home_bin instead." >&2
  dest="$home_bin"
  install_to "$dest" false || {
    echo "error: install to $dest failed" >&2
    exit 1
  }
  echo "Installed sc and sc-mcp to $dest."
fi

# Downloads carry a quarantine attribute that stops Gatekeeper from
# running the tools. Strip it from the installed copies, but only if
# it is actually there. Nothing else is touched.
for tool in sc sc-mcp; do
  if xattr -p com.apple.quarantine "$dest/$tool" >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$dest/$tool"
    echo "Removed quarantine attribute from $dest/$tool."
  fi
  chmod +x "$dest/$tool"
done

if [ "$dest" = "$home_bin" ]; then
  case ":$PATH:" in
    *":$home_bin:"*) ;;
    *)
      echo "Note: $home_bin is not on your PATH. Add this to ~/.zshrc" >&2
      echo "and restart the terminal:" >&2
      echo "  export PATH=\"\$HOME/.local/bin:\$PATH\"" >&2
      ;;
  esac
fi

"$dest/sc" --version
