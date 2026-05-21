name="debian-upgrade"
pkgname="debian-upgrade"
pkgver="0.1.0"
pkgrel="1"
pkgdesc="Assistant GUI + moteur backend pour mises a niveau majeures Debian"
arch=("amd64")
url="https://github.com/Xdavius/debian-upgrade"
license=("MIT")
maintainer="Debian Upgrade Team"

depends=(
  "apt"
  "ca-certificates"
  "libnotify-bin"
  "policykit-1"
  "sudo"
  "systemd"
)

makedepends=(
  "build-essential"
  "cargo"
  "rustc"
)

source=("git+https://github.com/Xdavius/debian-upgrade.git#branch=dev")

build() {
  cd "${srcdir}/debian-upgrade" || exit 1
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
  install -Dm644 "packaging/assets/systemd/debian-upgrade-offline.service" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-offline.service"
  install -Dm644 "packaging/assets/systemd/debian-upgrade-notify.service" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-notify.service"
  install -Dm644 "packaging/assets/systemd/debian-upgrade-notify.timer" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-notify.timer"
  install -d "${pkgdir}/usr/lib/systemd/system/timers.target.wants"
  ln -s ../debian-upgrade-notify.timer \
    "${pkgdir}/usr/lib/systemd/system/timers.target.wants/debian-upgrade-notify.timer"
  install -Dm644 "packaging/assets/desktop/debian-upgrade.desktop" \
    "${pkgdir}/usr/share/applications/debian-upgrade.desktop"
}

post_install() {
  if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
    systemctl enable --now debian-upgrade-notify.timer || true
  fi
}
