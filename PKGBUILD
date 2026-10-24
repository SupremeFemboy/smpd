# Maintainer: user
pkgname=smpd
pkgver=0.1.0
pkgrel=1
pkgdesc="Hardware-accelerated MPD active track selector with album art carousel (winit + wgpu)"
arch=('x86_64' 'aarch64')
url="https://github.com/RustBlyat/smpd"
license=('MIT' 'Apache-2.0')
depends=('gcc-libs' 'vulkan-driver' 'fontconfig')
makedepends=('cargo' 'rust')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
    export RUSTUP_TOOLCHAIN=stable
}

build() {
    cargo build --release --locked
}

check() {
    cargo test --release --locked
}

package() {
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 "README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
}
