#!/bin/bash
set -euo pipefail

# Praefectus installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ennio-datatide/praefectus/main/install.sh | bash

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${BOLD}${GREEN}[✓]${NC} $1"; }
warn()  { echo -e "${BOLD}${YELLOW}[!]${NC} $1"; }
error() { echo -e "${BOLD}${RED}[✗]${NC} $1"; exit 1; }

echo ""
echo -e "${BOLD}Praefectus Installer${NC}"
echo "━━━━━━━━━━━━━━━━━━━━"
echo ""

# Detect OS
OS="$(uname -s)"
case "$OS" in
  Darwin) OS_NAME="macOS" ;;
  Linux)  OS_NAME="Linux" ;;
  *)      error "Unsupported OS: $OS. Praefectus supports macOS and Linux." ;;
esac
info "Detected $OS_NAME ($(uname -m))"

# Check Node.js 22+
if command -v node &>/dev/null; then
  NODE_VERSION=$(node -v | sed 's/v//' | cut -d. -f1)
  if [ "$NODE_VERSION" -ge 22 ]; then
    info "Node.js $(node -v) found"
  else
    warn "Node.js $(node -v) found, but v22+ is required"
    if [ "$OS_NAME" = "macOS" ]; then
      echo "  Install with: brew install node@22"
    else
      echo "  Install with: https://nodejs.org/en/download/"
    fi
    exit 1
  fi
else
  error "Node.js not found. Install Node.js 22+ first.\n  macOS: brew install node@22\n  Linux: https://nodejs.org/en/download/"
fi

# Check npm
if ! command -v npm &>/dev/null; then
  error "npm not found. It should come with Node.js."
fi
info "npm $(npm -v) found"

# Check tmux
if command -v tmux &>/dev/null; then
  info "tmux found"
else
  warn "tmux not found (required for agent sessions)"
  if [ "$OS_NAME" = "macOS" ]; then
    echo "  Install with: brew install tmux"
  else
    echo "  Install with: sudo apt install tmux  (or your package manager)"
  fi
  exit 1
fi

# Check git
if command -v git &>/dev/null; then
  info "git found"
else
  error "git not found. Install git first."
fi

# Check Claude Code CLI (optional)
if command -v claude &>/dev/null; then
  info "Claude Code CLI found"
else
  warn "Claude Code CLI not found (required for agent spawning)"
  echo "  Install: curl -fsSL https://claude.ai/install.sh | bash"
  echo "  Continuing installation anyway..."
fi

# Install praefectus
echo ""
info "Installing praefectus via npm..."
npm install -g praefectus

# Verify
if command -v praefectus &>/dev/null; then
  echo ""
  info "praefectus $(praefectus --version) installed successfully!"
  echo ""
  echo "Get started:"
  echo "  praefectus up        # Start the server and dashboard"
  echo "  praefectus start     # Spawn an agent session"
  echo "  praefectus ls        # List active sessions"
  echo ""
else
  error "Installation failed. Try: npm install -g praefectus"
fi
