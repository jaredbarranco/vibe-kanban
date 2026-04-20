#!/usr/bin/env bash
set -euo pipefail

# Build arm64 binaries for macOS, Linux, and Windows and package them for a
# GitHub releases distribution (no R2 / CI required).
#
# Usage:
#   ./scripts/build-arm64-release.sh --tag v0.1.42 --github-repo owner/repo
#
# Prerequisites by target:
#   macos-arm64   — nothing extra (rustup target already installed)
#   linux-arm64   — brew install zig && cargo install cargo-zigbuild
#   windows-arm64 — cargo install cargo-xwin  (+ brew install llvm for ring crate)
#
# Output: release/ directory ready to upload to a GitHub release as-is.

# ── arg parsing ───────────────────────────────────────────────────────────────

TAG=""
GITHUB_REPO=""
SKIP_FRONTEND=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag)          TAG="$2";          shift 2 ;;
    --github-repo)  GITHUB_REPO="$2";  shift 2 ;;
    --skip-frontend) SKIP_FRONTEND=true; shift ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

if [[ -z "$TAG" || -z "$GITHUB_REPO" ]]; then
  echo "Usage: $0 --tag <tag> --github-repo <owner/repo>"
  echo "  e.g. $0 --tag v0.1.42 --github-repo myuser/vibe-kanban"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RELEASE_DIR="$ROOT/release"
RUST_TOOLCHAIN="nightly-2025-12-04"
GH_DOWNLOAD_BASE="https://github.com/${GITHUB_REPO}/releases/download/${TAG}"

echo "==> Tag:         $TAG"
echo "==> Repo:        $GITHUB_REPO"
echo "==> Release dir: $RELEASE_DIR"
echo ""

# ── helpers ───────────────────────────────────────────────────────────────────

sha256_file() {
  if command -v sha256sum &>/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

require_tool() {
  if ! command -v "$1" &>/dev/null; then
    echo "ERROR: '$1' not found. $2"
    exit 1
  fi
}

# ── prerequisites ─────────────────────────────────────────────────────────────

require_tool cargo  "Install Rust: https://rustup.rs"
require_tool node   "Install Node.js >= 20"
require_tool pnpm   "Run: npm install -g pnpm"
require_tool zip    "Install zip (brew install zip)"

HAS_ZIG=false
HAS_ZIGBUILD=false
HAS_XWIN=false

command -v zig          &>/dev/null && HAS_ZIG=true
cargo zigbuild --version &>/dev/null 2>&1 && HAS_ZIGBUILD=true
cargo xwin     --version &>/dev/null 2>&1 && HAS_XWIN=true

echo "==> Tool availability:"
echo "    macos-arm64  : ✓ (ready)"
$HAS_ZIG       && echo "    linux-arm64  : zig found" || echo "    linux-arm64  : ✗ zig missing — run: brew install zig"
$HAS_ZIGBUILD  && echo "    linux-arm64  : cargo-zigbuild found" || echo "    linux-arm64  : ✗ cargo-zigbuild missing — run: cargo install cargo-zigbuild"
$HAS_XWIN      && echo "    windows-arm64: cargo-xwin found" || echo "    windows-arm64: ✗ cargo-xwin missing — run: cargo install cargo-xwin"
echo ""

# ── frontend ──────────────────────────────────────────────────────────────────

if [[ "$SKIP_FRONTEND" == "false" ]]; then
  if [[ -d "$ROOT/packages/local-web/dist" ]]; then
    echo "==> Frontend already built, skipping (pass --skip-frontend to suppress this check)"
  else
    echo "==> Building frontend..."
    cd "$ROOT"
    pnpm install
    cd packages/local-web
    npm run build
    cd "$ROOT"
  fi
fi

# ── rust target setup ─────────────────────────────────────────────────────────

rustup target add aarch64-apple-darwin   --toolchain "$RUST_TOOLCHAIN" 2>/dev/null || true
$HAS_ZIGBUILD && rustup target add aarch64-unknown-linux-musl --toolchain "$RUST_TOOLCHAIN" 2>/dev/null || true
$HAS_XWIN     && rustup target add aarch64-pc-windows-msvc    --toolchain "$RUST_TOOLCHAIN" 2>/dev/null || true

# ── build function ────────────────────────────────────────────────────────────

BINS=(server vibe-kanban-mcp review)

build_target() {
  local target="$1"
  local name="$2"
  echo ""
  echo "==> Building $name ($target)..."

  cd "$ROOT"

  if [[ "$target" == "aarch64-apple-darwin" ]]; then
    if [[ "$(uname -m)" == "arm64" ]]; then
      export MACOSX_DEPLOYMENT_TARGET=11.0
      cargo "+$RUST_TOOLCHAIN" build --release --target "$target" \
        -p server -p mcp -p review --bin server --bin vibe-kanban-mcp --bin review
    else
      echo "WARNING: Building aarch64-apple-darwin on Intel — cross-compile not configured, skipping."
      return
    fi

  elif [[ "$target" == "aarch64-unknown-linux-musl" ]]; then
    if ! $HAS_ZIG || ! $HAS_ZIGBUILD; then
      echo "  Skipping $name — zig/cargo-zigbuild not installed."
      return
    fi
    cargo "+$RUST_TOOLCHAIN" zigbuild --release --target "$target" \
      -p server -p mcp -p review --bin server --bin vibe-kanban-mcp --bin review

  elif [[ "$target" == "aarch64-pc-windows-msvc" ]]; then
    if ! $HAS_XWIN; then
      echo "  Skipping $name — cargo-xwin not installed."
      return
    fi
    # arm64 Windows: ring crate needs clang instead of clang-cl
    CLANG_BIN="$(command -v clang)"
    CLANG_CL_BIN=""
    # Prefer LLVM clang-cl (brew install llvm) over Apple's stub
    for p in /opt/homebrew/opt/llvm/bin /usr/local/opt/llvm/bin; do
      [[ -x "$p/clang-cl" ]] && CLANG_CL_BIN="$p/clang-cl" && break
    done
    if [[ -z "$CLANG_CL_BIN" ]]; then
      echo "  WARNING: clang-cl not found (brew install llvm). Trying without — may fail for ring crate."
    fi
    export RING_CC="$CLANG_BIN"
    export DEFAULT_CC="${CLANG_CL_BIN:-clang-cl}"
    export CC_aarch64_pc_windows_msvc="$SCRIPT_DIR/ring-cc-wrapper.sh"
    export CARGO_PROFILE_RELEASE_DEBUG=0
    cargo "+$RUST_TOOLCHAIN" xwin build --cross-compiler clang-cl --release --target "$target" \
      -p server -p mcp -p review --bin server --bin vibe-kanban-mcp --bin review
  fi

  # ── package into zips ──────────────────────────────────────────────────────

  echo "  Packaging $name..."
  mkdir -p "$RELEASE_DIR"

  local bin_dir="$ROOT/target/$target/release"
  local is_windows=false
  [[ "$target" == *windows* ]] && is_windows=true

  local binary_map=("server:vibe-kanban" "vibe-kanban-mcp:vibe-kanban-mcp" "review:vibe-kanban-review")
  for entry in "${binary_map[@]}"; do
    local built="${entry%%:*}"
    local zip_base="${entry##*:}"
    local src ext zip_name

    if $is_windows; then
      src="$bin_dir/${built}.exe"
      ext=".exe"
    else
      src="$bin_dir/$built"
      ext=""
    fi

    zip_name="${name}-${zip_base}.zip"

    if [[ ! -f "$src" ]]; then
      echo "  WARNING: expected binary not found: $src"
      continue
    fi

    # Zip with just the binary filename (no directory prefix)
    ( cd "$bin_dir" && zip -j "$RELEASE_DIR/$zip_name" "${built}${ext}" )
    echo "  Created $zip_name ($(du -sh "$RELEASE_DIR/$zip_name" | cut -f1))"
  done
}

# ── run builds ────────────────────────────────────────────────────────────────

build_target "aarch64-apple-darwin"    "macos-arm64"
build_target "aarch64-unknown-linux-musl" "linux-arm64"
build_target "aarch64-pc-windows-msvc"   "windows-arm64"

# ── generate manifest.json ────────────────────────────────────────────────────

echo ""
echo "==> Generating manifest.json..."

node -e "
  const fs = require('fs');
  const crypto = require('crypto');

  const releaseDir = '$RELEASE_DIR';
  const base = '$GH_DOWNLOAD_BASE';
  const platforms = ['macos-arm64', 'linux-arm64', 'windows-arm64'];
  const binaries = ['vibe-kanban', 'vibe-kanban-mcp', 'vibe-kanban-review'];

  const manifest = { platforms: {} };

  for (const platform of platforms) {
    for (const binary of binaries) {
      const zipName = platform + '-' + binary + '.zip';
      const zipPath = releaseDir + '/' + zipName;
      if (!fs.existsSync(zipPath)) continue;

      const data = fs.readFileSync(zipPath);
      manifest.platforms[platform] = manifest.platforms[platform] || {};
      manifest.platforms[platform][binary] = {
        url: base + '/' + zipName,
        sha256: crypto.createHash('sha256').update(data).digest('hex'),
        size: data.length,
      };
    }
  }

  fs.writeFileSync(releaseDir + '/manifest.json', JSON.stringify(manifest, null, 2));
  console.log(JSON.stringify(manifest, null, 2));
"

# ── build and pack npx CLI ────────────────────────────────────────────────────

echo ""
echo "==> Building npx CLI..."
cd "$ROOT/npx-cli"
npm ci
npm run build

# Inject GitHub releases URLs into the compiled bundle
MANIFEST_URL="${GH_DOWNLOAD_BASE}/manifest.json"
sed -i.bak "s|__BINARY_MANIFEST_URL__|${MANIFEST_URL}|g" bin/cli.js && rm bin/cli.js.bak
sed -i.bak "s|__BINARY_TAG__|${TAG}|g"                   bin/cli.js && rm bin/cli.js.bak
# Leave R2_BASE_URL as-is (unused when BINARY_MANIFEST_URL + binaryInfo.url are set)

echo "==> Packing npm tarball..."
npm pack
mv vibe-kanban-*.tgz "$RELEASE_DIR/"
# Also copy as a fixed "latest" filename so users can use a stable URL
cp "$RELEASE_DIR/vibe-kanban-${TAG#v}.tgz" "$RELEASE_DIR/vibe-kanban-latest.tgz" 2>/dev/null || \
  cp "$RELEASE_DIR"/vibe-kanban-*.tgz "$RELEASE_DIR/vibe-kanban-latest.tgz"

# ── summary ───────────────────────────────────────────────────────────────────

echo ""
echo "════════════════════════════════════════════════════════════"
echo " Release files ready in: $RELEASE_DIR"
echo "════════════════════════════════════════════════════════════"
ls -lh "$RELEASE_DIR"
echo ""
echo "Next steps:"
echo "  1. Create a GitHub release with tag: $TAG"
echo "     (GitHub UI: https://github.com/$GITHUB_REPO/releases/new?tag=$TAG)"
echo ""
echo "  2. Upload all files from $RELEASE_DIR to the release"
echo ""
echo "  3. Users always install the latest with:"
echo "     npx https://github.com/$GITHUB_REPO/releases/latest/download/vibe-kanban-latest.tgz"
