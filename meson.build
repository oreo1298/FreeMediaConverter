# SPDX-License-Identifier: GPL-3.0-or-later
# Meson drives the install: Cargo builds the binaries (build-aux/cargo-build.py),
# Meson templates and installs the data files from branding.env.
project(
  'retort',
  version: '0.1.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 0.62.0',
)

fs = import('fs')
gnome = import('gnome')
i18n = import('i18n')
python = import('python').find_installation('python3')

# The app's identity is defined once, in branding.env.
branding = {}
foreach line : fs.read('branding.env').split('\n')
  line = line.strip()
  if line == '' or line.startswith('#')
    continue
  endif
  key = line.split('=')[0]
  branding += {key.strip(): line.replace(key + '=', '').strip()}
endforeach
app_id = branding['APP_ID']
app_bin = branding['APP_BIN']

# Cargo and Meson must agree on the version.
if not fs.read('Cargo.toml').contains('version = "@0@"'.format(meson.project_version()))
  error('The workspace version in Cargo.toml differs from meson.build')
endif

prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
datadir = prefix / get_option('datadir')
localedir = prefix / get_option('localedir')

conf = configuration_data()
foreach key, value : branding
  conf.set(key, value)
endforeach
conf.set('APP_PATH', '/' + app_id.replace('.', '/') + '/')
conf.set('VERSION', meson.project_version())

# ---- Rust binaries
cargo = find_program('cargo')
cargo_profile = get_option('buildtype') == 'debug' ? 'debug' : 'release'
cargo_target_dir = meson.project_build_root() / 'cargo-target'
cargo_env = environment()
cargo_env.set('RETORT_LOCALEDIR', localedir)
cargo_env.set('RETORT_PKGDATADIR', datadir / app_bin)
# Installable build: no build-tree paths compiled into the binaries (dev
# fallbacks for `cargo run` only; packagers' tools flag such references).
cargo_env.set('RETORT_INSTALLED_BUILD', '1')
cargo_args = [
  '--manifest', meson.project_source_root() / 'Cargo.toml',
  '--target-dir', cargo_target_dir,
  '--profile', cargo_profile,
  '--outdir', '@OUTDIR@',
  '--package', 'retort',
  '--package', 'retort-cli',
]
if get_option('offline')
  cargo_args += '--offline'
endif
# Newer libadwaita widgets are compiled in when the installed version has them.
adw_dep = dependency('libadwaita-1', version: '>= 1.5')
gtk_dep = dependency('gtk4', version: '>= 4.14')
cargo_features = []
foreach v : ['1.8', '1.7', '1.6']
  if cargo_features.length() == 0 and adw_dep.version().version_compare('>= ' + v)
    cargo_features += 'retort/adw_' + v.replace('.', '_')
  endif
endforeach
if get_option('update_check')
  cargo_features += 'retort/update-check'
endif
if cargo_features.length() > 0
  cargo_args += ['--features', ','.join(cargo_features)]
endif
custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  console: true,
  output: [app_bin, app_bin + '-cli'],
  install: true,
  install_dir: [bindir, bindir],
  env: cargo_env,
  command: [python, files('build-aux/cargo-build.py'), cargo_args, app_bin, app_bin + '-cli'],
)

summary({
  'libadwaita': adw_dep.version(),
  'GTK': gtk_dep.version(),
  'Cargo features': cargo_features.length() > 0 ? ', '.join(cargo_features) : 'none',
  'Cargo profile': cargo_profile,
}, section: 'Build')

subdir('data')
subdir('po')

if get_option('cargo_tests')
  test(
    'cargo-test',
    cargo,
    args: ['test', '--manifest-path', meson.project_source_root() / 'Cargo.toml',
           '--target-dir', cargo_target_dir, '--workspace', '--locked'],
    env: cargo_env,
    timeout: 3600,
  )
endif

# `meson devenv -C _build retort` runs the app from the build tree.
devenv = environment()
devenv.prepend('PATH', meson.project_build_root())
devenv.set('GSETTINGS_SCHEMA_DIR', meson.project_build_root() / 'data')
meson.add_devenv(devenv)

gnome.post_install(
  glib_compile_schemas: true,
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
