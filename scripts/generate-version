#!/usr/bin/env bash
set -euo pipefail
hash=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
cat > src/version.ts << EOF
export const GIT_HASH = "$hash"
EOF
