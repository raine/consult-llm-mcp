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
Darwin:arm64 | Darwin:aarch64)
	exec "$DIR/consult-llm-mcp-darwin-arm64" "$@"
	;;
Darwin:x86_64)
	exec "$DIR/consult-llm-mcp-darwin-x64" "$@"
	;;
Linux:x86_64 | Linux:amd64)
	exec "$DIR/consult-llm-mcp-linux-x64" "$@"
	;;
Linux:aarch64 | Linux:arm64)
	exec "$DIR/consult-llm-mcp-linux-arm64" "$@"
	;;
*)
	echo "consult-llm-mcp: unsupported platform: $OS $ARCH" >&2
	exit 1
	;;
esac
