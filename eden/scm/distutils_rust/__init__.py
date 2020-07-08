# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# distutils_rust.py - distutils extension for building Rust extension modules

from __future__ import absolute_import

import contextlib
import distutils
import distutils.command.build
import distutils.command.install
import distutils.command.install_scripts
import distutils.core
import distutils.errors
import distutils.util
import errno
import os
import shutil
import subprocess
import tarfile
import tempfile
import time


# This manifest is merged with the default .exe manifest to allow
# it to support long paths on Windows
LONG_PATHS_MANIFEST = """\
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <application>
        <windowsSettings
        xmlns:ws2="http://schemas.microsoft.com/SMI/2016/WindowsSettings">
            <ws2:longPathAware>true</ws2:longPathAware>
        </windowsSettings>
    </application>
</assembly>"""


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

    def __init__(self, name, package=None, manifest=None, features=None):
        self.name = name
        self.package = package
        self.manifest = manifest or "Cargo.toml"
        self.type = "library"
        self.features = features

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

    def __init__(self, name, package=None, manifest=None, rename=None, features=None):
        self.name = name
        self.manifest = manifest or "Cargo.toml"
        self.type = "binary"
        self.final_name = rename or name
        self.features = features

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
            return self.final_name + ".exe"
        else:
            return self.final_name


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
        (
            "long-paths-support",
            "l",
            "Windows-only. Add a manifest entry that makes the resulting binary long paths aware. "
            + "See https://docs.microsoft.com/en-us/windows/desktop/fileio/naming-a-file"
            + "#maximum-path-length-limitation for more details.",
        ),
    ]

    boolean_options = ["debug", "inplace"]

    def initialize_options(self):
        self.build_lib = None
        self.build_exe = None
        self.build_temp = None
        self.debug = None
        self.inplace = None
        self.long_paths_support = False
        self.features = None

    def finalize_options(self):
        self.set_undefined_options(
            "build",
            ("build_lib", "build_lib"),
            ("build_temp", "build_temp"),
            ("debug", "debug"),
        )
        self.set_undefined_options("build_scripts", ("build_dir", "build_exe"))

    def run(self):
        # write cargo config
        self.write_cargo_config()

        # Build Rust extensions
        for target in self.distribution.rust_ext_modules:
            self.build_library(target)

        # Build Rust binaries
        for target in self.distribution.rust_ext_binaries:
            self.build_binary(target)

    def get_cargo_target(self):
        return os.path.abspath(os.path.join("build", "cargo-target"))

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

    def write_cargo_config(self):
        config = """
# On OS X targets, configure the linker to perform dynamic lookup of undefined
# symbols.  This allows the library to be used as a Python extension.

[target.i686-apple-darwin]
rustflags = ["-C", "link-args=-Wl,-undefined,dynamic_lookup"]

[target.x86_64-apple-darwin]
rustflags = ["-C", "link-args=-Wl,-undefined,dynamic_lookup"]

# Use ld.gold on Linux to improve build time.

[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-args=-fuse-ld=gold"]

[build]
"""
        config += 'target-dir = "{}"\n'.format(self.get_cargo_target())
        paths = self.rust_binary_paths()
        for key in ["rustc", "rustdoc"]:
            if key in paths:
                config += '{} = "{}"\n'.format(key, paths[key])

        vendored_path = self.rust_vendored_crate_path()
        if vendored_path:
            config += """
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
"""
            config += 'directory = "{}"\n'.format(vendored_path)

        if os.name == "nt":
            config = config.replace("\\", "\\\\")

        try:
            os.mkdir(".cargo")
        except OSError as e:
            if e.errno != errno.EEXIST:
                raise
        with open(".cargo/config", "w") as f:
            f.write(config)

    def build_target(self, target):
        """Build Rust target"""
        # Cargo.lock may become out-of-date and make complication fail if
        # vendored crates are updated. Remove it so it can be re-generated.
        cargolockpath = os.path.join(os.path.dirname(target.manifest), "Cargo.lock")
        if os.path.exists(cargolockpath):
            os.unlink(cargolockpath)

        paths = self.rust_binary_paths()
        cmd = [paths.get("cargo", "cargo"), "build", "--manifest-path", target.manifest]
        if not self.debug:
            cmd.append("--release")

        if target.features:
            cmd.append("--features")
            cmd.append(target.features)

        env = os.environ.copy()
        env["LIB_DIRS"] = os.path.abspath(self.build_temp)

        rc = subprocess.call(cmd, env=env)
        if rc:
            raise distutils.errors.CompileError(
                "compilation of Rust target '%s' failed" % target.name
            )

        src = os.path.join(self.get_cargo_target(), self.get_temp_output(target))

        if (
            target.type == "binary"
            and distutils.util.get_platform().startswith("win-")
            and self.long_paths_support
        ):
            retry = 0
            while True:
                try:
                    self.set_long_paths_manifest(src)
                except Exception:
                    # mt.exe wants exclusive access to the exe. It can fail with
                    # if Windows Anti-Virus scans the exe. Retry a few times.
                    retry += 1
                    if retry > 5:
                        raise
                    else:
                        distutils.log.warn("Retrying setting long path on %s" % src)
                        continue
                else:
                    break

        dest = self.get_output_filename(target)
        try:
            os.makedirs(os.path.dirname(dest))
        except OSError:
            pass
        desttmp = dest + ".tmp"
        shutil.copy(src, desttmp)
        shutil.move(desttmp, dest)

        # Copy pdb debug info.
        pdbsrc = src[:-4] + ".pdb"
        if os.path.exists(pdbsrc):
            pdbdest = dest[:-4] + ".pdb"
            shutil.copy(pdbsrc, pdbdest)

    def set_long_paths_manifest(self, fname):
        if not distutils.util.get_platform().startswith("win-"):
            # This only makes sense on Windows
            distutils.log.info(
                "skipping set_long_paths_manifest call for %s "
                "as the plaform in not Windows",
                fname,
            )
            return
        if not fname.endswith(".exe"):
            # we only care about executables
            distutils.log.info(
                "skipping set_long_paths_manifest call for %s "
                "as the file extension is not exe",
                fname,
            )
            return

        fdauto, manfname = tempfile.mkstemp(suffix=".distutils_rust.manifest")
        os.close(fdauto)
        with open(manfname, "w") as f:
            f.write(LONG_PATHS_MANIFEST)
        distutils.log.debug(
            "LongPathsAware manifest written into tempfile: %s", manfname
        )
        inputresource = "-inputresource:%s;#1" % fname
        outputresource = "-outputresource:%s;#1" % fname
        command = [
            "mt.exe",
            "-nologo",
            "-manifest",
            manfname,
            outputresource,
            inputresource,
        ]
        try:
            distutils.log.debug(
                "Trying to merge LongPathsAware manifest with the existing "
                "manifest of the binary %s",
                fname,
            )
            subprocess.check_output(command)
            distutils.log.debug(
                "LongPathsAware manifest successfully merged into %s", fname
            )
        except subprocess.CalledProcessError as e:
            no_resource_err = (
                "The specified image file did not contain a resource section".lower()
            )
            if no_resource_err not in e.output.lower():
                distutils.log.error(
                    "Setting LongPathsAware manifest failed: %r", e.output
                )
                raise
            distutils.log.debug(
                "The binary %s does not have an existing manifest. Writing "
                "a LongPathsAware one",
                fname,
            )
            # Since the image does not contain a resource section, we don't try to merge manifests,
            # but rather just write one.
            command.remove(inputresource)
            subprocess.check_output(command)
            distutils.log.debug(
                "LongPathsAware manifest successfully written into %s", fname
            )
        finally:
            os.remove(manfname)

    def build_binary(self, target):
        distutils.log.info("building '%s' binary", target.name)
        self.build_target(target)

    def build_library(self, target):
        distutils.log.info("building '%s' library extension", target.name)
        self.build_target(target)

    try:
        from distutils_rust.fb import rust_binary_paths, rust_vendored_crate_path
    except ImportError:

        def rust_vendored_crate_path(self):
            return os.environ.get("RUST_VENDORED_CRATES_DIR")

        def rust_binary_paths(self):
            return {"cargo": os.environ.get("CARGO_BIN", "cargo")}


class InstallRustExt(distutils.command.install_scripts.install_scripts):
    description = "install Rust extensions and binaries"

    def run(self):
        if not self.skip_build:
            self.run_command("build_rust_ext")
        self.outfiles = self.copy_tree(self.build_dir, self.install_dir)


distutils.dist.Distribution.rust_ext_modules = ()
distutils.dist.Distribution.rust_ext_binaries = ()
distutils.command.build.build.sub_commands.append(
    (
        "build_rust_ext",
        lambda self: bool(self.distribution.rust_ext_modules)
        or bool(self.distribution.rust_ext_binaries),
    )
)
distutils.command.install.install.sub_commands.append(
    (
        "install_rust_ext",
        lambda self: bool(self.distribution.rust_ext_modules)
        or bool(self.distribution.rust_ext_binaries),
    )
)
