# SPDX-License-Identifier: GPL-3.0-or-later
# Translations are merged into the templates first (msgids keep their
# @APP_NAME@-style placeholders), then build-aux/subst.py fills in branding.env.
subst = [python, files('../build-aux/subst.py'),
         '--branding', meson.project_source_root() / 'branding.env',
         '--set', 'APP_PATH=' + conf.get('APP_PATH'),
         '--set', 'VERSION=' + meson.project_version(),
         '@INPUT@', '@OUTPUT@']

# Desktop entry
desktop_merged = i18n.merge_file(
  input: 'app.desktop.in',
  output: 'app.desktop.merged',
  type: 'desktop',
  po_dir: '../po',
)
desktop_file = custom_target(
  'desktop-file',
  input: desktop_merged,
  output: app_id + '.desktop',
  command: subst,
  depend_files: files('../branding.env'),
  install: true,
  install_dir: datadir / 'applications',
)
desktop_file_validate = find_program('desktop-file-validate', required: false)
if desktop_file_validate.found()
  test('validate-desktop', desktop_file_validate, args: [desktop_file])
endif

# AppStream metadata
metainfo_merged = i18n.merge_file(
  input: 'app.metainfo.xml.in',
  output: 'app.metainfo.xml.merged',
  po_dir: '../po',
)
metainfo_file = custom_target(
  'metainfo-file',
  input: metainfo_merged,
  output: app_id + '.metainfo.xml',
  command: subst,
  depend_files: files('../branding.env'),
  install: true,
  install_dir: datadir / 'metainfo',
)
appstreamcli = find_program('appstreamcli', required: false)
if appstreamcli.found()
  test('validate-metainfo', appstreamcli,
       args: ['validate', '--no-net', '--explain', metainfo_file])
endif

# GSettings schema (summaries are translated at runtime via gettext-domain).
schema = configure_file(
  input: 'app.gschema.xml.in',
  output: app_id + '.gschema.xml',
  configuration: conf,
  install: true,
  install_dir: datadir / 'glib-2.0' / 'schemas',
)
glib_compile_schemas = find_program('glib-compile-schemas')
# Compiled in the build dir too, for `meson devenv`.
custom_target(
  'gschemas-compiled',
  input: schema,
  output: 'gschemas.compiled',
  build_by_default: true,
  command: [glib_compile_schemas, '--strict', '--targetdir', meson.current_build_dir(), meson.current_build_dir()],
)
test('validate-schema', glib_compile_schemas,
     args: ['--strict', '--dry-run', meson.current_build_dir()])

# Icons
install_data('icons/app.svg',
  install_dir: datadir / 'icons' / 'hicolor' / 'scalable' / 'apps',
  rename: app_id + '.svg')
install_data('icons/app-symbolic.svg',
  install_dir: datadir / 'icons' / 'hicolor' / 'symbolic' / 'apps',
  rename: app_id + '-symbolic.svg')
