# distutils_rust.py - distutils extension for building Rust extension modules
#
# Copyright 2017 Facebook, Inc.

from __future__ import absolute_import

import contextlib
import distutils
import distutils.command.build
import distutils.core
import distutils.errors
import distutils.util
import os
import shutil
import subprocess
import tarfile


def find_fbsource_root():
    d = os.getcwd()
    while d != os.path.dirname(d) and \
            not os.path.exists(os.path.join(d, '.hg')):
        d = os.path.dirname(d)
    if d == os.path.dirname(d):
        raise RuntimeError('Could not find the fbsource root. CWD=%s' %
                           os.getcwd())
    return d

FBSOURCE = find_fbsource_root()
LFS_SCRIPT_PATH = os.path.join(FBSOURCE, "fbcode/tools/lfs/lfs.py")
LFS_POINTERS = os.path.join(__file__, '../../fb/tools/.lfs-pointers')


@contextlib.contextmanager
def chdir(path):
    cwd = os.getcwd()
    try:
        os.chdir(path)
        yield
    finally:
        os.chdir(cwd)


class RustExtension(object):
    """Data for a Rust extension.

    'name' is the name of the target, and must match the 'name' in the Cargo
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
        self.manifest = manifest or "Cargo.toml"
        self.type = "library"

    @property
    def dstnametmp(self):
        platform = distutils.util.get_platform()
        if platform.startswith("win-"):
            name = self.name + ".dll"
        elif platform.startswith("macosx"):
            name = "lib" + self.name + ".dylib"
        else:
            name = "lib" + self.name + ".so"
        return name

    @property
    def dstname(self):
        platform = distutils.util.get_platform()
        if platform.startswith("win-"):
            name = self.name + ".pyd"
        else:
            name = self.name + ".so"
        return name


class RustBinary(object):
    """Data for a Rust binary.

    'name' is the name of the target, and must match the 'name' in the Cargo
    manifest.  This will also be the name of the binary that is
    produced.

    'manifest' is the path to the Cargo.toml file for the Rust project.
    """

    def __init__(self, name, package=None, manifest=None):
        self.name = name
        self.manifest = manifest or "Cargo.toml"
        self.type = "binary"

    @property
    def dstnametmp(self):
        platform = distutils.util.get_platform()
        if platform.startswith("win-"):
            return self.name + ".exe"
        else:
            return self.name

    @property
    def dstname(self):
        platform = distutils.util.get_platform()
        if platform.startswith("win-"):
            return self.name + ".exe"
        else:
            return self.name


class RustVendoredCrates(object):
    """Data for Rust vendored crates stored in LFS.

    'name' is the name of the vendoring set, and is also the LFS name (with
    ".tar.gz" appended) to download.

    'dest' is the directory into which the archive should be extracted.
    """

    def __init__(self, name, dest=None):
        self.name = name
        self.dest = dest or os.getcwd()

    @property
    def filename(self):
        return "%s.tar.gz" % self.name


class BuildRustExt(distutils.core.Command):

    description = "build Rust extensions (compile/link to build directory)"

    user_options = [
        ("build-lib=", "b", "directory for compiled extension modules"),
        ("build-exe=", "e", "directory for compiled binary targets"),
        ("build-temp=", "t", "directory for temporary files (build by-products)"),
        ("debug", "g", "compile in debug mode"),
        (
            "inplace",
            "i",
            "ignore build-lib and put compiled extensions into the source "
            + "directory alongside your pure Python modules",
        ),
    ]

    boolean_options = ["debug", "inplace"]

    def initialize_options(self):
        self.build_lib = None
        self.build_exe = None
        self.build_temp = None
        self.debug = None
        self.inplace = None

    def finalize_options(self):
        self.set_undefined_options(
            "build",
            ("build_lib", "build_lib"),
            ("build_temp", "build_temp"),
            ("debug", "debug"),
        )
        self.set_undefined_options("build_scripts", ("build_dir", "build_exe"))

    def run(self):
        # Download vendored crates
        for ven in self.distribution.rust_vendored_crates:
            self.download_vendored_crates(ven)

        # Build Rust extensions
        for target in self.distribution.rust_ext_modules:
            self.build_library(target)

        # Build Rust binaries
        for target in self.distribution.rust_ext_binaries:
            self.build_binary(target)

    def get_temp_path(self):
        """Returns the path of the temporary directory to build in."""
        return os.path.join(self.build_temp, "cargo-target")

    def get_temp_output(self, target):
        """Returns the location in the temp directory of the output file."""
        return os.path.join("debug" if self.debug else "release", target.dstnametmp)

    def get_output_filename(self, target):
        """Returns the filename of the build output."""
        if target.type == "library":
            if self.inplace:
                # the inplace option requires to find the package directory
                # using the build_py command for that
                build_py = self.get_finalized_command("build_py")
                return os.path.join(
                    build_py.get_package_dir(target.package), target.dstname
                )
            else:
                return os.path.join(
                    os.path.join(self.build_lib, *target.package.split(".")),
                    target.dstname,
                )
        elif target.type == "binary":
            return os.path.join(self.build_exe, target.dstname)
        else:
            raise distutils.errors.CompileError("Unknown Rust target type")

    def download_vendored_crates(self, ven):
        try:
            os.makedirs(ven.dest)
        except OSError:
            pass

        distutils.log.info("downloading vendored crates '%s'", ven.name)
        cmd = [
            "python",
            LFS_SCRIPT_PATH,
            '-l',
            LFS_POINTERS,
            "download",
            ven.filename
        ]
        with chdir(ven.dest):
            rc = subprocess.call(cmd)
            if rc:
                raise distutils.errors.CompileError(
                    "download of Rust vendored crates '%s' failed" % ven.name
                )

            with tarfile.open(ven.filename, "r:gz") as tar:
                tar.extractall()

    def build_target(self, target):
        """Build Rust target"""
        # Cargo.lock may become out-of-date and make complication fail if
        # vendored crates are updated. Remove it so it can be re-generated.
        cargolockpath = os.path.join(os.path.dirname(target.manifest), "Cargo.lock")
        if os.path.exists(cargolockpath):
            os.unlink(cargolockpath)

        cmd = ["cargo", "build", "--manifest-path", target.manifest]
        if not self.debug:
            cmd.append("--release")

        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = self.get_temp_path()

        rc = subprocess.call(cmd, env=env)
        if rc:
            raise distutils.errors.CompileError(
                "compilation of Rust targetension '%s' failed" % target.name
            )

        src = os.path.join(self.get_temp_path(), self.get_temp_output(target))
        dest = self.get_output_filename(target)
        try:
            os.makedirs(os.path.dirname(dest))
        except OSError:
            pass
        desttmp = dest + ".tmp"
        shutil.copy(src, desttmp)
        shutil.move(desttmp, dest)

    def build_binary(self, target):
        distutils.log.info("building '%s' binary", target.name)
        self.build_target(target)

    def build_library(self, target):
        distutils.log.info("building '%s' library extension", target.name)
        self.build_target(target)


distutils.dist.Distribution.rust_ext_modules = ()
distutils.dist.Distribution.rust_ext_binaries = ()
distutils.dist.Distribution.rust_vendored_crates = ()
distutils.command.build.build.sub_commands.append(
    ("build_rust_ext", lambda self: bool(self.distribution.rust_ext_modules))
)
