#!/usr/bin/env bash
#
# consult-llm-mcp installation script
# Usage: curl -fsSL https://raw.githubusercontent.com/raine/consult-llm-mcp/main/scripts/install.sh | bash
#
# Environment variables:
#   CONSULT_LLM_MCP_VERSION      - Pin a specific version (e.g., v2.5.6)
#   CONSULT_LLM_MCP_INSTALL_DIR  - Override install directory (default: /usr/local/bin or ~/.local/bin)
#

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
	echo -e "${BLUE}==>${NC} $1"
}

log_success() {
	echo -e "${GREEN}==>${NC} $1"
}

log_warning() {
	echo -e "${YELLOW}==>${NC} $1"
}

log_error() {
	echo -e "${RED}Error:${NC} $1" >&2
}

detect_platform() {
	local os arch

	case "$(uname -s)" in
	Darwin)
		os="darwin"
		;;
	Linux)
		os="linux"
		;;
	*)
		log_error "Unsupported operating system: $(uname -s)"
		echo ""
		echo "consult-llm-mcp supports macOS and Linux."
		echo ""
		exit 1
		;;
	esac

	case "$(uname -m)" in
	x86_64 | amd64)
		arch="x64"
		;;
	aarch64 | arm64)
		arch="arm64"
		;;
	*)
		log_error "Unsupported architecture: $(uname -m)"
		echo ""
		echo "consult-llm-mcp prebuilt binaries are available for x64 and arm64."
		echo ""
		exit 1
		;;
	esac

	echo "${os}-${arch}"
}

install_from_release() {
	log_info "Installing consult-llm-mcp from GitHub releases..."

	local platform=$1
	local tmp_dir
	tmp_dir=$(mktemp -d)
	trap 'rm -rf "$tmp_dir"' EXIT

	local version="${CONSULT_LLM_MCP_VERSION:-}"

	if [ -z "$version" ]; then
		log_info "Fetching latest release..."
		local latest_url="https://api.github.com/repos/raine/consult-llm-mcp/releases/latest"
		local release_json

		if command -v curl &>/dev/null; then
			release_json=$(curl -fsSL --retry 3 --retry-connrefused --connect-timeout 10 --max-time 30 "$latest_url")
		elif command -v wget &>/dev/null; then
			release_json=$(wget --tries=3 --timeout=30 -qO- "$latest_url")
		else
			log_error "Neither curl nor wget found. Please install one of them."
			exit 1
		fi

		version=$(echo "$release_json" | grep '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/')

		if [ -z "$version" ]; then
			log_error "Failed to fetch latest version"
			echo ""
			echo "This might be due to network issues or GitHub API rate limits."
			echo "You can specify a version manually:"
			echo "  CONSULT_LLM_MCP_VERSION=v2.5.6 bash install.sh"
			echo ""
			exit 1
		fi
	fi

	log_info "Installing version: $version"

	local archive_name="consult-llm-mcp-${platform}.tar.gz"
	local download_url="https://github.com/raine/consult-llm-mcp/releases/download/${version}/${archive_name}"

	log_info "Downloading $archive_name..."

	cd "$tmp_dir"
	if command -v curl &>/dev/null; then
		if ! curl -fsSL --retry 3 --retry-connrefused --connect-timeout 10 --max-time 120 -o "$archive_name" "$download_url"; then
			log_error "Download failed"
			echo ""
			echo "The release may not have a prebuilt binary for your platform ($platform)."
			echo ""
			cd - >/dev/null || cd "$HOME"
			exit 1
		fi
	elif command -v wget &>/dev/null; then
		if ! wget --tries=3 --timeout=120 -q -O "$archive_name" "$download_url"; then
			log_error "Download failed"
			echo ""
			echo "The release may not have a prebuilt binary for your platform ($platform)."
			echo ""
			cd - >/dev/null || cd "$HOME"
			exit 1
		fi
	fi

	log_info "Verifying checksum..."
	local checksum_file="consult-llm-mcp-${platform}.sha256"
	local checksum_url="https://github.com/raine/consult-llm-mcp/releases/download/${version}/${checksum_file}"

	if command -v curl &>/dev/null; then
		if ! curl -fsSL --retry 3 --retry-connrefused --connect-timeout 10 --max-time 30 -o "$checksum_file" "$checksum_url"; then
			log_error "Failed to download checksum file"
			cd - >/dev/null || cd "$HOME"
			exit 1
		fi
	elif command -v wget &>/dev/null; then
		if ! wget --tries=3 --timeout=30 -q -O "$checksum_file" "$checksum_url"; then
			log_error "Failed to download checksum file"
			cd - >/dev/null || cd "$HOME"
			exit 1
		fi
	fi

	if command -v sha256sum &>/dev/null; then
		if ! sha256sum -c "$checksum_file" &>/dev/null; then
			log_error "Checksum verification failed"
			cd - >/dev/null || cd "$HOME"
			exit 1
		fi
	elif command -v shasum &>/dev/null; then
		if ! shasum -a 256 -c "$checksum_file" &>/dev/null; then
			log_error "Checksum verification failed"
			cd - >/dev/null || cd "$HOME"
			exit 1
		fi
	else
		log_warning "Neither sha256sum nor shasum found, skipping checksum verification"
	fi

	log_success "Checksum verified"

	log_info "Extracting archive..."
	if ! tar -xzf "$archive_name"; then
		log_error "Failed to extract archive"
		exit 1
	fi

	local install_dir="${CONSULT_LLM_MCP_INSTALL_DIR:-}"
	if [ -z "$install_dir" ]; then
		if [[ -w /usr/local/bin ]]; then
			install_dir="/usr/local/bin"
		else
			install_dir="$HOME/.local/bin"
		fi
	fi
	mkdir -p "$install_dir"

	if [ -f "$install_dir/consult-llm-mcp" ]; then
		local existing_version
		existing_version=$("$install_dir/consult-llm-mcp" --version 2>/dev/null || echo "unknown")
		log_info "Existing installation found: $existing_version"
		log_info "Upgrading to: $version"
	fi

	log_info "Installing to $install_dir..."
	local tmp_binary="$install_dir/consult-llm-mcp.tmp.$$"

	if [[ -w "$install_dir" ]]; then
		cp consult-llm-mcp "$tmp_binary"
		chmod +x "$tmp_binary"
		mv -f "$tmp_binary" "$install_dir/consult-llm-mcp"
	else
		if ! sudo cp consult-llm-mcp "$tmp_binary"; then
			log_error "Failed to install to $install_dir (sudo required)"
			exit 1
		fi
		sudo chmod +x "$tmp_binary"
		sudo mv -f "$tmp_binary" "$install_dir/consult-llm-mcp"
	fi

	# Remove macOS quarantine attribute if present
	if [[ "$(uname -s)" == "Darwin" ]] && command -v xattr &>/dev/null; then
		xattr -d com.apple.quarantine "$install_dir/consult-llm-mcp" 2>/dev/null || true
	fi

	log_success "consult-llm-mcp installed to $install_dir/consult-llm-mcp"

	if [[ ":$PATH:" != *":$install_dir:"* ]]; then
		log_warning "$install_dir is not in your PATH"
		echo ""
		echo "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
		echo "  export PATH=\"\$PATH:$install_dir\""
		echo ""
	fi

	cd - >/dev/null || cd "$HOME"

	INSTALL_DIR="$install_dir"
}

verify_installation() {
	local install_dir="$1"

	if [ ! -x "$install_dir/consult-llm-mcp" ]; then
		log_error "consult-llm-mcp binary not found or not executable at $install_dir/consult-llm-mcp"
		exit 1
	fi

	if ! "$install_dir/consult-llm-mcp" --version &>/dev/null; then
		log_error "consult-llm-mcp binary exists but failed to run"
		exit 1
	fi

	log_success "consult-llm-mcp is installed and ready!"
	echo ""
	"$install_dir/consult-llm-mcp" --version
	echo ""

	echo "Add to Claude Code:"
	echo "  claude mcp add consult-llm \\"
	echo "    -e GEMINI_API_KEY=your_key \\"
	echo "    -- $install_dir/consult-llm-mcp"
	echo ""
}

main() {
	echo ""
	echo "consult-llm-mcp installer"
	echo ""

	log_info "Detecting platform..."
	local platform
	platform=$(detect_platform)
	log_info "Platform: $platform"

	install_from_release "$platform"
	verify_installation "$INSTALL_DIR"
}

main "$@"
