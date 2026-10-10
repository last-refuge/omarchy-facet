#!/usr/bin/env bash
# Facet installer. Know your machine.
#
#   curl -fsSL https://raw.githubusercontent.com/last-refuge/omarchy-facet/master/install.sh | bash
#
# Arch Linux and Omarchy: installs a package (prebuilt when a release exists, built from source otherwise).
# Any other Linux: builds from source into ~/.local, no administrator access needed.
# Read it first if you like; it is short and does nothing you cannot see on the screen.
#
#   --source        build from source even if a prebuilt package is available
#   --prefix DIR    install into DIR without administrator access (default for non-Arch: ~/.local)
#   --yes           do not ask for confirmation
#   --dry-run       show what would happen and change nothing
#   --bar / --no-bar  add (or skip) the Omarchy bar widget; by default Omarchy users are asked
#   --uninstall     remove Facet
#   --help          this text
set -Eeuo pipefail

REPO="last-refuge/omarchy-facet"
RELEASE_URL="https://github.com/$REPO/releases/latest/download"
CLONE_URL="https://github.com/$REPO.git"
PKG_ASSET="facet-x86_64.pkg.tar.zst"

# ---------------------------------------------------------------------------------------------
# Presentation
# ---------------------------------------------------------------------------------------------
if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
  TTY=1
  if [[ "${COLORTERM:-}" == *truecolor* || "${COLORTERM:-}" == *24bit* ]]; then
    ACCENT=$'\033[38;2;209;69;45m'; SAGE=$'\033[38;2;120;145;124m'
  else
    ACCENT=$'\033[31m'; SAGE=$'\033[32m'
  fi
  BOLD=$'\033[1m'; DIM=$'\033[2m'; RESET=$'\033[0m'; HIDE=$'\033[?25l'; SHOW=$'\033[?25h'
else
  TTY=0; ACCENT=''; SAGE=''; BOLD=''; DIM=''; RESET=''; HIDE=''; SHOW=''
fi

banner() {
  printf '\n  %s%sfacet%s  %s·  Know your machine.%s\n\n' "$ACCENT" "$BOLD" "$RESET" "$DIM" "$RESET"
}
ok()   { printf '  %s✔%s  %s\n' "$SAGE" "$RESET" "$*"; }
note() { printf '  %s·%s  %s%s%s\n' "$DIM" "$RESET" "$DIM" "$*" "$RESET"; }
warn() { printf '  %s!%s  %s\n' "$ACCENT" "$RESET" "$*"; }
die()  { printf '\n  %s✘%s  %s\n' "$ACCENT" "$RESET" "$*" >&2; [[ -n "${LOG:-}" && -s "${LOG:-}" ]] && printf '     %sFull log: %s%s\n' "$DIM" "$LOG" "$RESET" >&2; printf '\n' >&2; exit 1; }

cleanup() { printf '%s' "$SHOW"; [[ -n "${WORK:-}" && -d "$WORK" ]] && rm -rf "$WORK"; return 0; }
trap cleanup EXIT
trap 'die "Interrupted."' INT TERM

# Run a command quietly with a spinner; its output goes to the log and is shown only if it fails.
step() {
  local msg=$1; shift
  if (( DRY )); then note "would: $msg"; return 0; fi
  if (( ! TTY )); then
    printf '  …  %s\n' "$msg"
    "$@" >>"$LOG" 2>&1 || { tail -n 12 "$LOG" | sed 's/^/     /' >&2; die "$msg failed."; }
    ok "$msg"
    return 0
  fi
  "$@" >>"$LOG" 2>&1 &
  local pid=$! i=0 frames='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
  printf '%s' "$HIDE"
  while kill -0 "$pid" 2>/dev/null; do
    printf '\r  %s%s%s  %s' "$ACCENT" "${frames:i++%${#frames}:1}" "$RESET" "$msg"
    sleep 0.1
  done
  printf '%s\r\033[K' "$SHOW"
  if wait "$pid"; then ok "$msg"; else printf '\r\033[K'; tail -n 12 "$LOG" | sed 's/^/     /' >&2; die "$msg failed."; fi
}

# ---------------------------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------------------------
FORCE_SOURCE=0; PREFIX=""; ASSUME_YES=0; DRY=0; UNINSTALL=0; BAR=ask
while (( $# )); do
  case "$1" in
    --source) FORCE_SOURCE=1 ;;
    --prefix) shift; [[ $# -gt 0 ]] || die "--prefix needs a directory."; PREFIX=$1 ;;
    --prefix=*) PREFIX=${1#--prefix=} ;;
    --yes|-y) ASSUME_YES=1 ;;
    --dry-run) DRY=1 ;;
    --bar) BAR=yes ;;
    --no-bar) BAR=no ;;
    --uninstall) UNINSTALL=1 ;;
    --help|-h) sed -n '2,17p' "${BASH_SOURCE[0]:-$0}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "Unknown option $1 (try --help)." ;;
  esac
  shift
done
WORK=$(mktemp -d "${TMPDIR:-/tmp}/facet-install.XXXXXX")
LOG="$WORK/install.log"

# ---------------------------------------------------------------------------------------------
# What kind of machine is this?
# ---------------------------------------------------------------------------------------------
IS_ARCH=0; IS_OMARCHY=0; DISTRO="this Linux system"
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  DISTRO=${PRETTY_NAME:-${NAME:-$DISTRO}}
  [[ "${ID:-}" == arch || "${ID_LIKE:-}" == *arch* ]] && IS_ARCH=1
fi
if [[ -d /usr/share/omarchy ]] || command -v omarchy >/dev/null 2>&1; then IS_OMARCHY=1; IS_ARCH=1; fi

sudo_note() {
  note "Installing a package needs administrator access, so you may be asked for your password."
}

confirm() {
  (( ASSUME_YES )) && return 0
  [[ -r /dev/tty ]] || return 0
  local reply; printf '  %s?%s  %s [Y/n] ' "$ACCENT" "$RESET" "$1" >/dev/tty; read -r reply </dev/tty || true
  [[ -z "$reply" || "$reply" =~ ^[Yy] ]]
}

# ---------------------------------------------------------------------------------------------
# Uninstall
# ---------------------------------------------------------------------------------------------
if (( UNINSTALL )); then
  banner
  if [[ -n "$PREFIX" || ( $IS_ARCH == 0 && -x "$HOME/.local/bin/facet-app" ) ]]; then
    P=${PREFIX:-$HOME/.local}
    if (( DRY )); then note "would remove Facet from $P"; else
      rm -f "$P/bin/facet" "$P/bin/facet-app" "$P/share/applications/dev.facet.Facet.desktop" "$P/share/icons/hicolor/scalable/apps/dev.facet.Facet.svg"
      rm -rf "$P/share/doc/facet" "$P/share/licenses/facet"
      ok "Removed Facet from $P"
    fi
  elif pacman -Q facet >/dev/null 2>&1; then
    sudo_note
    if (( DRY )); then note "would run: sudo pacman -Rns facet"; else sudo pacman -Rns $( (( ASSUME_YES )) && echo --noconfirm ) facet; ok "Removed the facet package"; fi
  else
    note "Facet is not installed."
  fi
  PLUGIN_DIR="$HOME/.config/omarchy/plugins/facet.hardware"
  if [[ -e "$PLUGIN_DIR" || -L "$PLUGIN_DIR" ]] && command -v omarchy >/dev/null 2>&1; then
    if (( DRY )); then note "would remove the Facet bar widget"; else omarchy plugin remove facet.hardware --yes >/dev/null 2>&1 || rm -rf "$PLUGIN_DIR"; ok "Removed the Facet bar widget"; fi
  fi
  STATE="${XDG_STATE_HOME:-$HOME/.local/state}/facet"
  # Settings are the user's data: only ever removed after an explicit, interactive yes (never by --yes).
  if [[ -d "$STATE" ]] && (( ! DRY )); then
    if (( ASSUME_YES )) || [[ ! -r /dev/tty ]]; then
      note "Kept your remembered settings in $STATE (delete that folder to remove them)."
    else
      reply=""; printf '  %s?%s  Also remove Facet'"'"'s remembered settings (%s)? [y/N] ' "$ACCENT" "$RESET" "$STATE" >/dev/tty; read -r reply </dev/tty || true
      if [[ "$reply" =~ ^[Yy] ]]; then rm -rf "$STATE"; ok "Removed settings"; else note "Kept your settings."; fi
    fi
  fi
  printf '\n'; exit 0
fi

# ---------------------------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------------------------
banner
if (( IS_OMARCHY )); then ok "Omarchy detected"; elif (( IS_ARCH )); then ok "$DISTRO detected"; else ok "$DISTRO detected"; fi
[[ "$(uname -m)" == x86_64 ]] || warn "Facet is only tested on x86_64; continuing from source."
(( DRY )) && note "Dry run: nothing will be changed."

compgen -G "/usr/share/vulkan/icd.d/*.json" >/dev/null || warn "No Vulkan driver found. Facet draws with Vulkan; install your GPU's driver (for example vulkan-intel, vulkan-radeon or nvidia-utils)."

# Where is the source? Inside a checkout we use it; otherwise we clone.
SRC=""
SELF_DIR=$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || true)
if [[ -n "$SELF_DIR" && -f "$SELF_DIR/Cargo.toml" && -d "$SELF_DIR/crates/app" ]]; then SRC=$SELF_DIR; fi

need_source() {
  [[ -n "$SRC" ]] && return 0
  command -v git >/dev/null 2>&1 || die "git is needed to fetch the source. Install it and run this again."
  step "Fetching the source" git clone --depth 1 ${FACET_REF:+--branch "$FACET_REF"} "$CLONE_URL" "$WORK/src"
  SRC="$WORK/src"
}

have_rust() { command -v cargo >/dev/null 2>&1; }

# ---- Arch / Omarchy: a real package ------------------------------------------------------------
install_arch() {
  local pkg=""
  if (( ! FORCE_SOURCE )) && [[ "$(uname -m)" == x86_64 ]]; then
    if (( DRY )); then
      if curl -fsIL --retry 1 -o /dev/null "$RELEASE_URL/$PKG_ASSET" 2>/dev/null; then
        note "would download the prebuilt package and verify its checksum"; pkg=dry
      else
        note "no prebuilt release exists yet, so it would build from source"
      fi
    elif curl -fsSL --retry 2 -o "$WORK/$PKG_ASSET" "$RELEASE_URL/$PKG_ASSET" 2>>"$LOG" \
         && curl -fsSL --retry 2 -o "$WORK/$PKG_ASSET.sha256" "$RELEASE_URL/$PKG_ASSET.sha256" 2>>"$LOG"; then
      ( cd "$WORK" && sha256sum -c "$PKG_ASSET.sha256" >>"$LOG" 2>&1 ) || die "The downloaded package failed its checksum. Nothing was installed."
      ok "Downloaded the latest release and verified its checksum"
      pkg="$WORK/$PKG_ASSET"
    else
      note "No prebuilt release yet; building from source instead (this takes a few minutes)."
    fi
  fi

  if [[ -z "$pkg" ]]; then
    local missing=()
    for t in git clang cmake pkg-config fakeroot make; do command -v "$t" >/dev/null 2>&1 || missing+=("$t"); done
    local pacman_pkgs=()
    for t in "${missing[@]}"; do case "$t" in pkg-config) pacman_pkgs+=(pkgconf) ;; fakeroot|make) pacman_pkgs+=(base-devel) ;; *) pacman_pkgs+=("$t") ;; esac; done
    have_rust || pacman_pkgs+=(rust)
    if (( ${#pacman_pkgs[@]} )); then
      note "Building needs: ${pacman_pkgs[*]}"
      sudo_note
      if (( DRY )); then note "would run: sudo pacman -S --needed ${pacman_pkgs[*]}"; else
        sudo pacman -S --needed $( (( ASSUME_YES )) && echo --noconfirm ) "${pacman_pkgs[@]}" || die "Could not install the build tools."
      fi
    fi
    need_source
    if (( DRY )); then note "would build the package with scripts/build-package.sh"; else
      step "Building Facet (a few minutes the first time)" bash -c "cd '$SRC' && scripts/build-package.sh"
      pkg=$(ls -1 "$SRC"/target/package/facet-*.pkg.tar.zst 2>/dev/null | head -n1) || true
      [[ -n "$pkg" ]] || die "The build finished but produced no package."
    fi
  fi

  sudo_note
  if (( DRY )); then note "would run: sudo pacman -U <package>"; return 0; fi
  sudo pacman -U $( (( ASSUME_YES )) && echo --noconfirm ) "$pkg" || die "pacman could not install the package."
  ok "Installed"
}

# ---- Everything else: build from source into a prefix, no administrator access ----------------------
install_prefix() {
  local p=${PREFIX:-$HOME/.local}
  have_rust || die "Rust is needed to build Facet. Install it from https://rustup.rs and run this again."
  for t in git clang cmake pkg-config make; do command -v "$t" >/dev/null 2>&1 || die "'$t' is needed to build Facet. Install it with your package manager and run this again."; done
  if ! pkg-config --exists wayland-client xkbcommon fontconfig freetype2 2>/dev/null; then
    warn "Some development libraries are missing. Install them first:"
    printf '\n     %sDebian / Ubuntu%s  sudo apt install build-essential git clang cmake pkg-config libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev libfontconfig-dev libfreetype-dev libvulkan1 mesa-vulkan-drivers libssl-dev libasound2-dev libdbus-1-dev\n' "$BOLD" "$RESET"
    printf '     %sFedora%s           sudo dnf install git clang cmake pkgconf-pkg-config wayland-devel libxkbcommon-devel libxkbcommon-x11-devel libxcb-devel fontconfig-devel freetype-devel vulkan-loader mesa-vulkan-drivers openssl-devel alsa-lib-devel dbus-devel\n\n' "$BOLD" "$RESET"
    die "Install those, then run this again."
  fi
  need_source
  step "Building Facet (a few minutes the first time)" make -C "$SRC" build
  step "Installing into $p" make -C "$SRC" install PREFIX="$p"
  case ":$PATH:" in *":$p/bin:"*) ;; *) warn "$p/bin is not on your PATH; add it to run 'facet' and 'facet-app' by name." ;; esac
}

if (( IS_ARCH )) && [[ -z "$PREFIX" ]]; then install_arch; else install_prefix; fi

# ---- Omarchy bar widget (optional) ----------------------------------------------------------------
# Plugins are plain folders in ~/.config/omarchy/plugins; Omarchy documents copy, rescan, enable as the
# supported by-hand route. It edits the user's shell config, so it is always opt-in.
install_bar_widget() {
  (( IS_OMARCHY )) && command -v omarchy >/dev/null 2>&1 || return 0
  [[ "$BAR" == no ]] && return 0
  local src_dir="" d
  for d in /usr/share/facet/omarchy-plugin "${PREFIX:-$HOME/.local}/share/facet/omarchy-plugin" "$SRC/omarchy-plugin"; do
    [[ -n "$d" && -f "$d/manifest.json" ]] && { src_dir=$d; break; }
  done
  [[ -n "$src_dir" ]] || return 0
  if [[ "$BAR" == ask ]]; then
    (( ASSUME_YES )) && return 0   # --yes never edits shell config on its own; use --bar
    (( DRY )) && { note "would offer to add the Facet bar widget"; return 0; }
    confirm "Add a Facet widget to your Omarchy bar (live CPU temperature and a hardware popup)?" || return 0
  fi
  if (( DRY )); then note "would add the Facet bar widget to your Omarchy bar"; return 0; fi
  local target="$HOME/.config/omarchy/plugins/facet.hardware"
  mkdir -p "$HOME/.config/omarchy/plugins"
  rm -rf "$target"; cp -r "$src_dir" "$target"
  if omarchy-shell shell rescanPlugins >>"$LOG" 2>&1 && omarchy plugin enable facet.hardware >>"$LOG" 2>&1; then
    ok "Added the Facet widget to your bar (right-click it to open Facet)"
  else
    warn "Copied the widget, but could not enable it. Run: omarchy plugin enable facet.hardware"
  fi
}
if (( ! DRY )) || [[ "$BAR" != no ]]; then install_bar_widget; fi

# ---- Check it works --------------------------------------------------------------------------------
if (( ! DRY )); then
  BIN=facet; [[ -n "$PREFIX" ]] && BIN="$PREFIX/bin/facet"; (( ! IS_ARCH )) && [[ -z "$PREFIX" ]] && BIN="$HOME/.local/bin/facet"
  if OUT=$("$BIN" --diagnose 2>/dev/null); then
    VER=$("$BIN" --version); N=$(grep -c ' components ' <<<"$OUT" || true)
    ok "Checked: $VER reads this machine ($(grep -E '^  total' <<<"$OUT" | awk '{print $2" ms"}'))"
    if grep -q 'runtime suspended' <<<"$OUT"; then note "Your discrete GPU is asleep and will stay that way."; fi
  else
    warn "Installed, but 'facet --diagnose' did not run. Try it yourself to see why."
  fi
fi

if (( DRY )); then printf '\n'; exit 0; fi
printf '\n  %sOpen it%s\n' "$BOLD" "$RESET"
if [[ -n "$PREFIX" ]]; then printf '     run %s%s/bin/facet-app%s\n' "$BOLD" "$PREFIX" "$RESET"
elif (( IS_OMARCHY )); then printf '     %sSuper + Space%s, then type "Facet"     or run %sfacet-app%s\n' "$BOLD" "$RESET" "$BOLD" "$RESET"
else printf '     find "Facet" in your application launcher     or run %sfacet-app%s\n' "$BOLD" "$RESET"; fi
printf '  %sOr in the terminal%s\n     %sfacet%s            a readable report\n     %sfacet --diagnose%s what Facet can read on this machine\n' "$BOLD" "$RESET" "$BOLD" "$RESET" "$BOLD" "$RESET"
if [[ -n "$PREFIX" ]]; then printf '  %sRemove it any time%s\n     %s./install.sh --uninstall --prefix %s%s\n\n' "$BOLD" "$RESET" "$DIM" "$PREFIX" "$RESET"
elif (( IS_ARCH )); then printf '  %sRemove it any time%s\n     %ssudo pacman -Rns facet%s\n\n' "$BOLD" "$RESET" "$DIM" "$RESET"
else printf '  %sRemove it any time%s\n     %s./install.sh --uninstall%s\n\n' "$BOLD" "$RESET" "$DIM" "$RESET"; fi
