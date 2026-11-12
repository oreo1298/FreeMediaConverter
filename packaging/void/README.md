# Void Linux template

Copy `srcpkgs/retort` into a `void-packages` checkout and run
`./xbps-src pkg retort`. The release workflow fills in the two
`checksum` lines (`@SHA256_SRC@`, `@SHA256_VENDOR@`) from
`dist/retort-<version>.sha256`.
