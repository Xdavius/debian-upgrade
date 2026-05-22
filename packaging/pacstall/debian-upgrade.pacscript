name="debian-upgrade"
pkgname="debian-upgrade"
pkgver="2.0.0"
pkgrel="1"
pkgdesc="Assistant GUI + moteur backend pour mises a niveau majeures Debian"
arch=("amd64")
url="https://github.com/Xdavius/debian-upgrade"
license=("MIT")
maintainer="Debian Upgrade Team"

depends=(
  "apt"
  "ca-certificates"
  "dbus-user-session"
  "libnotify-bin"
  "pkexec"
  "sudo"
  "systemd"
)

makedepends=(
  "build-essential"
  "cargo"
  "rustc"
)

source=("git+https://github.com/Xdavius/debian-upgrade.git")
external_connection="true"

build() {
  cd "${srcdir}/debian-upgrade" || exit 1

  min_rustc='1.85.0'
  rustc_version="$(rustc --version | awk '{print $2}')"
  if [ "$(printf '%s\n' "$min_rustc" "$rustc_version" | sort -V | head -n1)" != "$min_rustc" ]; then
    echo "Erreur: rustc $rustc_version detecte, version minimale requise: $min_rustc"
    exit 1
  fi

  min_cargo='1.85.0'
  cargo_version="$(cargo --version | awk '{print $2}')"
  if [ "$(printf '%s\n' "$min_cargo" "$cargo_version" | sort -V | head -n1)" != "$min_cargo" ]; then
    echo "Erreur: cargo $cargo_version detecte, version minimale requise: $min_cargo"
    exit 1
  fi

  cargo build --release -p frontend-gui -p backend-cli
}

package() {
  cd "${srcdir}/debian-upgrade" || exit 1

  install -Dm755 "target/release/debian-upgrade" \
    "${pkgdir}/usr/bin/debian-upgrade"
  install -Dm755 "target/release/debian-upgrade-backend" \
    "${pkgdir}/usr/libexec/debian-upgrade-backend"
  install -Dm755 "packaging/assets/bin/offline-upgrade.sh" \
    "${pkgdir}/usr/local/lib/debian-upgrade/offline-upgrade.sh"
  install -Dm755 "packaging/assets/bin/check-upgrade-notify.sh" \
    "${pkgdir}/usr/local/lib/debian-upgrade/check-upgrade-notify.sh"
  install -Dm755 "packaging/assets/bin/check-post-upgrade-notify.sh" \
    "${pkgdir}/usr/local/lib/debian-upgrade/check-post-upgrade-notify.sh"
  install -Dm644 "packaging/assets/systemd/debian-upgrade-offline.service" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-offline.service"
  install -Dm644 "packaging/assets/systemd/debian-upgrade-notify.service" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-notify.service"
  install -Dm644 "packaging/assets/systemd/debian-upgrade-notify.timer" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-notify.timer"
  install -Dm644 "packaging/assets/systemd/debian-upgrade-post-notify.service" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-post-notify.service"
  install -Dm644 "packaging/assets/systemd/debian-upgrade-post-notify.path" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-post-notify.path"
  install -d "${pkgdir}/usr/lib/systemd/system/timers.target.wants"
  ln -s ../debian-upgrade-notify.timer \
    "${pkgdir}/usr/lib/systemd/system/timers.target.wants/debian-upgrade-notify.timer"
  install -d "${pkgdir}/usr/lib/systemd/system/multi-user.target.wants"
  ln -s ../debian-upgrade-post-notify.path \
    "${pkgdir}/usr/lib/systemd/system/multi-user.target.wants/debian-upgrade-post-notify.path"
  install -Dm644 "packaging/assets/desktop/debian-upgrade.desktop" \
    "${pkgdir}/usr/share/applications/debian-upgrade.desktop"
}

post_install() {
  if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
    systemctl enable --now debian-upgrade-notify.timer || true
    systemctl enable --now debian-upgrade-post-notify.path || true
  fi
}
