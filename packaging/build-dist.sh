#!/usr/bin/env bash
# Builds and packs one MyChess release package into dist/.
#
#   packaging/build-dist.sh linux      # builds dist/MyChess-<ver>-linux-x86_64.tar.gz
#   packaging/build-dist.sh windows    # builds dist/MyChess-<ver>-windows-x86_64.zip (cross-compiled)
#   packaging/build-dist.sh sums       # rewrites dist/SHA256SUMS.txt
#   packaging/build-dist.sh stockfish19 DIR  # optional 81 MB package in packaging/work/ (not in dist/)
#
# One target per run (the link with fat LTO needs a lot of RAM). See
# packaging/README.md. Environment variables:
#   VERSION            package version (default: 0.1.0-alpha)
#   DATA_DIR           folder with Capablanca.*, Carlsen.* and Data Base/openings/
#                      (default: "$HOME/chess base"; never modified)
#   SF_DIR             folder with the Stockfish 10 files for the Linux package
#                      (default: "$DATA_DIR/Engines/Stockfish")
#   NAJDORF_PGN        the opening PGN to ship (default: SicilianNajdorf6Bg5.pgn)
#   CARGO_BUILD_JOBS   cargo parallelism (default 3; use 1 if the linker is OOM-killed)
#   CARGO_INCREMENTAL  forced to 0 here
#   CARGO_TARGET_DIR   default target/rel (linux) or target/rel-win (windows)
#   SKIP_BUILD=1       reuse the binary already in CARGO_TARGET_DIR
#   WIN_TARGET         Windows triple (default x86_64-pc-windows-gnu)
#   NATIVE_WINDOWS=1   build natively on Windows (MSVC, used by the release
#                      workflow) instead of cross-compiling with cargo-zigbuild
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${VERSION:-0.1.0-alpha}"
DATA_DIR="${DATA_DIR:-$HOME/chess base}"
SF_DIR="${SF_DIR:-$DATA_DIR/Engines/Stockfish}"
NAJDORF_PGN="${NAJDORF_PGN:-SicilianNajdorf6Bg5.pgn}"
WIN_TARGET="${WIN_TARGET:-x86_64-pc-windows-gnu}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}"
export CARGO_INCREMENTAL=0
WORK="$ROOT/packaging/work"
DIST="$ROOT/dist"
TPL="$ROOT/packaging/templates"

die() { echo "build-dist: $*" >&2; exit 1; }

# Stockfish 10 for Linux (official, unmodified; GPLv3 source offer included).
stage_stockfish() { # stage_stockfish <dest-dir>
  local dest="$1" sf="$SF_DIR"
  for f in stockfish_10_x64_modern_official stockfish-10-src.zip Readme.md compileinfo.txt; do
    [ -f "$sf/$f" ] || die "Stockfish file not found: $sf/$f (set SF_DIR)"
  done
  mkdir -p "$dest"
  cp "$sf/stockfish_10_x64_modern_official" "$sf/stockfish-10-src.zip" "$sf/Readme.md" "$sf/compileinfo.txt" "$dest/"
  chmod 755 "$dest/stockfish_10_x64_modern_official"
  python3 -I - "$sf/stockfish-10-src.zip" "$dest/Copying.txt" <<'PY'
import sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
with open(sys.argv[2], "wb") as out:
    out.write(z.read("Copying.txt"))
PY
  cp "$TPL/Stockfish-LEIA-ME.txt" "$dest/LEIA-ME.txt"
}

stage() { # stage <os-label> <exe-path> <exe-name> <triple>
  local label="$1" exe="$2" exe_name="$3" triple="$4"
  local name="MyChess-$VERSION-$label"
  local dir="$WORK/stage-$label/$name"
  [ -f "$exe" ] || die "binary not found: $exe"
  [ -f "$DATA_DIR/Capablanca.si4" ] || die "DATA_DIR does not contain Capablanca.si4: $DATA_DIR"
  local najdorf="$DATA_DIR/Data Base/openings/$NAJDORF_PGN"
  [ -f "$najdorf" ] || die "opening PGN not found: $najdorf"

  rm -rf "$WORK/stage-$label"
  mkdir -p "$dir/MyChess/Databases" "$dir/MyChess/Engine" "$dir/MyChess/Books"
  cp "$exe" "$dir/$exe_name"
  chmod 755 "$dir/$exe_name"
  : > "$dir/portable.flag"
  for b in Capablanca Carlsen; do
    for ext in si4 sg4 sn4; do
      cp "$DATA_DIR/$b.$ext" "$dir/MyChess/Databases/"
    done
  done
  cp "$najdorf" "$dir/MyChess/Databases/"
  cp "$TPL/Databases-LEIA-ME.txt" "$dir/MyChess/Databases/LEIA-ME.txt"
  cp "$TPL/Engine-LEIA-ME.txt" "$dir/MyChess/Engine/LEIA-ME.txt"
  cp "$TPL/Books-LEIA-ME.txt" "$dir/MyChess/Books/LEIA-ME.txt"
  cp "$TPL/LEIA-ME.txt" "$TPL/CREDITS.md" "$dir/"
  cp "$ROOT/LICENSE" "$dir/LICENSE"
  cp "$ROOT/assets/LICENSES.md" "$dir/LICENSES-ASSETS.md"
  if [ "$label" = linux-x86_64 ]; then
    stage_stockfish "$dir/MyChess/Engine/stockfish"
  fi
  python3 -I "$ROOT/packaging/third_party.py" "$triple" "$dir/LICENSES-THIRD-PARTY.txt"
  echo "$WORK/stage-$label"
}

sums() {
  (
    cd "$DIST"
    shopt -s nullglob
    files=(MyChess-*.tar.gz MyChess-*.zip)
    sha256sum "${files[@]}" > SHA256SUMS.txt
    cat SHA256SUMS.txt
  )
}

mkdir -p "$DIST" "$WORK"
case "${1:-}" in
  linux)
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/rel}"
    [ "${SKIP_BUILD:-0}" = 1 ] || cargo build --locked --profile dist -p mychess-gui
    stagedir="$(stage linux-x86_64 "$CARGO_TARGET_DIR/dist/mychess" mychess x86_64-unknown-linux-gnu | tail -1)"
    out="$DIST/MyChess-$VERSION-linux-x86_64.tar.gz"
    python3 -I "$ROOT/packaging/pack.py" tar "$stagedir" "$out"
    ;;
  windows)
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/rel-win}"
    if [ "${NATIVE_WINDOWS:-0}" = 1 ]; then
      WIN_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
      exe="$CARGO_TARGET_DIR/dist/mychess.exe"
      [ "${SKIP_BUILD:-0}" = 1 ] || cargo build --locked --profile dist -p mychess-gui
    else
      exe="$CARGO_TARGET_DIR/$WIN_TARGET/dist/mychess.exe"
      if [ "${SKIP_BUILD:-0}" != 1 ]; then
        rustup target add "$WIN_TARGET"
        cargo zigbuild --locked --profile dist --target "$WIN_TARGET" -p mychess-gui
      fi
    fi
    stagedir="$(stage windows-x86_64 "$exe" mychess.exe "$WIN_TARGET" | tail -1)"
    out="$DIST/MyChess-$VERSION-windows-x86_64.zip"
    python3 -I "$ROOT/packaging/pack.py" zip "$stagedir" "$out"
    ;;
  stockfish19)
    # Optional, NOT part of dist/ (the 81 MB archive would blow the 100 MB
    # limit of GitHub with the other packages): packaging/build-dist.sh
    # stockfish19 /path/to/stockfish-linux-x86-64-universal/stockfish
    sf19="${2:?usage: $0 stockfish19 <folder with the Stockfish 19 files>}"
    bin="$sf19/stockfish-linux-x86-64-universal"
    [ -f "$bin" ] || die "binary not found: $bin"
    d="$WORK/stage-sf19/MyChess/Engine/stockfish"
    rm -rf "$WORK/stage-sf19"; mkdir -p "$d"
    cp "$bin" "$d/stockfish-linux-x86-64-universal"; chmod 755 "$d/stockfish-linux-x86-64-universal"
    for f in Copying.txt AUTHORS README.md; do cp "$sf19/$f" "$d/"; done
    cp -r "$sf19/src" "$d/src"
    printf 'Stockfish 19 oficial, nao modificado, GPL v3. Codigo-fonte em src/.\nSHA-256 do binario: %s\n' "$(sha256sum "$bin" | cut -d' ' -f1)" > "$d/LEIA-ME.txt"
    chmod 755 "$WORK/stage-sf19/MyChess" 2>/dev/null || true
    # EXEC_NAMES of pack.py only knows the Stockfish 10 build, so use tar directly.
    out="$WORK/Stockfish-19-linux-x86_64.tar.gz"
    tar -C "$WORK/stage-sf19" --sort=name --owner=0 --group=0 -czf "$out" MyChess
    ls -l "$out"
    exit 0
    ;;
  sums)
    sums
    exit 0
    ;;
  *)
    die "usage: $0 linux|windows|sums|stockfish19 <dir>"
    ;;
esac
ls -l "$out"
sums
