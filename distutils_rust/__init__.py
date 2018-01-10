# distutils_rust.py - distutils extension for building Rust extension modules
#
# Copyright 2017 Facebook, Inc.

from __future__ import absolute_import
import distutils
import distutils.command.build
import distutils.core
import distutils.errors
import distutils.util
import os
import shutil
import subprocess
import tarfile

class RustExtension(object):
    """Data for a Rust extension.

    'name' is the name of the extension, and must match the 'name' in the Cargo
    manifest.  This will also be the name of the python module that is
    produced.

    'package' is the name of the python package into which the compiled
    extension will be placed.  If none, the extension will be placed in the
    root package.

    'manifest' is the path to the Cargo.toml file for the Rust project.
    """
    def __init__(self, name, package=None, manifest=None):
        self.name = name
        self.package = package
        self.manifest = manifest or 'Cargo.toml'

class RustVendoredCrates(object):
    """Data for Rust vendored crates stored in Dewey.

    'name' is the name of the vendoring set, and is also the Dewey tag.

    'project' is the Dewey project the vendored crates are stored in.

    'hashfile' is the filename that stores the Dewey hash that should be
    downloaded.

    'path' is the path in Dewey of the file to download.

    'dest' is the directory into which the archive should be extracted.
    """
    def __init__(self, name, project=None, hashfile=None, path=None, dest=None):
        self.name = name
        self.project = project
        self.hashfile = hashfile or '.' + name
        self.path = path or name + '.tar.gz'
        self.dest = dest or '.'

class BuildRustExt(distutils.core.Command):

    description = "build Rust extensions (compile/link to build directory)"

    user_options = [
        ('build-lib=', 'b',
         "directory for compiled extension modules"),
        ('build-temp=', 't',
         "directory for temporary files (build by-products)"),
        ('debug', 'g',
         "compile in debug mode"),
        ('inplace', 'i',
         "ignore build-lib and put compiled extensions into the source " +
         "directory alongside your pure Python modules"),
    ]

    boolean_options = ['debug', 'inplace']

    def initialize_options(self):
        self.build_lib = None
        self.build_temp = None
        self.debug = None
        self.inplace = None

    def finalize_options(self):
        self.set_undefined_options('build',
                                   ('build_lib', 'build_lib'),
                                   ('build_temp', 'build_temp'),
                                   ('debug', 'debug'),
                                   )

    def run(self):
        # Download vendored crates
        for ven in self.distribution.rust_vendored_crates:
            self.download_vendored_crates(ven)

        # Build Rust extensions
        for ext in self.distribution.rust_ext_modules:
            self.build_ext(ext)

    def get_temp_path(self, ext):
        """Returns the path of the temporary directory to build in."""
        return os.path.join(self.build_temp,
                            os.path.dirname(ext.manifest),
                            'target')

    def get_temp_output(self, ext):
        """Returns the location in the temp directory of the output file."""
        platform = distutils.util.get_platform()
        if platform.startswith('win-'):
            name = ext.name + '.dll'
        elif platform.startswith('macosx'):
            name = 'lib' + ext.name + '.dylib'
        else:
            name = 'lib' + ext.name + '.so'
        return os.path.join('debug' if self.debug else 'release', name)

    def get_output_filename(self, ext):
        """Returns the filename of the build output."""
        if self.inplace:
            # the inplace option requires to find the package directory
            # using the build_py command for that
            build_py = self.get_finalized_command('build_py')
            package_dir = build_py.get_package_dir(ext.package)
        else:
            package_dir = os.path.join(self.build_lib, *ext.package.split('.'))
        platform = distutils.util.get_platform()
        if platform.startswith('win-'):
            name = ext.name + '.pyd'
        else:
            name = ext.name + '.so'
        return os.path.join(package_dir, name)

    def download_vendored_crates(self, ven):
        commit = open(ven.hashfile).read()
        desttarfile = os.path.join(ven.dest, ven.path)
        desthashfile = desttarfile + ".hash"
        if (os.path.exists(ven.dest) and
                os.path.exists(desthashfile) and
                commit == open(desthashfile).read()):
            # Already downloaded, nothing to do.
            return

        try:
            os.makedirs(ven.dest)
        except OSError:
            pass

        distutils.log.info("downloading vendored crates '%s'", ven.name)
        cmd = ["dewey", "cat"]
        if ven.project is not None:
            cmd += ["--project", ven.project]
        cmd += [
            "--commit", commit,
            "--tag", ven.name,
            "--path", ven.path,
            "--dest", desttarfile]

        rc = subprocess.call(cmd)
        if rc:
            raise distutils.errors.CompileError(
                "download of Rust vendored crates '%s' failed" % ven.name)

        with tarfile.open(desttarfile, 'r:gz') as tar:
            tar.extractall(ven.dest)
        os.remove(desttarfile)
        with open(desthashfile, 'w') as f:
            f.write(commit)

    def build_ext(self, ext):
        distutils.log.info("building '%s' extension", ext.name)

        cmd = ['cargo', 'build', '--manifest-path', ext.manifest]
        if not self.debug:
            cmd.append('--release')

        env = os.environ.copy()
        env['CARGO_TARGET_DIR'] = self.get_temp_path(ext)

        rc = subprocess.call(cmd, env=env)
        if rc:
            raise distutils.errors.CompileError(
                "compilation of Rust extension '%s' failed" % ext.name)

        src = os.path.join(self.get_temp_path(ext), self.get_temp_output(ext))
        dest = self.get_output_filename(ext)
        try:
            os.makedirs(os.path.dirname(dest))
        except OSError:
            pass
        desttmp = dest + '.tmp'
        shutil.copy(src, desttmp)
        shutil.move(desttmp, dest)

distutils.dist.Distribution.rust_ext_modules = ()
distutils.dist.Distribution.rust_vendored_crates = ()
distutils.command.build.build.sub_commands.append(
    ('build_rust_ext', lambda self: bool(self.distribution.rust_ext_modules)))
