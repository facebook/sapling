# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# This is the Sapling setup script.
#
# 'python3 setup.py install', or
# 'python3 setup.py --help' for more options

# isort:skip_file
import os
import sys

# If we're executing inside an embedded Python instance, it won't load
# modules outside the embedded python. So let's add our directory manually,
# before we import things.
sys.path.append(os.path.dirname(os.path.realpath(__file__)))

import contextlib
import errno
import glob
import hashlib
import logging
import re
import shutil
import stat
import struct
import subprocess
import tempfile
from sysconfig import get_platform

from contrib.pick_python import load_build_env

log = logging.getLogger()

ossbuild = bool(os.environ.get("SAPLING_OSS_BUILD"))

# If this is set, then skip downloading third-party dependencies
# like IPython.
offline = bool(os.environ.get("SAPLING_OFFLINE"))


def ensureenv():
    """Load build/env's as environment variables.

    If build/env has specified a different set of environment variables,
    restart the current command. Otherwise do nothing.
    """
    env = load_build_env()
    if all(os.environ.get(k) == v for k, v in env.items()):
        # No restart needed
        return
    # Restart with new environment
    newenv = os.environ.copy()
    newenv.update(env)
    # Pick the right Python interpreter
    python = env.get("PYTHON_SYS_EXECUTABLE", sys.executable)
    print("Relaunching with %s and build/env environment" % python, file=sys.stderr)
    p = subprocess.Popen([python] + sys.argv, env=newenv)
    sys.exit(p.wait())


ensureenv()

PY_VERSION = "%s%s" % sys.version_info[:2]

# rust-cpython uses this to collect Python information
os.environ["PYTHON_SYS_EXECUTABLE"] = sys.executable


def filter(f, it):
    return list(__builtins__.filter(f, it))


from distutils.ccompiler import new_compiler

from distutils.command.build import build
from distutils.command.build_scripts import build_scripts
from distutils.command.install import install
from distutils.command.install_scripts import install_scripts
from distutils.core import Command, setup
from distutils.spawn import spawn

from distutils_rust import BuildRustExt, InstallRustExt, RustBinary

havefb = not ossbuild and os.path.exists("fb")
isgetdepsbuild = os.environ.get("GETDEPS_BUILD") == "1"

# Find path for dependencies when not in havefb mode
dep_build_dir = "../../.."
dep_install_dir = "../../.."
if isgetdepsbuild:
    # when running from getdeps src-dir may be . so don't use .. to get from source to build and install
    getdeps_build = os.environ.get("GETDEPS_BUILD_DIR", None)
    if getdeps_build:
        dep_build_dir = getdeps_build
    getdeps_install = os.environ.get("GETDEPS_INSTALL_DIR", None)
    if getdeps_install:
        dep_install_dir = getdeps_install
    # getdeps builds of hg client are OSS only
    havefb = False

iswindows = os.name == "nt"
NOOPTIMIZATION = "/Od" if iswindows else "-O0"
PIC = "" if iswindows else "-fPIC"
PRODUCEDEBUGSYMBOLS = "/DEBUG:FULL" if iswindows else "-g"
STDC99 = "" if iswindows else "-std=c99"
STDCPP0X = "" if iswindows else "-std=c++0x"
STDCPP11 = "" if iswindows else "-std=c++11"
WALL = "/Wall" if iswindows else "-Wall"
WSTRICTPROTOTYPES = None if iswindows else "-Werror=strict-prototypes"

cflags = []

# if this is set, compile all C extensions with -O0 -g for easy debugging.  note
# that this is not manifested in any way in the Makefile dependencies.
# therefore, if you already have build products, they won't be rebuilt!
if os.getenv("FB_HGEXT_CDEBUG") is not None:
    cflags.extend([NOOPTIMIZATION, PRODUCEDEBUGSYMBOLS])


def write_if_changed(path, content):
    """Write content to a file iff the content hasn't changed."""
    if os.path.exists(path):
        with open(path, "rb") as fh:
            current = fh.read()
    else:
        current = b""

    if current != content:
        with open(path, "wb") as fh:
            fh.write(content)


pjoin = os.path.join
relpath = os.path.relpath
scriptdir = os.path.realpath(pjoin(__file__, ".."))
builddir = pjoin(scriptdir, "build")


def ensureexists(path):
    if not os.path.exists(path):
        os.makedirs(path)


def ensureempty(path):
    if os.path.exists(path):
        rmtree(path)
    os.makedirs(path)


def samepath(path1, path2):
    p1 = os.path.normpath(os.path.normcase(path1))
    p2 = os.path.normpath(os.path.normcase(path2))
    return p1 == p2


def tryunlink(path):
    try:
        os.unlink(path)
    except Exception as ex:
        if ex.errno != errno.ENOENT:
            raise


def copy_to(source, target):
    if os.path.isdir(source):
        shutil.copytree(source, target)
    else:
        ensureexists(os.path.dirname(target))
        shutil.copy2(source, target)


def rmtree(path):
    # See https://stackoverflow.com/questions/1213706/what-user-do-python-scripts-run-as-in-windows
    processed = set()

    def handlereadonly(func, path, exc):
        if path not in processed:
            processed.add(path)
            excvalue = exc[1]
            if func in (os.rmdir, os.remove) and excvalue.errno == errno.EACCES:
                os.chmod(path, stat.S_IRWXU | stat.S_IRWXG | stat.S_IRWXO)
                return func(path)
        raise

    shutil.rmtree(path, ignore_errors=False, onerror=handlereadonly)


@contextlib.contextmanager
def chdir(nwd):
    cwd = os.getcwd()
    try:
        log.debug("chdir: %s", nwd)
        os.chdir(nwd)
        yield
    finally:
        log.debug("restore chdir: %s", cwd)
        os.chdir(cwd)


# Rename hg to $HGNAME. Useful when "hg" is a wrapper calling $HGNAME (or chg).
hgname = os.environ.get("HGNAME", "hg")
if not re.match(r"\A(hg|sl)[.0-9a-z-]*\Z", hgname):
    raise RuntimeError("Illegal HGNAME: %s" % hgname)


def cancompile(cc, code):
    tmpdir = tempfile.mkdtemp(prefix="hg-install-")
    devnull = oldstderr = None
    try:
        fname = os.path.join(tmpdir, "testcomp.c")
        f = open(fname, "w")
        f.write(code)
        f.close()
        # Redirect stderr to /dev/null to hide any error messages
        # from the compiler.
        # This will have to be changed if we ever have to check
        # for a function on Windows.
        devnull = open("/dev/null", "w")
        oldstderr = os.dup(sys.stderr.fileno())
        os.dup2(devnull.fileno(), sys.stderr.fileno())
        objects = cc.compile([fname], output_dir=tmpdir)
        cc.link_executable(objects, os.path.join(tmpdir, "a.out"))
        return True
    except Exception:
        return False
    finally:
        if oldstderr is not None:
            os.dup2(oldstderr, sys.stderr.fileno())
        if devnull is not None:
            devnull.close()
        rmtree(tmpdir)


# simplified version of distutils.ccompiler.CCompiler.has_function
# that actually removes its temporary files.
def hasfunction(cc, funcname):
    code = "int main(void) { %s(); }\n" % funcname
    return cancompile(cc, code)


def hasheader(cc, headername):
    code = "#include <%s>\nint main(void) { return 0; }\n" % headername
    return cancompile(cc, code)


def runcmd(cmd, env):
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    out, err = p.communicate()
    return p.returncode, out, err


class hgcommand:
    def __init__(self, cmd, env):
        self.cmd = cmd
        self.env = env

    def run(self, args):
        cmd = self.cmd + args
        returncode, out, err = runcmd(cmd, self.env)
        err = filterhgerr(err)
        if err or returncode != 0:
            print("stderr from '%s':" % (" ".join(cmd)), file=sys.stderr)
            print(err, file=sys.stderr)
            return b""
        return out


def filterhgerr(err):
    # If root is executing setup.py, but the repository is owned by
    # another user (as in "sudo python setup.py install") we will get
    # trust warnings since the .hg/hgrc file is untrusted. That is
    # fine, we don't want to load it anyway.  Python may warn about
    # a missing __init__.py in mercurial/locale, we also ignore that.
    err = [
        e
        for e in err.splitlines()
        if (
            not e.startswith(b"not trusting file")
            and not e.startswith(b"warning: Not importing")
            and not e.startswith(b"obsolete feature not enabled")
            and not e.startswith(b"devel-warn:")
            # Ignore hints from building hg with an old hg
            and not e.startswith(b"hint[old-version]")
            and not e.startswith(b"hint[hint-ack]")
        )
    ]
    return b"\n".join(b"  " + e for e in err)


def findhg(vcname: str):
    """Try to figure out how we should invoke hg for examining the local
    repository contents.

    Returns an hgcommand object, or None if a working hg command cannot be
    found.
    """
    # By default, prefer the "hg" command in the user's path.  This was
    # presumably the hg command that the user used to create this repository.
    #
    # This repository may require extensions or other settings that would not
    # be enabled by running the hg script directly from this local repository.
    hgenv = os.environ.copy()
    # Use HGPLAIN to disable hgrc settings that would change output formatting,
    # and disable localization for the same reasons.
    hgenv["HGPLAIN"] = "1"
    hgenv["LANGUAGE"] = "C"
    hgcmd = [vcname]
    # Run a simple "hg log" command just to see if using hg from the user's
    # path works and can successfully interact with this repository.
    check_cmd = ["log", "-r.", "-Ttest"]
    try:
        retcode, out, err = runcmd(hgcmd + check_cmd, hgenv)
    except EnvironmentError:
        retcode = -1
    if retcode == 0 and not filterhgerr(err):
        return hgcommand(hgcmd, hgenv)

    # Fall back to trying the local hg installation.
    hgenv = localhgenv()
    hgcmd = [sys.executable, vcname]
    try:
        retcode, out, err = runcmd(hgcmd + check_cmd, hgenv)
    except EnvironmentError:
        retcode = -1
    if retcode == 0 and not filterhgerr(err):
        return hgcommand(hgcmd, hgenv)

    # Neither local or system hg can be used.
    return None


def localhgenv():
    """Get an environment dictionary to use for invoking or importing
    mercurial from the local repository."""
    # Execute hg out of this directory with a custom environment which takes
    # care to not use any hgrc files and do no localization.
    env = {
        "HGMODULEPOLICY": "py",
        "HGRCPATH": "",
        "LANGUAGE": "C",
        "PATH": "",
    }  # make pypi modules that use os.environ['PATH'] happy
    if "LD_LIBRARY_PATH" in os.environ:
        env["LD_LIBRARY_PATH"] = os.environ["LD_LIBRARY_PATH"]
    if "SystemRoot" in os.environ:
        # SystemRoot is required by Windows to load various DLLs.  See:
        # https://bugs.python.org/issue13524#msg148850
        env["SystemRoot"] = os.environ["SystemRoot"]
    return env


hg = findhg("sl") or findhg("hg")


def hgtemplate(template, cast=None):
    if not hg:
        return None
    result = hg.run(["log", "-r.", "-T", template]).decode("utf-8")
    if result and cast:
        result = cast(result)
    return result


def gitversion():
    format = "%cd-h%h"
    date_format = "format:%Y%m%d-%H%M%S"
    try:
        retcode, out, err = runcmd(
            [
                "git",
                "-c",
                "core.abbrev=8",
                "show",
                "-s",
                f"--format={format}",
                f"--date={date_format}",
            ],
            os.environ,
        )
        if retcode or err:
            return None
        return out.decode("utf-8")
    except EnvironmentError:
        return None


def pickversion():
    # Respect SAPLING_VERSION set by GitHub workflows.
    env_version = os.environ.get("SAPLING_VERSION")
    if env_version:
        return env_version
    # New version system: YYMMDD_HHmmSS_hash
    # This is duplicated a bit from build_rpm.py:auto_release_str()
    template = r'{sub("([:+-]|\d\d\d\d$)", "",date|isodatesec)} {node|short}'
    # if hg is not found, fallback to a fixed version
    out = hgtemplate(template) or gitversion() or ""
    # Some tools parse this number to figure out if they support this version of
    # Mercurial, so prepend with 4.4.2.
    # ex. 4.4.2_20180105_214829_58fda95a0202
    return "_".join(["4.4.2"] + out.split())


if not os.path.isdir(builddir):
    # Create the "build" directory
    try:
        # $DISK_TEMP is a location for scratch files on disk that sandcastle
        # maintains and cleans up between jobs
        if ("SANDCASTLE" in os.environ) and ("DISK_TEMP" in os.environ):
            scratchpath = os.path.join(
                os.environ["DISK_TEMP"], "hgbuild%d" % os.getpid()
            )
            ensureexists(scratchpath)
        else:
            # Prefer a symlink to a "scratch path" path if the "mkscratch" tool exists
            scratchpath = (
                subprocess.check_output(["mkscratch", "path", "--subdir", "hgbuild3"])
                .strip()
                .decode()
            )
        assert os.path.isdir(scratchpath)
        os.symlink(scratchpath, builddir, target_is_directory=True)
    except Exception:
        ensureexists(builddir)


sapling_version = pickversion()
sapling_versionb = sapling_version
if not isinstance(sapling_versionb, bytes):
    sapling_versionb = sapling_versionb.encode("ascii")

# calculate a versionhash, which is used by chg to make sure the client
# connects to a compatible server.
sapling_versionhash = str(
    struct.unpack(">Q", hashlib.sha1(sapling_versionb).digest()[:8])[0]
)


class hgbuild(build):
    # Insert hgbuildmo first so that files in mercurial/locale/ are found
    # when build_py is run next. Also, normally build_scripts is automatically
    # a subcommand of build iff the `scripts` argumnent or `setup` is present
    # Since we removed that argument, let's add the subcommand explicitly
    sub_commands = [("build_mo", None), ("build_scripts", None)] + build.sub_commands


class hgbuildmo(build):
    description = "build translations (.mo files)"

    def run(self):
        if not shutil.which("msgfmt"):
            self.warn("could not find msgfmt executable, no translations will be built")
            return

        podir = "i18n"
        if not os.path.isdir(podir):
            self.warn("could not find %s/ directory" % podir)
            return

        join = os.path.join
        for po in os.listdir(podir):
            if not po.endswith(".po"):
                continue
            pofile = join(podir, po)
            modir = join("locale", po[:-3], "LC_MESSAGES")
            mofile = join(modir, "hg.mo")
            mobuildfile = join("sapling", mofile)
            cmd = ["msgfmt", "-v", "-o", mobuildfile, pofile]
            if sys.platform != "sunos5":
                # msgfmt on Solaris does not know about -c
                cmd.append("-c")
            self.mkpath(join("sapling", modir))
            self.make_file([pofile], mobuildfile, spawn, (cmd,))


class hgbuildscripts(build_scripts):
    def run(self):
        if havefanotify:
            cc = new_compiler()
            objs = cc.compile(glob.glob("contrib/whochanges/*.c"), debug=True)
            dest = os.path.join(self.build_dir, "whochanges")
            cc.link_executable(objs, dest)

        return build_scripts.run(self)

    def copy_scripts(self):
        build_scripts.copy_scripts(self)


class buildembedded(Command):
    description = (
        "Build the embedded version of Mercurial. Intended to be used on Windows."
    )

    user_options = [
        (
            "local-bins",
            "l",
            "use binary files (shared libs and executables) "
            "from the hg dir (where this script lives), rather "
            "than ./build. Note that Python files are always "
            "taken from the hg dir",
        )
    ]

    def initialize_options(self):
        self.local_bins = False

    def finalize_options(self):
        pass

    def _copy_py_lib(self, dirtocopy):
        """Copy main Python shared library"""
        pyroot = os.path.realpath(pjoin(sys.executable, ".."))
        pylib = f"python{PY_VERSION}"
        pylibext = pylib + (".dll" if iswindows else ".so")
        # First priority is the python lib that lives alongside the executable
        pylibpath = pjoin(pyroot, pylibext)
        if not os.path.exists(pylibpath):
            # a fallback option
            import ctypes.util

            pylibpath = ctypes.util.find_library(pylib)
        log.debug("Python dynamic library is copied from: %s" % pylibpath)
        copy_to(pylibpath, pjoin(dirtocopy, os.path.basename(pylibpath)))
        # Copy pythonXX.zip
        pyzipname = pylib + ".zip"
        pyzippath = pjoin(pyroot, pyzipname)
        if os.path.exists(pyzippath):
            copy_to(pyzippath, pjoin(dirtocopy, pyzipname))

        # Copy native python modules

        # Python adds these paths to sys.path:
        # - Current EXE directory.
        # - python310.dll directory + "\DLLs", "\lib", "\python310.zip".
        # So if the main EXE is in a different directory, for example, in tests
        # the main EXE might be copied to $TESTTMP/bin, and uses this
        # python310.dll, it won't import stdlib native modules like
        # unicodedata. Fix it by moving the native modules to DLLs/.
        # Alternatively, the "embedded" windows python package should be built
        # with `PYTHONPATH` C macro set to "." [1], but that's not what the
        # official package provides.
        # [1]: https://github.com/python/cpython/blob/3.10/PC/pyconfig.h#L71
        dlls_dir = pjoin(dirtocopy, "DLLs")
        ensureexists(dlls_dir)
        for pylibpath in glob.glob(os.path.join(pyroot, "*.pyd")):
            copy_to(pylibpath, dlls_dir)
        for pylibpath in glob.glob(os.path.join(pyroot, "*.dll")):
            name = os.path.basename(pylibpath)
            if "python" in name or "vcruntime" in name:
                dest = dirtocopy
            else:
                dest = dlls_dir
            copy_to(pylibpath, dest)

    def _copy_hg_exe(self, dirtocopy):
        """Copy main mercurial executable which would load the embedded Python"""
        bindir = scriptdir
        if not self.local_bins:
            # copy .exe's from ./build/lib.win-amd64/, not from ./
            bindir = pjoin(scriptdir, "build", distutils_dir_name("scripts"))
            sourcename = f"{hgname}.exe" if iswindows else f"{hgname}.rust"
        else:
            sourcename = f"{hgname}.exe" if iswindows else hgname
        targetname = f"{hgname}.exe" if iswindows else hgname
        log.debug("copying main mercurial binary from %s" % bindir)
        copy_to(pjoin(bindir, sourcename), pjoin(dirtocopy, targetname))
        # On Windows, debuginfo is not embedded, but stored as .pdb.
        # Copy it for better debugging.
        if iswindows:
            pdbname = pjoin(bindir, f"{hgname}.pdb")
            copy_to(pdbname, pjoin(dirtocopy, f"{hgname}.pdb"))

    def _copy_other(self, dirtocopy):
        """Copy misc files, which aren't main hg codebase"""
        tocopy = {
            "contrib/editmergeps.ps1": "contrib/editmergeps.ps1",
            "contrib/editmergeps.bat": "contrib/editmergeps.bat",
            "isl-dist.tar.xz": "isl-dist.tar.xz",
        }
        for sname, tname in tocopy.items():
            source = pjoin(scriptdir, sname)
            target = pjoin(dirtocopy, tname)
            copy_to(source, target)

    def run(self):
        embdir = pjoin(scriptdir, "build", "embedded")
        ensureempty(embdir)
        ensureexists(embdir)

        # On Windows, Python shared library has to live at the same level
        # as the main project binary, since this is the location which
        # has the first priority in dynamic linker search path.
        self._copy_py_lib(embdir)

        self._copy_hg_exe(embdir)
        self._copy_other(embdir)


class BuildInteractiveSmartLog(build):
    description = "builds interactive Smartlog"

    user_options = [
        ("build-lib=", "b", "directory for compiled extension modules"),
        ("build-temp=", "t", "directory for temporary files (build by-products)"),
    ]

    def initialize_options(self):
        self.build_lib = None
        self.build_temp = None

    def finalize_options(self):
        self.set_undefined_options(
            "build",
            ("build_lib", "build_lib"),
            ("build_temp", "build_temp"),
        )

    def run(self):
        # External path to addons/
        addons_path = os.path.realpath(pjoin(scriptdir, "..", "..", "addons"))
        if not os.path.isdir(addons_path):
            # Internal path to addons/
            addons_path = os.path.realpath(pjoin(scriptdir, "..", "addons"))
            if not os.path.isdir(addons_path):
                # Currently, the addons/ folder is not available at this
                # revision in the Sapling repo.
                return

        env = None
        if havefb and "YARN" not in os.environ:
            env = {
                **os.environ,
                "YARN": os.path.realpath(
                    pjoin(
                        scriptdir,
                        "../../../xplat/third-party/yarn",
                        iswindows and "yarn.bat" or "yarn",
                    )
                ),
            }

        subprocess.run(
            [sys.executable, "build-tar.py", "-o", pjoin(scriptdir, "isl-dist.tar.xz")],
            check=True,
            cwd=addons_path,
            env=env,
        )


class hginstall(install):
    user_options = install.user_options + [
        ("old-and-unmanageable", None, "noop, present for eggless setuptools compat"),
        (
            "single-version-externally-managed",
            None,
            "noop, present for eggless setuptools compat",
        ),
    ]

    # Also helps setuptools not be sad while we refuse to create eggs.
    single_version_externally_managed = True

    def get_sub_commands(self):
        # Screen out egg related commands to prevent egg generation.  But allow
        # mercurial.egg-info generation, since that is part of modern
        # packaging.
        excl = set(["bdist_egg"])
        return filter(lambda x: x not in excl, install.get_sub_commands(self))


class hginstallscripts(install_scripts):
    """
    This is a specialization of install_scripts that replaces the @LIBDIR@ with
    the configured directory for modules. If possible, the path is made relative
    to the directory for scripts.
    """

    def initialize_options(self):
        install_scripts.initialize_options(self)

        self.install_lib = None

    def finalize_options(self):
        install_scripts.finalize_options(self)
        self.set_undefined_options("install", ("install_lib", "install_lib"))

    def run(self):
        install_scripts.run(self)

        # It only makes sense to replace @LIBDIR@ with the install path if
        # the install path is known. For wheels, the logic below calculates
        # the libdir to be "../..". This is because the internal layout of a
        # wheel archive looks like:
        #
        #   mercurial-3.6.1.data/scripts/hg
        #   mercurial/__init__.py
        #
        # When installing wheels, the subdirectories of the "<pkg>.data"
        # directory are translated to system local paths and files therein
        # are copied in place. The mercurial/* files are installed into the
        # site-packages directory. However, the site-packages directory
        # isn't known until wheel install time. This means we have no clue
        # at wheel generation time what the installed site-packages directory
        # will be. And, wheels don't appear to provide the ability to register
        # custom code to run during wheel installation. This all means that
        # we can't reliably set the libdir in wheels: the default behavior
        # of looking in sys.path must do.

        if (
            os.path.splitdrive(self.install_dir)[0]
            != os.path.splitdrive(self.install_lib)[0]
        ):
            # can't make relative paths from one drive to another, so use an
            # absolute path instead
            libdir = self.install_lib
        else:
            common = os.path.commonprefix((self.install_dir, self.install_lib))
            rest = self.install_dir[len(common) :]
            uplevel = len([n for n in os.path.split(rest) if n])

            libdir = uplevel * (".." + os.sep) + self.install_lib[len(common) :]

        for outfile in self.outfiles:
            with open(outfile, "rb") as fp:
                data = fp.read()

            # skip binary files
            if b"\0" in data:
                continue

            # During local installs, the shebang will be rewritten to the final
            # install path. During wheel packaging, the shebang has a special
            # value.
            if data.startswith(b"#!python"):
                log.info(
                    "not rewriting @LIBDIR@ in %s because install path "
                    "not known" % outfile
                )
                continue

            data = data.replace(b"@LIBDIR@", libdir.encode("unicode_escape"))
            with open(outfile, "wb") as fp:
                fp.write(data)


cmdclass = {
    "build": hgbuild,
    "build_mo": hgbuildmo,
    "build_scripts": hgbuildscripts,
    "install": hginstall,
    "install_scripts": hginstallscripts,
    "build_rust_ext": BuildRustExt,
    "build_embedded": buildembedded,
    "install_rust_ext": InstallRustExt,
    "build_interactive_smartlog": BuildInteractiveSmartLog,
}

packages = [
    os.path.dirname(p).replace("/", ".").replace("\\", ".")
    for p in glob.glob("sapling/**/__init__.py", recursive=True)
] + [
    "saplingnative",
    "ghstack",
]

common_depends = [
    "sapling/bitmanipulation.h",
    "sapling/compat.h",
    "sapling/cext/util.h",
]


def get_env_path_list(var_name, default=None):
    """Get a path list from an environment variable.  The variable is parsed as
    a colon-separated list."""
    value = os.environ.get(var_name)
    if not value:
        return default
    return value.split(os.path.pathsep)


def filter_existing_dirs(dirs):
    """Filters the given list and keeps only existing directory names."""
    return [d for d in dirs if os.path.isdir(d)]


def distutils_dir_name(dname):
    """Returns the name of a distutils build directory"""
    if dname == "scripts":
        # "scripts" dir in distutils builds does not contain
        # any platform info, just the Python version
        f = "{dirname}-{version}"
    else:
        f = "{dirname}.{platform}-{version}"
    return f.format(
        dirname=dname,
        platform=get_platform(),
        version=("%s.%s" % sys.version_info[:2]),
    )


# Historical default values.
# We should perhaps clean these up in the future after verifying that it
# doesn't break the build on any platforms.
#
# The /usr/local/* directories shouldn't actually be needed--the compiler
# should already use these directories when appropriate (e.g., if we are
# using the standard system compiler that has them in its default paths).
#
# The /opt/local paths may be necessary on Darwin builds.
include_dirs = get_env_path_list("INCLUDE_DIRS")
if include_dirs is None:
    if iswindows:
        include_dirs = []
    else:
        include_dirs = filter_existing_dirs(
            ["/usr/local/include", "/opt/local/include", "/opt/homebrew/include/"]
        )
include_dirs = [".", "../.."] + include_dirs

library_dirs = get_env_path_list("LIBRARY_DIRS")
if library_dirs is None:
    if iswindows:
        library_dirs = []
    else:
        library_dirs = filter_existing_dirs(
            ["/usr/local/lib", "/opt/local/lib", "/opt/homebrew/lib/"]
        )
    library_dirs.append("build/" + distutils_dir_name("lib"))

extra_libs = get_env_path_list("EXTRA_LIBS", [])

havefanotify = (
    not ossbuild
    and "linux" in sys.platform
    and cancompile(
        new_compiler(),
        """
     #include <fcntl.h>
     #include <sys/fanotify.h>
     int main() { return fanotify_init(0, 0); }""",
    )
)


extmodules = []


libraries = []

# let's add EXTRA_LIBS to every buildable
for extmodule in extmodules:
    extmodule.libraries.extend(extra_libs)
for libname, libspec in libraries:
    libspec["libraries"] = libspec.get("libraries", []) + extra_libs

try:
    from distutils import cygwinccompiler

    # the -mno-cygwin option has been deprecated for years
    mingw32compilerclass = cygwinccompiler.Mingw32CCompiler

    class HackedMingw32CCompiler(cygwinccompiler.Mingw32CCompiler):
        def __init__(self, *args, **kwargs):
            mingw32compilerclass.__init__(self, *args, **kwargs)
            for i in "compiler compiler_so linker_exe linker_so".split():
                try:
                    getattr(self, i).remove("-mno-cygwin")
                except ValueError:
                    pass

    cygwinccompiler.Mingw32CCompiler = HackedMingw32CCompiler
except ImportError:
    # the cygwinccompiler package is not available on some Python
    # distributions like the ones from the optware project for Synology
    # DiskStation boxes
    class HackedMingw32CCompiler:
        pass


if os.name == "nt":
    # Allow compiler/linker flags to be added to Visual Studio builds.  Passing
    # extra_link_args to distutils.extensions.Extension() doesn't have any
    # effect.
    from distutils import msvccompiler

    msvccompilerclass = msvccompiler.MSVCCompiler

    class HackedMSVCCompiler(msvccompiler.MSVCCompiler):
        def initialize(self):
            msvccompilerclass.initialize(self)
            # "warning LNK4197: export 'func' specified multiple times"
            self.ldflags_shared.append("/ignore:4197")
            self.ldflags_shared.append("/DEBUG")
            self.ldflags_shared_debug.append("/ignore:4197")
            self.compile_options.append("/Z7")

    msvccompiler.MSVCCompiler = HackedMSVCCompiler

packagedata = {
    "sapling": [
        "mercurial/locale/*/LC_MESSAGES/hg.mo",
        "mercurial/help/*.txt",
        "mercurial/help/internals/*.txt",
        "mercurial/default.d/*.rc",
        "mercurial/dummycert.pem",
    ]
}


def ordinarypath(p):
    return p and p[0] != "." and p[-1] != "~"


hgmainfeatures = (
    " ".join(
        filter(
            None,
            [
                "with_chg" if not iswindows else None,
                "fb" if havefb else None,
                "eden" if not ossbuild else None,
                "sl_oss" if ossbuild else None,
            ],
        )
    ).strip()
    or None
)

rustextmodules = []

rustextbinaries = [
    RustBinary(
        "hgmain",
        manifest="exec/hgmain/Cargo.toml",
        rename=hgname,
        features=hgmainfeatures,
        env={
            "SAPLING_VERSION": sapling_version,
            "SAPLING_VERSION_HASH": sapling_versionhash,
        },
    ),
]

skip_other_binaries = bool(os.environ.get("SAPLING_SKIP_OTHER_RUST_BINARIES"))

if not ossbuild and not skip_other_binaries:
    rustextbinaries += [
        RustBinary("mkscratch", manifest="exec/scratch/Cargo.toml"),
        RustBinary("scm_daemon", manifest="exec/scm_daemon/Cargo.toml"),
    ]

if sys.platform == "cygwin":
    print("WARNING: CYGWIN BUILD NO LONGER OFFICIALLY SUPPORTED")


setup(
    name="sapling",
    version="0.0.1",  # dummy version to make setuptools happy
    author="Olivia Mackall and many others",
    url="https://sapling-scm.com/",
    description=(
        "Sapling SCM is a cross-platform, highly scalable, Git-compatible source control system."
    ),
    long_description=(
        "It aims to provide both user-friendly and powerful interfaces for users, as "
        "well as extreme scalability to deal with repositories containing many millions "
        "of files and many millions of commits."
    ),
    license="GNU GPLv2 or any later version",
    classifiers=[
        "Environment :: Console",
        "Intended Audience :: Developers",
        "Intended Audience :: System Administrators",
        "License :: OSI Approved :: GNU General Public License (GPL)",
        "Operating System :: Microsoft :: Windows",
        "Operating System :: OS Independent",
        "Operating System :: POSIX",
        "Programming Language :: C",
        "Programming Language :: Python",
        "Topic :: Software Development :: Version Control",
    ],
    packages=packages,
    ext_modules=extmodules,
    libraries=libraries,
    rust_ext_binaries=rustextbinaries,
    rust_ext_modules=rustextmodules,
    package_data=packagedata,
    cmdclass=cmdclass,
    options={
        "bdist_mpkg": {
            "zipdist": False,
            "license": "COPYING",
            "readme": "contrib/macosx/Readme.html",
            "welcome": "contrib/macosx/Welcome.html",
        }
    },
)
