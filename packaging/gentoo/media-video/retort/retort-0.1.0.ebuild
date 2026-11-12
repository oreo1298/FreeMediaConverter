# Copyright 2026 Retort contributors
# Distributed under the terms of the GNU General Public License v3

EAPI=8

RUST_MIN_VER="1.92.0"

inherit cargo gnome2-utils meson xdg

DESCRIPTION="Convert video and audio files with FFmpeg, using verified hardware acceleration"
HOMEPAGE="https://github.com/oreo1298/FreeMediaConverter"
SRC_URI="
	https://github.com/oreo1298/FreeMediaConverter/releases/download/v${PV}/${P}.tar.xz
	https://github.com/oreo1298/FreeMediaConverter/releases/download/v${PV}/${P}-vendor.tar.xz
"

LICENSE="GPL-3+"
# Vendored crates (checked by cargo-deny upstream).
LICENSE+=" Apache-2.0 Apache-2.0-with-LLVM-exceptions BSD BSD-2 ISC MIT MPL-2.0 Unicode-3.0 ZLIB"
SLOT="0"
KEYWORDS="~amd64 ~arm64"

DEPEND="
	>=dev-libs/glib-2.80:2
	>=gui-libs/gtk-4.14:4
	>=gui-libs/libadwaita-1.5:1
"
RDEPEND="
	${DEPEND}
	media-video/ffmpeg
"
BDEPEND="
	dev-libs/appstream
	dev-util/desktop-file-utils
	sys-devel/gettext
	virtual/pkgconfig
"

QA_FLAGS_IGNORED="usr/bin/retort usr/bin/retort-cli"

src_unpack() {
	# The -vendor tarball adds vendor/ and .cargo/config.toml to ${S}.
	default
}

src_configure() {
	local emesonargs=(
		-Doffline=true
	)
	meson_src_configure
}

src_compile() {
	meson_src_compile
}

src_test() {
	meson_src_test
}

src_install() {
	meson_src_install
}

pkg_postinst() {
	xdg_pkg_postinst
	gnome2_schemas_update
}

pkg_postrm() {
	xdg_pkg_postrm
	gnome2_schemas_update
}
