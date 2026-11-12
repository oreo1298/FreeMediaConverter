# SPDX-License-Identifier: GPL-3.0-or-later
# Rust sources are scanned with xgettext's C parser (gettext < 0.24 has no
# Rust support). Keywords: gettext/ngettext (default), tr_fmt, ntr_fmt, and
# n_ (core-crate marker for catalog strings).
i18n.gettext(
  app_bin,
  preset: 'glib',
  args: ['--keyword=tr_fmt', '--keyword=ntr_fmt:1,2', '--keyword=n_'],
)
