#!/bin/sh
set -e

# Resolve symlinks to find the actual script directory (npm creates symlinks in .bin/)
# When invoked via PATH, $0 may be just the command name without a path
script="$0"
case "$script" in
*/*) ;;
*) script="$(command -v "$script" 2>/dev/null || echo "$script")" ;;
esac
while [ -L "$script" ]; do
	target="$(readlink "$script")"
	case "$target" in
	/*) script="$target" ;;
	*) script="$(dirname "$script")/$target" ;;
	esac
done
DIR="$(cd -P "$(dirname "$script")" && pwd)"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS:$ARCH" in
Darwin:arm64 | Darwin:aarch64) PLATFORM="darwin-arm64" ;;
Darwin:x86_64) PLATFORM="darwin-x64" ;;
Linux:x86_64 | Linux:amd64) PLATFORM="linux-x64" ;;
Linux:aarch64 | Linux:arm64) PLATFORM="linux-arm64" ;;
*)
	echo "consult-llm-mcp: unsupported platform: $OS $ARCH" >&2
	exit 1
	;;
esac

PKG_NAME="consult-llm-mcp-$PLATFORM"
BIN_NAME="consult-llm-mcp"

# Fast path: check common node_modules layouts
for candidate in \
	"$DIR/../$PKG_NAME/$BIN_NAME" \
	"$DIR/node_modules/$PKG_NAME/$BIN_NAME"; do
	if [ -f "$candidate" ]; then
		[ -x "$candidate" ] || chmod +x "$candidate" 2>/dev/null || true
		exec "$candidate" "$@"
	fi
done

# Fallback: use Node's require.resolve for non-standard layouts (pnpm, yarn pnp, etc.)
if command -v node >/dev/null 2>&1; then
	BIN="$(PKG_NAME="$PKG_NAME" BIN_NAME="$BIN_NAME" SEARCH_DIR="$DIR" node -e "
try {
  const p = require('path');
  const pkg = require.resolve(process.env.PKG_NAME + '/package.json', { paths: [process.env.SEARCH_DIR] });
  process.stdout.write(p.join(p.dirname(pkg), process.env.BIN_NAME));
} catch (e) {
  process.exit(1);
}
" 2>/dev/null)" && [ -f "$BIN" ] && {
		[ -x "$BIN" ] || chmod +x "$BIN" 2>/dev/null || true
		exec "$BIN" "$@"
	}
fi

echo "consult-llm-mcp: could not find platform binary package '$PKG_NAME'" >&2
echo "Ensure npm install was not run with --no-optional or --omit=optional." >&2
exit 1
