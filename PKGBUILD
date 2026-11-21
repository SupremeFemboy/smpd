# Maintainer: user
pkgname=smpd
pkgver=0.1.0
pkgrel=1
pkgdesc="Hardware-accelerated MPD active track selector on Wayland Layer Shell (wgpu)"
arch=('x86_64' 'aarch64')
url="https://github.com/RustBlyat/smpd"
license=('MIT' 'Apache-2.0')
depends=('gcc-libs' 'vulkan-driver' 'fontconfig')
makedepends=('cargo')
source=()
options=(!debug)
sha256sums=()

prepare() {
    export RUSTUP_TOOLCHAIN=stable
}

build() {
    cd "$startdir"
    cargo build --release --locked
}

check() {
    cd "$startdir"
    cargo test --release --locked
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 "README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
}
