# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# This is the mercurial setup script.
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
import ctypes
import ctypes.util
import errno
import glob
import hashlib
import re
import shutil
import socket
import stat
import struct
import subprocess
import tarfile
import tempfile
import time
import zipfile

if sys.version_info.major == 2:
    raise RuntimeError("This setup.py is Python 3 only!")

PY_VERSION = os.environ.get("PY_VERSION")
if PY_VERSION is None:
    if os.name == "nt":
        PY_VERSION = "39"
    else:
        PY_VERSION = "38"

ossbuild = bool(os.environ.get("SAPLING_OSS_BUILD"))


def ensureenv():
    """Load build/env's as environment variables.

    If build/env has specified a different set of environment variables,
    restart the current command. Otherwise do nothing.
    """
    if not os.path.exists("build/env"):
        return
    with open("build/env", "r") as f:
        env = dict(l.split("=", 1) for l in f.read().splitlines() if "=" in l)
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

# rust-cpython uses this to collect Python information
os.environ["PYTHON_SYS_EXECUTABLE"] = sys.executable


def filter(f, it):
    return list(__builtins__.filter(f, it))


ispypy = "PyPy" in sys.version


import distutils
from distutils import file_util, log
from distutils.ccompiler import new_compiler
from distutils.command.build import build
from distutils.command.build_ext import build_ext
from distutils.command.build_py import build_py
from distutils.command.build_scripts import build_scripts
from distutils.command.install import install
from distutils.command.install_lib import install_lib
from distutils.command.install_scripts import install_scripts
from distutils.core import Command, Extension, setup
from distutils.dir_util import copy_tree
from distutils.dist import Distribution
from distutils.errors import CCompilerError, DistutilsExecError
from distutils.spawn import find_executable, spawn
from distutils.sysconfig import get_config_var
from distutils.version import StrictVersion

from distutils_rust import BuildRustExt, InstallRustExt, RustBinary, RustExtension

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
        copy_tree(source, target)
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


class hgcommand(object):
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
        )
    ]
    return b"\n".join(b"  " + e for e in err)


def findhg():
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
    hgcmd = ["hg"]
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
    hgcmd = [sys.executable, "hg"]
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


hg = None if ossbuild else findhg()


def hgtemplate(template, cast=None):
    if not hg:
        return None
    result = hg.run(["log", "-r.", "-T", template]).decode("utf-8")
    if result and cast:
        result = cast(result)
    return result


def pickversion():
    # Respect SAPLING_VERSION set by GitHub workflows.
    env_version = os.environ.get("SAPLING_VERSION")
    if env_version:
        return env_version
    # New version system: YYMMDD_HHmmSS_hash
    # This is duplicated a bit from build_rpm.py:auto_release_str()
    template = r'{sub("([:+-]|\d\d\d\d$)", "",date|isodatesec)} {node|short}'
    # if hg is not found, fallback to a fixed version
    out = hgtemplate(template) or ""
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
sapling_versionhashb = sapling_versionhash.encode("ascii")

chgcflags = ["-std=c99", "-D_GNU_SOURCE", "-DHAVE_VERSIONHASH", "-I%s" % builddir]
versionhashpath = pjoin(builddir, "versionhash.h")
write_if_changed(
    versionhashpath, b"#define HGVERSIONHASH %sULL\n" % sapling_versionhashb
)

write_if_changed(
    "edenscm/__version__.py",
    b"".join(
        [
            b"# this file is autogenerated by setup.py\n"
            b'version = "%s"\n' % sapling_versionb,
            b"versionhash = %s\n" % sapling_versionhashb,
        ]
    ),
)


def writebuildinfoc():
    """Write build/buildinfo.c"""
    commithash = hgtemplate("{node}")
    commitunixtime = hgtemplate('{sub("[^0-9].*","",date)}', cast=int)

    # Search 'extractBuildInfoFromELF' in fbcode for supported fields.
    buildinfo = {
        "Host": socket.gethostname(),
        "PackageName": os.environ.get("RPM_PACKAGE_NAME")
        or os.environ.get("PACKAGE_NAME"),
        "PackageRelease": os.environ.get("RPM_PACKAGE_RELEASE")
        or os.environ.get("PACKAGE_RELEASE"),
        "PackageVersion": os.environ.get("RPM_PACKAGE_VERSION")
        or os.environ.get("PACKAGE_VERSION"),
        "Path": os.getcwd(),
        "Platform": os.environ.get("RPM_OS"),
        "Revision": commithash,
        "RevisionCommitTimeUnix": commitunixtime,
        "TimeUnix": int(time.time()),
        "UpstreamRevision": commithash,
        "UpstreamRevisionCommitTimeUnix": commitunixtime,
        "User": os.environ.get("USER"),
    }

    buildinfosrc = """
#include <stdio.h>
#include <time.h>
"""
    for name, value in sorted(buildinfo.items()):
        if isinstance(value, str):
            buildinfosrc += 'const char *BuildInfo_k%s = "%s";\n' % (
                name,
                value.replace('"', '\\"'),
            )
        elif isinstance(value, int):
            # The only usage of int is timestamp
            buildinfosrc += "const time_t BuildInfo_k%s = %d;\n" % (name, value)

    buildinfosrc += """
/* This function keeps references of the symbols and prevents them from being
 * optimized out if this function is used. */
void print_buildinfo() {
"""
    for name, value in sorted(buildinfo.items()):
        if isinstance(value, str):
            buildinfosrc += (
                '  fprintf(stderr, "%(name)s: %%s (at %%p)\\n", BuildInfo_k%(name)s, BuildInfo_k%(name)s);\n'
                % {"name": name}
            )
        elif isinstance(value, int):
            buildinfosrc += (
                '  fprintf(stderr, "%(name)s: %%lu (at %%p)\\n", (long unsigned)BuildInfo_k%(name)s, &BuildInfo_k%(name)s) ;\n'
                % {"name": name}
            )
    buildinfosrc += """
}
"""

    path = pjoin(builddir, "buildinfo.c")
    write_if_changed(path, buildinfosrc)
    return path


# If NEED_BUILDINFO is set, write buildinfo.
# For rpmbuild, imply NEED_BUILDINFO.
needbuildinfo = bool(
    os.environ.get("NEED_BUILDINFO", "RPM_PACKAGE_NAME" in os.environ and not ossbuild)
)

if needbuildinfo:
    buildinfocpath = writebuildinfoc()


class asset(object):
    def __init__(self, name=None, url=None, destdir=None, version=0):
        """Declare an asset to download

        When building inside fbsource, look up the name from the LFS list, and
        use internal LFS to download it. Outside fbsource, use the specified
        URL to download it.

        name: File name matching the internal lfs-pointers file
        url:  External url. If not provided, external build will fail.
        destdir: Destination directory name, excluding build/
        version: Number to invalidate existing downloaded cache. Useful when
                 content has changed while neither name nor url was changed.

        Files will be downloaded to build/<name> and extract to
        build/<destdir>.
        """
        if name is None and url:
            # Try to infer the name from url
            name = os.path.basename(url)
        assert name is not None
        if destdir is None:
            # Try to infer it from name
            destdir = os.path.splitext(name)[0]
            if destdir.endswith(".tar"):
                destdir = destdir[:-4]
        assert name != destdir, "name (%s) and destdir cannot be the same" % name
        self.name = name
        self.url = url
        self.destdir = destdir
        self.version = version

    def ensureready(self):
        """Download and extract the asset to self.destdir. Return full path of
        the directory containing extracted files.
        """
        if not self._isready():
            self._download()
            self._extract()
            self._markready()
        assert self._isready(), "%r should be ready now" % self
        return pjoin(builddir, self.destdir)

    def _download(self):
        destpath = pjoin(builddir, self.name)
        if havefb:
            # via internal LFS utlity
            lfspypath = os.environ.get(
                "LFSPY_PATH", pjoin(scriptdir, "../../tools/lfs/lfs.py")
            )
            args = [sys.executable, lfspypath, "-q", "download", destpath]
        else:
            # via external URL
            assert self.url, "Cannot download %s - no URL provided" % self.name
            args = ["curl", "-L", self.url, "-o", destpath]
            cert_dir = os.environ.get("SSL_CERT_DIR", None)
            if cert_dir:
                args += ["--capath", cert_dir]
            ca_file = os.environ.get("SSL_CERT_FILE", None)
            if ca_file:
                args += ["--cacert", ca_file]
            print(f"downloading with: {' '.join(args)}")
        subprocess.check_call(args)

    def _extract(self):
        destdir = self.destdir
        srcpath = pjoin(builddir, self.name)
        destpath = pjoin(builddir, destdir)
        assert os.path.isfile(srcpath), "%s is not downloaded properly" % srcpath
        ensureempty(destpath)

        if srcpath.endswith(".tar.gz"):
            with tarfile.open(srcpath, "r") as f:
                # Be smarter: are all paths in the tar already starts with
                # destdir? If so, strip it.
                prefix = destdir + "/"
                if all((name + "/").startswith(prefix) for name in f.getnames()):
                    destpath = os.path.dirname(destpath)
                f.extractall(destpath)
        elif srcpath.endswith(".zip") or srcpath.endswith(".whl"):
            with zipfile.ZipFile(srcpath, "r") as f:
                # Same as above. Strip the destdir name if all entries have it.
                prefix = destdir + "/"
                if all((name + "/").startswith(prefix) for name in f.namelist()):
                    destpath = os.path.dirname(destpath)
                f.extractall(destpath)
        else:
            raise RuntimeError("don't know how to extract %s" % self.name)

    def hash(self):
        sha = hashlib.sha256()
        sha.update((self.name or "").encode("utf8"))
        sha.update((self.url or "").encode("utf8"))
        sha.update(b"%i" % self.version)
        return sha.hexdigest()

    def _isready(self):
        try:
            with open(self._readypath) as f:
                return f.read() == self.hash()
        except Exception:
            return False

    def _markready(self):
        with open(self._readypath, "w") as f:
            f.write("%s" % self.hash())

    @property
    def _readypath(self):
        return pjoin(builddir, self.destdir, ".ready")


class fbsourcepylibrary(asset):
    """An asset available from inside fbsource only.
    This is used to pull in python libraries from fbsource
    and process them to fit our installation requirements.
    "name" specifies the python package name for the library.
    "path" is its location relative to the current location
    in fbsource.
    "excludes" is a list of paths relative to "path" that should
    be excluded from the installation image."""

    def __init__(self, name, path, excludes=None):
        assert (
            havefb or isgetdepsbuild or ossbuild
        ), "can only build this internally at FB or via the getdeps.py script"
        topname = "fbsource-" + name.replace("/", ".")
        super(fbsourcepylibrary, self).__init__(name=name, destdir=topname)
        self.path = path
        self.excludes = excludes or []
        self.pkgname = name

    def _download(self):
        # Nothing to download; already present in fbsource
        pass

    def _extract(self):
        # Extraction is really just copying files.  We generate
        # a directory with a name matching the pkgname as an intermediate
        # step so that it resolves correctly at import time
        topdir = pjoin(builddir, self.destdir)
        ensureexists(topdir)
        destpath = pjoin(topdir, self.pkgname)
        if os.path.exists(destpath):
            shutil.rmtree(destpath)
        shutil.copytree(self.path, destpath)
        for root, dirs, files in os.walk(topdir):
            if "__init__.py" not in files:
                with open(pjoin(root, "__init__.py"), "w") as f:
                    f.write("\n")
        for name in self.excludes:
            tryunlink(pjoin(topdir, name))


class edenpythrift(asset):
    """In this context, we are only interested in the `py/` subdir,
    so we extract only that dir"""

    def _extract(self):
        destdir = self.destdir
        srcpath = pjoin(builddir, self.name)
        destpath = pjoin(builddir, destdir)
        assert os.path.isfile(srcpath), "%s is not downloaded properly" % srcpath
        ensureempty(destpath)

        with zipfile.ZipFile(srcpath, "r") as f:
            for name in f.namelist():
                if name.startswith("py/"):
                    targetname = name[3:]  # strip off `py/` prefix
                    ensureexists(os.path.dirname(pjoin(destpath, targetname)))
                    with open(pjoin(destpath, targetname), "wb") as target:
                        target.write(f.read(name))


class thriftasset(asset):
    def __init__(self, name, sourcemap, destdir=None):
        assert isgetdepsbuild, "can only build this only via the getdeps.py script"

        if destdir is None:
            destdir = name + "-py"
        assert name != destdir, "name (%s) and destdir cannot be the same" % name

        super(thriftasset, self).__init__(name=name, destdir=destdir)
        self.sourcemap = sourcemap

    def _download(self):
        for source, dest in self.sourcemap.items():
            copy_to(pjoin(scriptdir, source), pjoin(builddir, self.name, dest))

    def _extract(self):
        thriftdir = pjoin(builddir, self.name)
        destdir = pjoin(builddir, self.destdir)
        for thriftdest in self.sourcemap.values():
            thriftfile = pjoin(thriftdir, thriftdest)
            subprocess.check_call(
                [
                    os.environ["THRIFT"],
                    "-I",
                    thriftdir,
                    "-gen",
                    "py:new_style",
                    "-out",
                    destdir,
                    thriftfile,
                ]
            )

    def __hash__(self):
        thriftdir = pjoin(builddir, self.name)
        hasher = hashlib.sha1()

        for thriftdest in sorted(self.sourcemap.values()):
            thriftfile = pjoin(thriftdir, thriftdest)
            if os.path.exists(thriftfile):
                with open(thriftfile, "rb") as f:
                    hasher.update(f.read())
        return int(hasher.hexdigest(), 16)


class fetchbuilddeps(Command):
    description = "download build depencencies"
    user_options = []

    pyassets = [
        asset(url=url)
        for url in [
            "https://files.pythonhosted.org/packages/22/a6/858897256d0deac81a172289110f31629fc4cee19b6f01283303e18c8db3/ptyprocess-0.7.0-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/39/7b/88dbb785881c28a102619d46423cb853b46dbccc70d3ac362d99773a78ce/pexpect-4.8.0-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/23/6a/210816c943c9aeeb29e4e18a298f14bf0e118fe222a23e13bfcc2d41b0a4/ipython-7.16.1-py3-none-any.whl",
            "https://files.pythonhosted.org/packages/3d/57/4d9c9e3ae9a255cd4e1106bb57e24056d3d0709fc01b2e3e345898e49d5b/simplegeneric-0.8.1.zip",
            "https://files.pythonhosted.org/packages/44/6f/7120676b6d73228c96e17f1f794d8ab046fc910d781c8d151120c3f1569e/toml-0.10.2-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/44/98/5b86278fbbf250d239ae0ecb724f8572af1c91f4a11edf4d36a206189440/colorama-0.4.4-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/4c/1c/ff6546b6c12603d8dd1070aa3c3d273ad4c07f5771689a7b69a550e8c951/backcall-0.2.0-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/4e/78/56aa1b5f4d8ac548755ae767d84f0be54fdd9d404197a3d9e4659d272348/setuptools-57.0.0-py3-none-any.whl",
            "https://files.pythonhosted.org/packages/59/7c/e39aca596badaf1b78e8f547c807b04dae603a433d3e7a7e04d67f2ef3e5/wcwidth-0.2.5-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/87/61/2dfea88583d5454e3a64f9308a686071d58d59a55db638268a6413e1eb6d/prompt_toolkit-2.0.10-py3-none-any.whl",
            "https://files.pythonhosted.org/packages/6a/36/b1b9bfdf28690ae01d9ca0aa5b0d07cb4448ac65fb91dc7e2d094e3d992f/decorator-5.0.9-py3-none-any.whl",
            "https://files.pythonhosted.org/packages/9a/41/220f49aaea88bc6fa6cba8d05ecf24676326156c23b991e80b3f2fc24c77/pickleshare-0.7.5-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/a6/c9/be11fce9810793676017f79ffab3c6cb18575844a6c7b8d4ed92f95de604/Pygments-2.9.0-py3-none-any.whl",
            "https://files.pythonhosted.org/packages/ca/ab/872a23e29cec3cf2594af7e857f18b687ad21039c1f9b922fac5b9b142d5/traitlets-4.3.3-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/d9/5a/e7c31adbe875f2abbb91bd84cf2dc52d792b5a01506781dbcf25c91daf11/six-1.16.0-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/fa/bc/9bd3b5c2b4774d5f33b2d544f1460be9df7df2fe42f352135381c347c69a/ipython_genutils-0.2.0-py2.py3-none-any.whl",
            "https://files.pythonhosted.org/packages/fc/56/9f67dcd4a4b9960373173a31be1b8c47fe351a1c9385677a7bdd82810e57/ipdb-0.13.9.tar.gz",
        ]
    ]

    # These pyassets are eden and thrift related. We don't use those in open
    # source for now.
    if not ossbuild:
        pyassets += [
            fbsourcepylibrary(
                "thrift",
                "../../thrift/lib/py"
                if havefb
                else f"{dep_build_dir}/fbthrift/thrift/lib/py/thrift_py.lib_install/thrift_py/thrift",
                excludes=[
                    "thrift/util/asyncio.py",
                    "thrift/util/inspect.py",
                    "thrift/server/TAsyncioServer.py",
                    "thrift/server/test/TAsyncioServerTest.py",
                    "thrift/util/tests/__init__.py",
                ],
            ),
            fbsourcepylibrary("eden", "../../eden/fs/py/eden"),
        ]
        pyassets += (
            [
                edenpythrift(
                    name="eden-rust-deps-2f6da57cdd616a6f0e5d1b9fcd7f0349f4edcf47.zip"
                )
            ]
            if havefb
            else [
                thriftasset(
                    name="eden-thrift",
                    sourcemap={
                        "../../eden/fs/service/eden.thrift": "eden/fs/service/eden.thrift",
                        "../../eden/fs/config/eden_config.thrift": "eden/fs/config/eden_config.thrift",
                        f"{dep_install_dir}/fb303/include/thrift-files/fb303/thrift/fb303_core.thrift": "fb303/thrift/fb303_core.thrift",
                    },
                )
            ]
        )

    assets = pyassets

    def initialize_options(self):
        pass

    def finalize_options(self):
        pass

    def run(self):
        for item in self.assets:
            item.ensureready()


class hgbuild(build):
    # Insert hgbuildmo first so that files in mercurial/locale/ are found
    # when build_py is run next. Also, normally build_scripts is automatically
    # a subcommand of build iff the `scripts` argumnent or `setup` is present
    # Since we removed that argument, let's add the subcommand explicitly
    sub_commands = [("build_mo", None), ("build_scripts", None)] + build.sub_commands


class hgbuildmo(build):

    description = "build translations (.mo files)"

    def run(self):
        if not find_executable("msgfmt"):
            self.warn(
                "could not find msgfmt executable, no translations " "will be built"
            )
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
            mobuildfile = join("edenscm", mofile)
            cmd = ["msgfmt", "-v", "-o", mobuildfile, pofile]
            if sys.platform != "sunos5":
                # msgfmt on Solaris does not know about -c
                cmd.append("-c")
            self.mkpath(join("edenscm", modir))
            self.make_file([pofile], mobuildfile, spawn, (cmd,))


class hgdist(Distribution):
    pure = False
    cffi = ispypy

    global_options = Distribution.global_options + [
        ("pure", None, "use pure (slow) Python " "code instead of C extensions")
    ]

    def has_ext_modules(self):
        # self.ext_modules is emptied in hgbuildpy.finalize_options which is
        # too late for some cases
        return not self.pure and Distribution.has_ext_modules(self)


# This is ugly as a one-liner. So use a variable.
buildextnegops = dict(getattr(build_ext, "negative_options", {}))


class hgbuildext(build_ext):
    def build_extensions(self):
        fetchbuilddeps(self.distribution).run()
        return build_ext.build_extensions(self)

    def build_extension(self, ext):
        try:
            build_ext.build_extension(self, ext)
        except CCompilerError:
            if not getattr(ext, "optional", False):
                raise
            log.warn("Failed to build optional extension '%s' (skipping)", ext.name)


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

    def _process_hg_exts(self, dirforexts):
        """Prepare Mercurail native Python extensions

        This just copies edenscmnative/ to the destination."""
        parentdir = scriptdir
        if not self.local_bins:
            # copy .pyd's from ./build/lib.win-amd64/, not from ./
            parentdir = pjoin(scriptdir, "build", distutils_dir_name("lib"))
        copy_to(pjoin(parentdir, "edenscmnative"), pjoin(dirforexts, "edenscmnative"))
        # copy the conch_parser extension, not living in the edenscmnative directory
        for pattern in ["*.pyd", "*.so"]:
            for path in glob.glob(pjoin(parentdir, pattern)):
                copy_to(path, dirforexts)

    def _process_isl(self, dirforisl):
        """Copy edenscm-isl, if present, to the destination.

        This is for external OSS build."""
        isldir = pjoin(scriptdir, "build", distutils_dir_name("lib"), "edenscm-isl")
        if os.path.isdir(isldir):
            copy_to(isldir, pjoin(dirforisl, "edenscm-isl"))

    def _zip_pyc_files(self, zipname, package):
        """Modify a zip archive to include our .pyc files"""
        sourcedir = pjoin(scriptdir, package)
        with zipfile.PyZipFile(zipname, "a") as z:
            # Write .py files for better traceback.
            for root, _dirs, files in os.walk(sourcedir):
                for basename in files:
                    sourcepath = pjoin(root, basename)
                    if sourcepath.endswith(".py"):
                        # relative to scriptdir
                        inzippath = sourcepath[len(scriptdir) + 1 :]
                        z.write(sourcepath, inzippath)
            # Compile and write .pyc files.
            z.writepy(sourcedir)

    def _copy_py_lib(self, dirtocopy):
        """Copy main Python shared library"""
        pyroot = os.path.realpath(pjoin(sys.executable, ".."))
        pylib = f"python{PY_VERSION}"
        pylibext = pylib + (".dll" if iswindows else ".so")
        # First priority is the python lib that lives alongside the executable
        pylibpath = pjoin(pyroot, pylibext)
        if not os.path.exists(pylibpath):
            # a fallback option
            pylibpath = ctypes.util.find_library(pylib)
        log.debug("Python dynamic library is copied from: %s" % pylibpath)
        copy_to(pylibpath, pjoin(dirtocopy, os.path.basename(pylibpath)))
        # Copy pythonXX.zip
        pyzipname = pylib + ".zip"
        pyzippath = pjoin(pyroot, pyzipname)
        if os.path.exists(pyzippath):
            copy_to(pyzippath, pjoin(dirtocopy, pyzipname))

        # Copy native python modules
        for pylibpath in glob.glob(os.path.join(pyroot, "*.pyd")):
            copy_to(pylibpath, dirtocopy)
        for pylibpath in glob.glob(os.path.join(pyroot, "*.dll")):
            copy_to(pylibpath, dirtocopy)

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
        }
        # @fb-only
            # @fb-only
        for sname, tname in tocopy.items():
            source = pjoin(scriptdir, sname)
            target = pjoin(dirtocopy, tname)
            copy_to(source, target)

    def run(self):
        embdir = pjoin(scriptdir, "build", "embedded")
        ensureempty(embdir)
        ensureexists(embdir)
        self._process_hg_exts(embdir)
        self._process_isl(embdir)

        # On Windows, Python shared library has to live at the same level
        # as the main project binary, since this is the location which
        # has the first priority in dynamic linker search path.
        self._copy_py_lib(embdir)

        # Build everything into pythonXX.zip, which is in the default sys.path.
        zippath = pjoin(embdir, f"python{PY_VERSION}.zip")
        buildpyzip(self.distribution).run(appendzippath=zippath)
        self._zip_pyc_files(zippath, "edenscm")
        self._zip_pyc_files(zippath, "ghstack")
        self._copy_hg_exe(embdir)
        self._copy_other(embdir)


class hgbuildpy(build_py):
    def finalize_options(self):
        build_py.finalize_options(self)

        if self.distribution.pure:
            self.distribution.ext_modules = []
        elif self.distribution.cffi:
            from edenscm.cffi import bdiffbuild, mpatchbuild

            exts = [
                mpatchbuild.ffi.distutils_extension(),
                bdiffbuild.ffi.distutils_extension(),
            ]
            # cffi modules go here
            if sys.platform == "darwin":
                from edenscm.cffi import osutilbuild

                exts.append(osutilbuild.ffi.distutils_extension())
            self.distribution.ext_modules = exts

    def run(self):
        basepath = os.path.join(self.build_lib, "edenscm")
        self.mkpath(basepath)

        build_py.run(self)

        buildpyzip(self.distribution).run()


class buildpyzip(Command):
    description = "generate zip for bundled dependent Python modules (ex. IPython)"
    user_options = [
        (
            "inplace",
            "i",
            "ignore build-lib and put compiled extensions into the source "
            + "directory alongside your pure Python modules",
        )
    ]
    boolean_options = ["inplace"]

    # Currently this only handles IPython. It avoids conflicts with the system
    # IPython (which might be older and have GUI dependencies that we don't
    # need). In the future this might evolve into packing the main library
    # as weel (i.e. some buildembedded logic will move here).

    def initialize_options(self):
        self.inplace = None

    def finalize_options(self):
        pass

    def run(self, appendzippath=None):
        """If appendzippath is not None, files will be appended to the given
        path. Otherwise, zippath will be a default path and recreated.
        """
        fetchbuilddeps(self.distribution).run()

        # Directories of IPython dependencies
        depdirs = [pjoin(builddir, a.destdir) for a in fetchbuilddeps.pyassets]
        if not depdirs:
            return

        if appendzippath is None:
            zippath = pjoin(builddir, "edenscmdeps3.zip")
        else:
            zippath = appendzippath

        print(f"Packaging assets into {zippath}")

        # Perform a mtime check so we can skip building if possible
        if os.path.exists(zippath):
            depmtime = max(os.stat(d).st_mtime for d in depdirs)
            zipmtime = os.stat(zippath).st_mtime
            if zipmtime > depmtime:
                return

        # Compile all (pure Python) IPython dependencies and zip them.
        if not appendzippath:
            tryunlink(zippath)
        with zipfile.PyZipFile(zippath, "a") as f:
            for asset in fetchbuilddeps.pyassets:
                # writepy only scans directories if it is a Python package
                # (ex. with __init__.py). Therefore scan the top-level
                # directories to get everything included.
                extracteddir = pjoin(builddir, asset.destdir)

                def process_top_level(top):
                    for name in os.listdir(top):
                        if name == "setup.py" or name == "__pycache__":
                            continue
                        path = pjoin(top, name)
                        if name == "src" and os.path.isdir(path):
                            # eg: the "future" tarball has a top level src dir
                            # that contains the python packages, recurse and
                            # process those.
                            process_top_level(path)
                        elif path.endswith(".py") or os.path.isdir(path):
                            f.writepy(path)

                process_top_level(extracteddir)


class buildextindex(Command):
    description = "generate prebuilt index of ext (for frozen package)"
    user_options = []
    _indexfilename = "edenscm/ext/__index__.py"

    def initialize_options(self):
        pass

    def finalize_options(self):
        pass

    def run(self):
        if os.path.exists(self._indexfilename):
            with open(self._indexfilename, "w") as f:
                f.write("# empty\n")

        # here no extension enabled, disabled() lists up everything
        code = (
            "import pprint; from edenscm import extensions; "
            "pprint.pprint(extensions.disabled())"
        )
        returncode, out, err = runcmd([sys.executable, "-c", code], localhgenv())
        if err or returncode != 0:
            raise DistutilsExecError(err)

        with open(self._indexfilename, "w") as f:
            f.write("# this file is autogenerated by setup.py\n")
            f.write("docs = ")
            f.write(out)


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
        if not ossbuild:
            raise DistutilsSetupError(
                "ISL should only built as part of the open source build"
            )

        # External path to addons/
        addons_path = os.path.realpath(pjoin(scriptdir, "..", "..", "addons"))
        if not os.path.isdir(addons_path):
            # Internal path to addons/
            addons_path = os.path.realpath(pjoin(scriptdir, "..", "addons"))
            if not os.path.isdir(addons_path):
                # Currently, the addons/ folder is not available at this
                # revision in the Sapling repo.
                return

        isl_out = os.path.realpath(pjoin(self.build_temp, "edenscm-isl"))
        ensureempty(isl_out)

        subprocess.run(
            [lookup_path("yarn"), "install", "--prefer-offline"],
            check=True,
            cwd=addons_path,
        )
        subprocess.run(
            [lookup_path("node"), "release.js", isl_out],
            check=True,
            cwd=os.path.join(addons_path, "isl"),
        )
        copy_to(isl_out, pjoin(self.build_lib, "edenscm-isl"))


def lookup_path(cmd: str) -> str:
    r"""Use PATH to resolve `cmd` to an absolute path.

    `subprocess` has trouble finding executables on Windows, such as
    when running `yarn` when it is in your PATH as `C:\somewhere\yarn.CMD`.
    Here we use `shutil.which` to pre-expand the executable path, which works better."""
    found = shutil.which(cmd)
    if not found:
        raise RuntimeError(f"Could not find '{cmd}' in path")
    return found


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


class hginstalllib(install_lib):
    """
    This is a specialization of install_lib that replaces the copy_file used
    there so that it supports setting the mode of files after copying them,
    instead of just preserving the mode that the files originally had.  If your
    system has a umask of something like 027, preserving the permissions when
    copying will lead to a broken install.

    Note that just passing keep_permissions=False to copy_file would be
    insufficient, as it might still be applying a umask.
    """

    def run(self):
        realcopyfile = file_util.copy_file

        def copyfileandsetmode(*args, **kwargs):
            src, dst = args[0], args[1]
            dst, copied = realcopyfile(*args, **kwargs)
            if copied:
                st = os.stat(src)
                # Persist executable bit (apply it to group and other if user
                # has it)
                if st[stat.ST_MODE] & stat.S_IXUSR:
                    setmode = int("0755", 8)
                else:
                    setmode = int("0644", 8)
                m = stat.S_IMODE(st[stat.ST_MODE])
                m = (m & ~int("0777", 8)) | setmode
                os.chmod(dst, m)

        file_util.copy_file = copyfileandsetmode
        try:
            install_lib.run(self)
            self._installpyzip()
        finally:
            file_util.copy_file = realcopyfile

    def _installpyzip(self):
        for src, dst in [("edenscmdeps3.zip", "edenscmdeps3.zip")]:
            srcpath = pjoin(builddir, src)
            dstpath = pjoin(self.install_dir, dst)
            file_util.copy_file(srcpath, dstpath)


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
    "fetch_build_deps": fetchbuilddeps,
    "build": hgbuild,
    "build_mo": hgbuildmo,
    "build_ext": hgbuildext,
    "build_py": hgbuildpy,
    "build_pyzip": buildpyzip,
    "build_scripts": hgbuildscripts,
    "build_extindex": buildextindex,
    "install": hginstall,
    "install_lib": hginstalllib,
    "install_scripts": hginstallscripts,
    "build_rust_ext": BuildRustExt,
    "build_embedded": buildembedded,
    "install_rust_ext": InstallRustExt,
    "build_interactive_smartlog": BuildInteractiveSmartLog,
}

packages = [
    os.path.dirname(p).replace("/", ".").replace("\\", ".")
    for p in glob.glob("edenscm/**/__init__.py", recursive=True)
] + [
    "edenscmnative",
    "ghstack",
]

common_depends = [
    "edenscm/bitmanipulation.h",
    "edenscm/compat.h",
    "edenscm/cext/util.h",
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
        dirname=dname, platform=distutils.util.get_platform(), version=sys.version[:3]
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

osutil_cflags = []

# platform specific macros
for plat, func in [("bsd", "setproctitle")]:
    if re.search(plat, sys.platform) and hasfunction(new_compiler(), func):
        osutil_cflags.append("-DHAVE_%s" % func.upper())

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


extmodules = [
    Extension(
        "edenscmnative.base85",
        ["edenscm/cext/base85.c"],
        include_dirs=include_dirs,
        depends=common_depends,
    ),
    Extension(
        "edenscmnative.bdiff",
        ["edenscm/bdiff.c", "edenscm/cext/bdiff.c"],
        include_dirs=include_dirs,
        depends=common_depends + ["edenscm/bdiff.h"],
    ),
    Extension(
        "edenscmnative.mpatch",
        ["edenscm/mpatch.c", "edenscm/cext/mpatch.c"],
        include_dirs=include_dirs,
        depends=common_depends + ["edenscm/mpatch.h"],
    ),
    Extension(
        "edenscmnative.parsers",
        [
            "edenscm/cext/charencode.c",
            "edenscm/cext/manifest.c",
            "edenscm/cext/parsers.c",
            "edenscm/cext/revlog.c",
        ],
        include_dirs=include_dirs,
        depends=common_depends + ["edenscm/cext/charencode.h"],
    ),
    Extension(
        "edenscmnative.osutil",
        ["edenscm/cext/osutil.c"],
        include_dirs=include_dirs,
        extra_compile_args=osutil_cflags,
        depends=common_depends,
    ),
    Extension(
        "edenscmnative.xdiff",
        sources=[
            "lib/third-party/xdiff/xdiffi.c",
            "lib/third-party/xdiff/xprepare.c",
            "lib/third-party/xdiff/xutils.c",
            "edenscm/cext/xdiff.c",
        ],
        include_dirs=include_dirs,
        depends=common_depends
        + [
            "lib/third-party/xdiff/xdiff.h",
            "lib/third-party/xdiff/xdiffi.h",
            "lib/third-party/xdiff/xinclude.h",
            "lib/third-party/xdiff/xmacros.h",
            "lib/third-party/xdiff/xprepare.h",
            "lib/third-party/xdiff/xtypes.h",
            "lib/third-party/xdiff/xutils.h",
        ],
    ),
    Extension(
        "edenscmnative.bser",
        sources=["edenscm/ext/extlib/pywatchman/bser.c"],
        include_dirs=include_dirs,
    ),
]


def cythonize(*args, **kwargs):
    """Proxy to Cython.Build.cythonize. Download Cython on demand."""
    cythonsrc = asset(
        url="https://files.pythonhosted.org/packages/4c/76/1e41fbb365ad20b6efab2e61b0f4751518444c953b390f9b2d36cf97eea0/Cython-0.29.32.tar.gz"
    )
    path = cythonsrc.ensureready()
    sys.path.insert(0, path)

    from Cython.Build import cythonize

    return cythonize(*args, **kwargs)


# Cython modules
# see http://cython.readthedocs.io/en/latest/src/reference/compilation.html
cythonopts = {"unraisable_tracebacks": False, "c_string_type": "bytes"}

extmodules += cythonize(
    [
        Extension(
            "edenscmnative.traceprof",
            sources=["edenscmnative/traceprof.pyx"],
            include_dirs=include_dirs,
            extra_compile_args=filter(None, [STDCPP11, PRODUCEDEBUGSYMBOLS]),
        ),
        Extension(
            "edenscmnative.linelog",
            sources=["edenscmnative/linelog.pyx"],
            include_dirs=include_dirs,
            extra_compile_args=filter(None, [STDC99, PRODUCEDEBUGSYMBOLS]),
        ),
    ],
    compiler_directives=cythonopts,
)

libraries = [
    (
        "mpatch",
        {
            "sources": ["edenscm/mpatch.c"],
            "depends": [
                "edenscm/bitmanipulation.h",
                "edenscm/compat.h",
                "edenscm/mpatch.h",
            ],
            "include_dirs": ["."] + include_dirs,
        },
    ),
]
if needbuildinfo:
    libraries += [
        (
            "buildinfo",
            {
                "sources": [buildinfocpath],
                "extra_args": filter(None, cflags + [WALL, PIC]),
            },
        )
    ]

if not iswindows:
    libraries.append(
        (
            "chg",
            {
                "sources": [
                    "contrib/chg/chg.c",
                    "contrib/chg/hgclient.c",
                    "contrib/chg/procutil.c",
                    "contrib/chg/util.c",
                ],
                "depends": [versionhashpath],
                "include_dirs": ["contrib/chg"] + include_dirs,
                "extra_args": filter(None, cflags + chgcflags + [STDC99, WALL, PIC]),
            },
        )
    )

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
    class HackedMingw32CCompiler(object):
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
    "edenscm": [
        "mercurial/locale/*/LC_MESSAGES/hg.mo",
        "mercurial/help/*.txt",
        "mercurial/help/internals/*.txt",
        "mercurial/default.d/*.rc",
        "mercurial/dummycert.pem",
    ]
}


def ordinarypath(p):
    return p and p[0] != "." and p[-1] != "~"


# distutils expects version to be str/unicode. Converting it to
# unicode on Python 2 still works because it won't contain any
# non-ascii bytes and will be implicitly converted back to bytes
# when operated on.
setupversion = sapling_version

if os.name == "nt":
    # Windows binary file versions for exe/dll files must have the
    # form W.X.Y.Z, where W,X,Y,Z are numbers in the range 0..65535
    setupversion = sapling_version.split("+", 1)[0]

if sys.platform == "darwin" and os.path.exists("/usr/bin/xcodebuild"):
    xcode_version = runcmd(["/usr/bin/xcodebuild", "-version"], {})[1].splitlines()
    if xcode_version:
        xcode_version = xcode_version[0]
        xcode_version = xcode_version.decode("utf-8")
        xcode4 = xcode_version.startswith("Xcode") and StrictVersion(
            xcode_version.split()[1]
        ) >= StrictVersion("4.0")
        xcode51 = re.match(r"^Xcode\s+5\.1", xcode_version) is not None
    else:
        # xcodebuild returns empty on OS X Lion with XCode 4.3 not
        # installed, but instead with only command-line tools. Assume
        # that only happens on >= Lion, thus no PPC support.
        xcode4 = True
        xcode51 = False

    # XCode 4.0 dropped support for ppc architecture, which is hardcoded in
    # distutils.sysconfig
    if xcode4:
        os.environ["ARCHFLAGS"] = ""

    # XCode 5.1 changes clang such that it now fails to compile if the
    # -mno-fused-madd flag is passed, but the version of Python shipped with
    # OS X 10.9 Mavericks includes this flag. This causes problems in all
    # C extension modules, and a bug has been filed upstream at
    # http://bugs.python.org/issue21244. We also need to patch this here
    # so Mercurial can continue to compile in the meantime.
    if xcode51:
        cflags = get_config_var("CFLAGS")
        if cflags and re.search(r"-mno-fused-madd\b", cflags) is not None:
            os.environ["CFLAGS"] = os.environ.get("CFLAGS", "") + " -Qunused-arguments"


import distutils.command.build_clib
from distutils.dep_util import newer_group
from distutils.errors import DistutilsSetupError


def build_libraries(self, libraries):
    for (lib_name, build_info) in libraries:
        sources = build_info.get("sources")
        if sources is None or not isinstance(sources, (list, tuple)):
            raise DistutilsSetupError(
                "in 'libraries' option (library '%s'), "
                + "'sources' must be present and must be "
                + "a list of source filenames"
            ) % lib_name
        sources = list(sources)

        lib_path = self.compiler.library_filename(lib_name, output_dir=self.build_clib)
        depends = sources + build_info.get("depends", [])
        if not (self.force or newer_group(depends, lib_path, "newer")):
            log.debug("skipping '%s' library (up-to-date)", lib_name)
            continue
        else:
            log.info("building '%s' library" % lib_name)

        # First, compile the source code to object files in the library
        # directory.  (This should probably change to putting object
        # files in a temporary build directory.)
        macros = build_info.get("macros", [])
        include_dirs = build_info.get("include_dirs")
        extra_args = build_info.get("extra_args")
        objects = self.compiler.compile(
            sources,
            output_dir=self.build_temp,
            macros=macros,
            include_dirs=include_dirs,
            debug=self.debug,
            extra_postargs=extra_args,
        )

        # Now "link" the object files together into a static library.
        # (On Unix at least, this isn't really linking -- it just
        # builds an archive.  Whatever.)
        libraries = build_info.get("libraries", [])
        for lib in libraries:
            self.compiler.add_library(lib)
        self.compiler.create_static_lib(
            objects, lib_name, output_dir=self.build_clib, debug=self.debug
        )


distutils.command.build_clib.build_clib.build_libraries = build_libraries

hgmainfeatures = (
    " ".join(
        filter(
            None,
            [
                "buildinfo" if needbuildinfo else None,
                "with_chg" if not iswindows else None,
                "fb" if havefb else None,
                "eden" if not ossbuild else None,
                "sl_only" if ossbuild else None,
            ],
        )
    ).strip()
    or None
)

rustextmodules = [
    RustExtension(
        "conch_parser",
        package="",
        manifest="edenscmnative/conch_parser/Cargo.toml",
    )
]

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

if not ossbuild:
    rustextbinaries += [
        RustBinary("mkscratch", manifest="exec/scratch/Cargo.toml"),
        RustBinary("scm_daemon", manifest="exec/scm_daemon/Cargo.toml"),
    ]

if havefb and iswindows:
    rustextbinaries += [RustBinary("fbclone", manifest="fb/fbclone/Cargo.toml")]


setup(
    name="edenscm",
    version=setupversion,
    author="Olivia Mackall and many others",
    author_email="mercurial@mercurial-scm.org",
    url="https://mercurial-scm.org/",
    download_url="https://mercurial-scm.org/release/",
    description=(
        "Fast scalable distributed SCM (revision control, version " "control) system"
    ),
    long_description=(
        "Mercurial is a distributed SCM tool written in Python."
        " It is used by a number of large projects that require"
        " fast, reliable distributed revision control, such as "
        "Mozilla."
    ),
    license="GNU GPLv2 or any later version",
    classifiers=[
        "Development Status :: 6 - Mature",
        "Environment :: Console",
        "Intended Audience :: Developers",
        "Intended Audience :: System Administrators",
        "License :: OSI Approved :: GNU General Public License (GPL)",
        "Natural Language :: Danish",
        "Natural Language :: English",
        "Natural Language :: German",
        "Natural Language :: Italian",
        "Natural Language :: Japanese",
        "Natural Language :: Portuguese (Brazilian)",
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
    distclass=hgdist,
    options={
        "bdist_mpkg": {
            "zipdist": False,
            "license": "COPYING",
            "readme": "contrib/macosx/Readme.html",
            "welcome": "contrib/macosx/Welcome.html",
        }
    },
)
