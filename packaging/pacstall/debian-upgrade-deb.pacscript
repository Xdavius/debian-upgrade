name="debian-upgrade-deb"
pkgname="debian-upgrade"
pkgver="0.1.0"
pkgrel="1"
pkgdesc="Assistant GUI + moteur backend pour mises a niveau majeures Debian"
arch=("amd64")
url="https://example.com/debian-upgrade"
license=("MIT")
maintainer="Debian Upgrade Team"

depends=(
  "apt"
  "ca-certificates"
  "policykit-1"
  "sudo"
  "systemd"
  "zenity"
)

makedepends=(
  "build-essential"
  "cargo"
  "rustc"
)

source=("git+https://example.com/debian-upgrade.git")

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
  install -Dm644 "packaging/assets/systemd/debian-upgrade-offline.service" \
    "${pkgdir}/usr/lib/systemd/system/debian-upgrade-offline.service"
  install -Dm644 "packaging/assets/desktop/debian-upgrade.desktop" \
    "${pkgdir}/usr/share/applications/debian-upgrade.desktop"
}
