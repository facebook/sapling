# This is the mercurial setup script.
#
# 'python setup.py install', or
# 'python setup.py --help' for more options

# isort:skip_file

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from distutils.version import LooseVersion
import contextlib
import ctypes
import ctypes.util
import errno
import imp
import glob
import os
import py_compile
import re
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import time
import zipfile


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
    p = subprocess.Popen([python] + sys.argv, env=newenv)
    sys.exit(p.wait())


ensureenv()


supportedpy = "~= 2.7"
if os.environ.get("HGALLOWPYTHON3", ""):
    # Mercurial will never work on Python 3 before 3.5 due to a lack
    # of % formatting on bytestrings, and can't work on 3.6.0 or 3.6.1
    # due to a bug in % formatting in bytestrings.
    #
    # TODO: when we actually work on Python 3, use this string as the
    # actual supportedpy string.
    supportedpy = ",".join(
        [
            ">=2.7",
            "!=3.0.*",
            "!=3.1.*",
            "!=3.2.*",
            "!=3.3.*",
            "!=3.4.*",
            "!=3.6.0",
            "!=3.6.1",
        ]
    )

import platform

if sys.version_info[0] >= 3:
    printf = eval("print")
    libdir_escape = "unicode_escape"

    def sysstr(s):
        return s.decode("latin-1")


else:
    libdir_escape = "string_escape"

    def printf(*args, **kwargs):
        f = kwargs.get("file", sys.stdout)
        end = kwargs.get("end", "\n")
        f.write(b" ".join(args) + end)

    def sysstr(s):
        return s


try:
    import Cython
except ImportError:
    havecython = False
else:
    havecython = LooseVersion(Cython.__version__) >= LooseVersion("0.22")

if not havecython:
    raise RuntimeError("Cython >= 0.22 is required")

from Cython.Build import cythonize

# Attempt to guide users to a modern pip - this means that 2.6 users
# should have a chance of getting a 4.2 release, and when we ratchet
# the version requirement forward again hopefully everyone will get
# something that works for them.
if sys.version_info < (2, 7, 0, "final"):
    pip_message = (
        "This may be due to an out of date pip. " "Make sure you have pip >= 9.0.1."
    )
    try:
        import pip

        pip_version = tuple([int(x) for x in pip.__version__.split(".")[:3]])
        if pip_version < (9, 0, 1):
            pip_message = (
                "Your pip version is out of date, please install "
                "pip >= 9.0.1. pip {} detected.".format(pip.__version__)
            )
        else:
            # pip is new enough - it must be something else
            pip_message = ""
    except Exception:
        pass
    error = """
Mercurial does not support Python older than 2.7.
Python {py} detected.
{pip}
""".format(
        py=sys.version_info, pip=pip_message
    )
    printf(error, file=sys.stderr)
    sys.exit(1)

# Solaris Python packaging brain damage
try:
    import hashlib

    sha = hashlib.sha1()
except ImportError:
    try:
        import sha

        sha.sha  # silence unused import warning
    except ImportError:
        raise SystemExit(
            "Couldn't import standard hashlib (incomplete Python install)."
        )

try:
    import zlib

    zlib.compressobj  # silence unused import warning
except ImportError:
    raise SystemExit("Couldn't import standard zlib (incomplete Python install).")

# The base IronPython distribution (as of 2.7.1) doesn't support bz2
isironpython = False
try:
    isironpython = platform.python_implementation().lower().find("ironpython") != -1
except AttributeError:
    pass

if isironpython:
    sys.stderr.write("warning: IronPython detected (no bz2 support)\n")
else:
    try:
        import bz2

        bz2.BZ2Compressor  # silence unused import warning
    except ImportError:
        raise SystemExit("Couldn't import standard bz2 (incomplete Python install).")

ispypy = "PyPy" in sys.version


# We have issues with setuptools on some platforms and builders. Until
# those are resolved, setuptools is opt-in except for platforms where
# we don't have issues.
issetuptools = os.name == "nt" or "FORCE_SETUPTOOLS" in os.environ
if issetuptools:
    from setuptools import setup
else:
    from distutils.core import setup
from distutils import log
from distutils.ccompiler import new_compiler
from distutils.core import Command, Extension
from distutils.dir_util import copy_tree
from distutils.dist import Distribution
from distutils.command.build import build
from distutils.command.build_ext import build_ext
from distutils.command.build_py import build_py
from distutils.command.build_scripts import build_scripts
from distutils.command.install import install
from distutils.command.install_lib import install_lib
from distutils.command.install_scripts import install_scripts
from distutils.spawn import spawn, find_executable
from distutils import file_util
from distutils.errors import CCompilerError, DistutilsError, DistutilsExecError
from distutils.sysconfig import get_python_inc, get_config_var
from distutils.version import StrictVersion, LooseVersion
from distutils_rust import RustExtension, RustBinary, RustVendoredCrates, BuildRustExt
import distutils

havefb = os.path.exists("fb")

iswindows = os.name == "nt"
NOOPTIMIZATION = "/Od" if iswindows else "-O0"
PRODUCEDEBUGSYMBOLS = "/DEBUG:FULL" if iswindows else "-g"
SHA1_LIBRARY = "sha1detectcoll"
SHA1LIB_DEFINE = "/DSHA1_USE_SHA1DC" if iswindows else "-DSHA1_USE_SHA1DC"
STDC99 = "" if iswindows else "-std=c99"
STDCPP0X = "" if iswindows else "-std=c++0x"
STDCPP11 = "" if iswindows else "-std=c++11"
WALL = "/Wall" if iswindows else "-Wall"
WSTRICTPROTOTYPES = None if iswindows else "-Werror=strict-prototypes"

cflags = [SHA1LIB_DEFINE]

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


def ensureexists(path):
    if not os.path.exists(path):
        os.makedirs(path)


def ensureempty(path):
    if os.path.exists(path):
        shutil.rmtree(path)
    os.makedirs(path)


def samepath(path1, path2):
    p1 = os.path.normpath(os.path.normcase(path1))
    p2 = os.path.normpath(os.path.normcase(path2))
    return p1 == p2


def copy_to(source, target):
    if os.path.isdir(source):
        copy_tree(source, target)
    else:
        ensureexists(os.path.dirname(target))
        shutil.copy2(source, target)


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


scripts = ["hg"]

# Rename hg to $HGNAME. Useful when "hg" is a wrapper calling $HGNAME (or chg).
hgname = os.environ.get("HGNAME", "hg")
if not re.match("\Ahg[.0-9a-z-]*\Z", hgname):
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
        shutil.rmtree(tmpdir)


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
            printf("stderr from '%s':" % (" ".join(cmd)), file=sys.stderr)
            printf(err, file=sys.stderr)
            return ""
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
    # if retcode == 0 and not filterhgerr(err):
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


def pickversion():
    hg = findhg()
    if not hg:
        # if hg is not found, fallback to a fixed version
        return "4.4.2"
    # New version system: YYMMDD_HHmmSS_hash
    # This is duplicated a bit from build_rpm.py:auto_release_str()
    template = '{sub("([:+-]|\d\d\d\d$)", "",date|isodatesec)} {node|short}'
    out = sysstr(hg.run(["log", "-r.", "-T", template]))
    # Some tools parse this number to figure out if they support this version of
    # Mercurial, so prepend with 4.4.2.
    # ex. 4.4.2_20180105_214829_58fda95a0202
    return "_".join(["4.4.2"] + out.split())


version = pickversion()
versionb = version
if not isinstance(versionb, bytes):
    versionb = versionb.encode("ascii")

# calculate a versionhash, which is used by chg to make sure the client
# connects to a compatible server.
versionhash = struct.unpack(">Q", hashlib.sha1(versionb).digest()[:8])[0]

write_if_changed(
    "mercurial/__version__.py",
    b"".join(
        [
            b"# this file is autogenerated by setup.py\n"
            b'version = "%s"\n' % versionb,
            b"versionhash = %s\n" % versionhash,
        ]
    ),
)

try:
    oldpolicy = os.environ.get("HGMODULEPOLICY", None)
    os.environ["HGMODULEPOLICY"] = "py"
    from mercurial import __version__

    version = __version__.version
except ImportError:
    version = "unknown"
finally:
    if oldpolicy is None:
        del os.environ["HGMODULEPOLICY"]
    else:
        os.environ["HGMODULEPOLICY"] = oldpolicy


class hgbuild(build):
    # Insert hgbuildmo first so that files in mercurial/locale/ are found
    # when build_py is run next.
    sub_commands = [("build_mo", None)] + build.sub_commands


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
            mobuildfile = join("mercurial", mofile)
            cmd = ["msgfmt", "-v", "-o", mobuildfile, pofile]
            if sys.platform != "sunos5":
                # msgfmt on Solaris does not know about -c
                cmd.append("-c")
            self.mkpath(join("mercurial", modir))
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
    user_options = build_ext.user_options + [
        ("re2-src=", None, "directory containing re2 source code")
    ]

    def initialize_options(self):
        self.re2_src = os.environ.get("RE2SRC", "")
        return build_ext.initialize_options(self)

    def build_extensions(self):
        re2path = self.re2_src
        if re2path:
            self.extensions.append(
                Extension(
                    "mercurial.thirdparty.pyre2._re2",
                    sources=[
                        "mercurial/thirdparty/pyre2/_re2.cc",
                        os.path.join(re2path, "re2/bitstate.cc"),
                        os.path.join(re2path, "re2/compile.cc"),
                        os.path.join(re2path, "re2/dfa.cc"),
                        os.path.join(re2path, "re2/filtered_re2.cc"),
                        os.path.join(re2path, "re2/mimics_pcre.cc"),
                        os.path.join(re2path, "re2/nfa.cc"),
                        os.path.join(re2path, "re2/onepass.cc"),
                        os.path.join(re2path, "re2/parse.cc"),
                        os.path.join(re2path, "re2/perl_groups.cc"),
                        os.path.join(re2path, "re2/prefilter.cc"),
                        os.path.join(re2path, "re2/prefilter_tree.cc"),
                        os.path.join(re2path, "re2/prog.cc"),
                        os.path.join(re2path, "re2/re2.cc"),
                        os.path.join(re2path, "re2/regexp.cc"),
                        os.path.join(re2path, "re2/set.cc"),
                        os.path.join(re2path, "re2/simplify.cc"),
                        os.path.join(re2path, "re2/stringpiece.cc"),
                        os.path.join(re2path, "re2/tostring.cc"),
                        os.path.join(re2path, "re2/unicode_casefold.cc"),
                        os.path.join(re2path, "re2/unicode_groups.cc"),
                        os.path.join(re2path, "util/rune.cc"),
                        os.path.join(re2path, "util/strutil.cc"),
                    ],
                    include_dirs=[re2path] + include_dirs,
                    depends=common_depends
                    + [
                        os.path.join(re2path, "re2/bitmap256.h"),
                        os.path.join(re2path, "re2/filtered_re2.h"),
                        os.path.join(re2path, "re2/prefilter.h"),
                        os.path.join(re2path, "re2/prefilter_tree.h"),
                        os.path.join(re2path, "re2/prog.h"),
                        os.path.join(re2path, "re2/re2.h"),
                        os.path.join(re2path, "re2/regexp.h"),
                        os.path.join(re2path, "re2/set.h"),
                        os.path.join(re2path, "re2/stringpiece.h"),
                        os.path.join(re2path, "re2/unicode_casefold.h"),
                        os.path.join(re2path, "re2/unicode_groups.h"),
                        os.path.join(re2path, "re2/walker-inl.h"),
                        os.path.join(re2path, "util/logging.h"),
                        os.path.join(re2path, "util/mix.h"),
                        os.path.join(re2path, "util/mutex.h"),
                        os.path.join(re2path, "util/sparse_array.h"),
                        os.path.join(re2path, "util/sparse_set.h"),
                        os.path.join(re2path, "util/strutil.h"),
                        os.path.join(re2path, "util/utf.h"),
                        os.path.join(re2path, "util/util.h"),
                    ],
                    extra_compile_args=filter(None, [STDCPP11, PRODUCEDEBUGSYMBOLS]),
                )
            )

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
        # Build chg on non-Windows platform
        if not iswindows:
            cc = new_compiler()
            chgcflags = [
                "-std=c99",
                "-D_GNU_SOURCE",
                "-DHGVERSIONHASH=%sULL" % versionhash,
            ]
            if hgname != "hg":
                chgcflags.append('-DHGPATH="%s"' % hgname)
            objs = cc.compile(
                glob.glob("contrib/chg/*.c"), debug=True, extra_preargs=chgcflags
            )
            dest = os.path.join(self.build_dir, "chg")
            cc.link_executable(objs, dest)

        return build_scripts.run(self)

    def copy_scripts(self):
        build_scripts.copy_scripts(self)
        # Rename hg to hgname
        if hgname != "hg":
            oldpath = os.path.join(self.build_dir, "hg")
            newpath = os.path.join(self.build_dir, hgname)
            os.rename(oldpath, newpath)


class buildembedded(Command):
    extsuffixes = [s[0] for s in imp.get_suffixes() if s[2] == imp.C_EXTENSION]
    srcsuffixes = [s[0] for s in imp.get_suffixes() if s[2] == imp.PY_SOURCE]
    # there should be at least one compiled python suffix, and we don't care about more
    compsuffix = [s[0] for s in imp.get_suffixes() if s[2] == imp.PY_COMPILED][0]
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

    def _is_ext_file(self, path):
        """Return True if file is a native Python extension"""
        return any(map(lambda sf: path.endswith(sf), self.extsuffixes))

    def _process_dir(self, dirtowalk, target_prefix, skipdirs, dirforpycs):
        """Py-Compile all the files recursively in dir

        `dirtowalk` - a dir to process
        `target_prefix` - put compiled files in this subdir. It has to be
                          a suffix of `dirtowalk`. For example, we would call
                          this with `dirtowalk` ".../hg/mercurial" and
                          `target_prefix` "mercurial" and expect any processed
                          files to be put in `dirforpycs`/mercurial
        `skipdirs` - do not process dirs in this list
        `dirforpycs` - a place to put .pyc files
        """
        # we need to chdir to be able to use relative source paths, see below
        # so if dirtowalk is /a/b/c/ and target_prefix is b/c/, tmpcwd would be /a/
        tmpcwd = dirtowalk[: -len(target_prefix)] if target_prefix else dirtowalk
        with chdir(tmpcwd):
            for dirpath, dirnames, filenames in os.walk(dirtowalk):
                relative_dirpath = (
                    pjoin(target_prefix, relpath(dirpath, dirtowalk)) + os.path.sep
                )  # trailing separator is needed to match skipdirs
                if any(
                    map(lambda skipdir: relative_dirpath.startswith(skipdir), skipdirs)
                ):
                    continue
                target_dir = os.path.join(dirforpycs, relative_dirpath)
                ensureexists(target_dir)
                for filename in filenames:
                    if not any(filename.endswith(suff) for suff in self.srcsuffixes):
                        continue
                    # it is important to use relative source path rather than absolute
                    # since it is what gets baked into the resulting .pyc
                    relative_source_path = os.path.join(relative_dirpath, filename)
                    compfilename = None
                    # we are certain that this loop will produce a good compiled filename
                    # because we know that filename ends with one of the suffixes
                    # from our chek above
                    for suff in self.srcsuffixes:
                        if not filename.endswith(suff):
                            continue
                        compfilename = filename[: -len(suff)] + self.compsuffix
                        break

                    targetpath = os.path.join(target_dir, compfilename)
                    py_compile.compile(relative_source_path, targetpath)

    def _process_python_install(self, dirforpycs):
        """Py-Compile all of the files in python installation and
        copy results to `dirforpycs`"""
        interp_dir = os.path.realpath(pjoin(sys.executable, ".."))

        def good_path_item(pitem):
            if not os.path.exists(pitem):
                return False
            if not os.path.isdir(item):
                return False
            if samepath(pitem, scriptdir):
                return False
            if samepath(pitem, interp_dir):
                return False
            return True

        for item in sys.path:
            if not good_path_item(item):
                continue
            skipdirs = []
            for other in sys.path:
                if other != item and other.startswith(item):
                    skipdirs.append(relpath(other, item))
            # hardcoded list of things we want to skip
            skipdirs.extend(["test", "tests", "lib2to3", "py2exe"])
            if "cython" in item.lower():
                skipdirs.append("Demos")
            skipdirs = set(skipdir + os.path.sep for skipdir in skipdirs)
            self._process_dir(item, "", skipdirs, dirforpycs)

    def _process_hg_source(self, dirforpycs):
        """Py-Compile all of the Mercurial Python files and copy
        results to `dirforpycs`"""
        hgdirs = ["mercurial", "hgdemandimport", "hgext"]
        for d, p in {pjoin(scriptdir, hgdir): hgdir for hgdir in hgdirs}.items():
            self._process_dir(d, p, set(), dirforpycs)

    def _process_hg_exts(self, dirforexts):
        """Prepare Mercurail Python extensions to be used by EmbeddedImporter

        Since all of the Mercurial extensions are in packages, we know to
        just rename the .pyd/.so files into `path.to.module.pyd`"""
        parentdir = scriptdir
        if not self.local_bins:
            # copy .pyd's from ./build/lib.win-amd64/, not from ./
            parentdir = pjoin(scriptdir, "build", distutils_dir_name("lib"))
        hgdirs = ["mercurial", "hgdemandimport", "hgext"]
        for hgdir in hgdirs:
            fulldir = pjoin(parentdir, hgdir)
            for dirpath, dirnames, filenames in os.walk(fulldir):
                for filename in filenames:
                    if not self._is_ext_file(filename):
                        continue
                    # Mercurial exts are in packages, so rename the files
                    # mercurial/cext/osutil.pyd => mercurial.cext.osutil.pyd
                    newfilename = relpath(pjoin(dirpath, filename), parentdir)
                    newfilename = newfilename.replace(os.path.sep, ".")
                    log.debug("copying %s from %s" % (dirpath, filename))
                    copy_to(
                        os.path.join(dirpath, filename),
                        os.path.join(dirforexts, newfilename),
                    )

    def _process_py_exts(self, dirforexts):
        """Prepare Python extensions to be used by EmbeddedImporter

        Unlike Mercurial files, we can't be sure about native extensions
        from the Python installation. Most of them are standalone modules
        (like lz4, all of the standard python .pyds/.sos), so we err on
        this side. If we learn about third-party packaged native
        extensions, we'll have to hardcode them here"""
        for item in sys.path:
            if not os.path.exists(item) or not os.path.isdir(item):
                continue
            for filename in os.listdir(item):
                if not self._is_ext_file(filename):
                    continue
                copy_to(pjoin(item, filename), pjoin(dirforexts, filename))

    def _zip_pyc_files(self, zipname, dirtozip):
        """Create a zip archive of all the .pyc files"""
        with zipfile.ZipFile(zipname, "w") as z:
            for dirpath, dirnames, filenames in os.walk(dirtozip):
                for filename in filenames:
                    fullfname = pjoin(dirpath, filename)
                    relname = fullfname[len(dirtozip) + 1 :]
                    z.write(fullfname, relname)

    def _copy_py_lib(self, dirtocopy):
        """Copy main Python shared library"""
        pylib = "python27" if iswindows else "python2.7"
        pylibext = pylib + (".dll" if iswindows else ".so")
        # First priority is the python lib that lives alongside the executable
        pylibpath = os.path.realpath(pjoin(sys.executable, "..", pylibext))
        if not os.path.exists(pylibpath):
            # a fallback option
            pylibpath = ctypes.util.find_library(pylib)
        log.debug("Python dynamic library is copied from: %s" % pylibpath)
        copy_to(pylibpath, pjoin(dirtocopy, os.path.basename(pylibpath)))

    def _copy_hg_exe(self, dirtocopy):
        """Copy main mercurial executable which would load the embedded Python"""
        bindir = scriptdir
        if not self.local_bins:
            # copy .exe's from ./build/lib.win-amd64/, not from ./
            bindir = pjoin(scriptdir, "build", distutils_dir_name("scripts"))
            sourcename = "hg.rust.exe" if iswindows else "hg.rust"
        else:
            sourcename = "hg.exe" if iswindows else "hg"
        targetname = "hg.exe" if iswindows else "hg"
        log.debug("copying main mercurial binary from %s" % bindir)
        copy_to(pjoin(bindir, sourcename), pjoin(dirtocopy, targetname))

    def _copy_configs(self, dirtocopy):
        """Copy the relevant config bits into an embedded directory"""
        source = pjoin(
            scriptdir, "fb", "staticfiles", "etc", "mercurial", "include_for_nupkg.rc"
        )
        target = pjoin(dirtocopy, "hgrc.d", "include.rc")
        copy_to(source, target)

    def _copy_other(self, dirtocopy):
        """Copy misc files, which aren't main hg codebase"""
        tocopy = {
            "CONTRIBUTING": "CONTRIBUTING",
            "CONTRIBUTORS": "CONTRIBUTORS",
            "contrib": "contrib",
            pjoin("mercurial", "templates"): "templates",
            pjoin("mercurial", "help"): "help",
        }
        for sname, tname in tocopy.items():
            source = pjoin(scriptdir, sname)
            target = pjoin(dirtocopy, tname)
            copy_to(source, target)

    def run(self):
        embdir = pjoin(scriptdir, "build", "embedded")
        libdir = pjoin(embdir, "lib")
        tozip = pjoin(embdir, "_tozip")
        ensureempty(embdir)
        ensureexists(libdir)
        ensureexists(tozip)
        self._process_python_install(tozip)
        self._process_hg_source(tozip)
        self._process_hg_exts(libdir)
        self._process_py_exts(libdir)
        self._zip_pyc_files(pjoin(libdir, "library.zip"), tozip)
        shutil.rmtree(tozip)
        # On Windows, Python shared library has to live at the same level
        # as the main project binary, since this is the location which
        # has the first priority in dynamic linker search path.
        self._copy_py_lib(embdir)
        self._copy_hg_exe(embdir)
        if havefb:
            self._copy_configs(embdir)
        self._copy_other(embdir)


class hgbuildpy(build_py):
    def finalize_options(self):
        build_py.finalize_options(self)

        if self.distribution.pure:
            self.distribution.ext_modules = []
        elif self.distribution.cffi:
            from mercurial.cffi import bdiffbuild, mpatchbuild

            exts = [
                mpatchbuild.ffi.distutils_extension(),
                bdiffbuild.ffi.distutils_extension(),
            ]
            # cffi modules go here
            if sys.platform == "darwin":
                from mercurial.cffi import osutilbuild

                exts.append(osutilbuild.ffi.distutils_extension())
            self.distribution.ext_modules = exts
        else:
            h = os.path.join(get_python_inc(), "Python.h")
            if not os.path.exists(h):
                raise SystemExit(
                    "Python headers are required to build "
                    "Mercurial but weren't found in %s" % h
                )

    def run(self):
        basepath = os.path.join(self.build_lib, "mercurial")
        self.mkpath(basepath)

        if self.distribution.pure:
            modulepolicy = "py"
        elif self.build_lib == ".":
            # in-place build should run without rebuilding C extensions
            modulepolicy = "allow"
        else:
            modulepolicy = "c"

        content = b"".join(
            [
                b"# this file is autogenerated by setup.py\n",
                b'modulepolicy = b"%s"\n' % modulepolicy.encode("ascii"),
            ]
        )
        write_if_changed(os.path.join(basepath, "__modulepolicy__.py"), content)

        build_py.run(self)


class buildhgextindex(Command):
    description = "generate prebuilt index of hgext (for frozen package)"
    user_options = []
    _indexfilename = "hgext/__index__.py"

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
            "import pprint; from mercurial import extensions; "
            "pprint.pprint(extensions.disabled())"
        )
        returncode, out, err = runcmd([sys.executable, "-c", code], localhgenv())
        if err or returncode != 0:
            raise DistutilsExecError(err)

        with open(self._indexfilename, "w") as f:
            f.write("# this file is autogenerated by setup.py\n")
            f.write("docs = ")
            f.write(out)


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
        finally:
            file_util.copy_file = realcopyfile


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

            data = data.replace(b"@LIBDIR@", libdir.encode(libdir_escape))
            with open(outfile, "wb") as fp:
                fp.write(data)


cmdclass = {
    "build": hgbuild,
    "build_mo": hgbuildmo,
    "build_ext": hgbuildext,
    "build_py": hgbuildpy,
    "build_scripts": hgbuildscripts,
    "build_hgextindex": buildhgextindex,
    "install": hginstall,
    "install_lib": hginstalllib,
    "install_scripts": hginstallscripts,
    "build_rust_ext": BuildRustExt,
    "build_embedded": buildembedded,
}

packages = [
    "mercurial",
    "mercurial.cext",
    "mercurial.cffi",
    "mercurial.commands",
    "mercurial.hgweb",
    "mercurial.httpclient",
    "mercurial.pure",
    "mercurial.rust",
    "mercurial.thirdparty",
    "mercurial.thirdparty.attr",
    "mercurial.thirdparty.pyre2",
    "hgext",
    "hgext.absorb",
    "hgext.amend",
    "hgext.commitcloud",
    "hgext.convert",
    "hgext.extlib",
    "hgext.extlib.phabricator",
    "hgext.extlib.pywatchman",
    "hgext.extlib.watchmanclient",
    "hgext.fastannotate",
    "hgext.fastmanifest",
    "hgext.fsmonitor",
    "hgext.hgevents",
    "hgext.hggit",
    "hgext.hgsubversion",
    "hgext.hgsubversion.hooks",
    "hgext.hgsubversion.layouts",
    "hgext.hgsubversion.svnwrap",
    "hgext.highlight",
    "hgext.infinitepush",
    "hgext.lfs",
    "hgext.p4fastimport",
    "hgext.pushrebase",
    "hgext.remotefilelog",
    "hgext.treemanifest",
    "hgext.zeroconf",
    "hgext3rd",
    "hgdemandimport",
]

if havefb:
    packages.append("mercurial.fb")

common_depends = [
    "mercurial/bitmanipulation.h",
    "mercurial/compat.h",
    "mercurial/cext/util.h",
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
include_dirs = ["."] + include_dirs

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
osutil_ldflags = []

# platform specific macros
for plat, func in [("bsd", "setproctitle")]:
    if re.search(plat, sys.platform) and hasfunction(new_compiler(), func):
        osutil_cflags.append("-DHAVE_%s" % func.upper())

for plat, macro, code in [
    (
        "bsd|darwin",
        "BSD_STATFS",
        """
     #include <sys/param.h>
     #include <sys/mount.h>
     int main() { struct statfs s; return sizeof(s.f_fstypename); }
     """,
    ),
    (
        "linux",
        "LINUX_STATFS",
        """
     #include <linux/magic.h>
     #include <sys/vfs.h>
     int main() { struct statfs s; return sizeof(s.f_type); }
     """,
    ),
]:
    if re.search(plat, sys.platform) and cancompile(new_compiler(), code):
        osutil_cflags.append("-DHAVE_%s" % macro)

if sys.platform == "darwin":
    osutil_ldflags += ["-framework", "ApplicationServices"]

extmodules = [
    Extension(
        "mercurial.cext.base85",
        ["mercurial/cext/base85.c"],
        include_dirs=include_dirs,
        depends=common_depends,
    ),
    Extension(
        "mercurial.cext.bdiff",
        ["mercurial/bdiff.c", "mercurial/cext/bdiff.c"],
        include_dirs=include_dirs,
        depends=common_depends + ["mercurial/bdiff.h"],
    ),
    Extension(
        "mercurial.cext.diffhelpers",
        ["mercurial/cext/diffhelpers.c"],
        include_dirs=include_dirs,
        depends=common_depends,
    ),
    Extension(
        "mercurial.cext.mpatch",
        ["mercurial/mpatch.c", "mercurial/cext/mpatch.c"],
        include_dirs=include_dirs,
        depends=common_depends + ["mercurial/mpatch.h"],
    ),
    Extension(
        "mercurial.cext.parsers",
        [
            "mercurial/cext/charencode.c",
            "mercurial/cext/dirs.c",
            "mercurial/cext/manifest.c",
            "mercurial/cext/parsers.c",
            "mercurial/cext/pathencode.c",
            "mercurial/cext/revlog.c",
        ],
        include_dirs=include_dirs,
        depends=common_depends + ["mercurial/cext/charencode.h"],
    ),
    Extension(
        "mercurial.cext.osutil",
        ["mercurial/cext/osutil.c"],
        include_dirs=include_dirs,
        extra_compile_args=osutil_cflags,
        extra_link_args=osutil_ldflags,
        depends=common_depends,
    ),
    Extension(
        "mercurial.cext.xdiff",
        sources=[
            "lib/third-party/xdiff/xdiffi.c",
            "lib/third-party/xdiff/xprepare.c",
            "lib/third-party/xdiff/xutils.c",
            "mercurial/cext/xdiff.c",
        ],
        include_dirs=include_dirs + ["lib/third-party/xdiff"],
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
    Extension("hgext.extlib.pywatchman.bser", ["hgext/extlib/pywatchman/bser.c"]),
    Extension(
        "hgext.extlib.cstore",
        sources=[
            "hgext/extlib/cstore/datapackstore.cpp",
            "hgext/extlib/cstore/deltachain.cpp",
            "hgext/extlib/cstore/py-cstore.cpp",
            "hgext/extlib/cstore/pythonutil.cpp",
            "hgext/extlib/cstore/pythondatastore.cpp",
            "hgext/extlib/cstore/uniondatapackstore.cpp",
            "hgext/extlib/ctreemanifest/manifest.cpp",
            "hgext/extlib/ctreemanifest/manifest_entry.cpp",
            "hgext/extlib/ctreemanifest/manifest_fetcher.cpp",
            "hgext/extlib/ctreemanifest/manifest_ptr.cpp",
            "hgext/extlib/ctreemanifest/treemanifest.cpp",
        ],
        depends=[
            "hgext/extlib/cstore/datapackstore.h",
            "hgext/extlib/cstore/datastore.h",
            "hgext/extlib/cstore/deltachain.h",
            "hgext/extlib/cstore/key.h",
            "hgext/extlib/cstore/match.h",
            "hgext/extlib/cstore/py-cdatapack.h",
            "hgext/extlib/cstore/py-datapackstore.h",
            "hgext/extlib/cstore/py-structs.h",
            "hgext/extlib/cstore/py-treemanifest.h",
            "hgext/extlib/cstore/pythondatastore.h",
            "hgext/extlib/cstore/pythonkeyiterator.h",
            "hgext/extlib/cstore/pythonutil.h",
            "hgext/extlib/cstore/store.h",
            "hgext/extlib/cstore/uniondatapackstore.h",
            "hgext/extlib/cstore/util.h",
        ],
        include_dirs=include_dirs,
        library_dirs=["build/" + distutils_dir_name("lib")] + library_dirs,
        libraries=["datapack", "lz4", "mpatch", SHA1_LIBRARY],
        extra_compile_args=filter(None, [STDCPP0X, WALL] + cflags),
    ),
    Extension(
        "hgext.extlib.cfastmanifest",
        sources=[
            "hgext/extlib/cfastmanifest.c",
            "hgext/extlib/cfastmanifest/bsearch.c",
            "lib/clib/buffer.c",
            "hgext/extlib/cfastmanifest/checksum.c",
            "hgext/extlib/cfastmanifest/node.c",
            "hgext/extlib/cfastmanifest/tree.c",
            "hgext/extlib/cfastmanifest/tree_arena.c",
            "hgext/extlib/cfastmanifest/tree_convert.c",
            "hgext/extlib/cfastmanifest/tree_copy.c",
            "hgext/extlib/cfastmanifest/tree_diff.c",
            "hgext/extlib/cfastmanifest/tree_disk.c",
            "hgext/extlib/cfastmanifest/tree_iterator.c",
            "hgext/extlib/cfastmanifest/tree_path.c",
        ],
        depends=[
            "hgext/extlib/cfastmanifest/bsearch.h",
            "hgext/extlib/cfastmanifest/checksum.h",
            "hgext/extlib/cfastmanifest/internal_result.h",
            "hgext/extlib/cfastmanifest/node.h",
            "hgext/extlib/cfastmanifest/path_buffer.h",
            "hgext/extlib/cfastmanifest/result.h",
            "hgext/extlib/cfastmanifest/tests.h",
            "hgext/extlib/cfastmanifest/tree_arena.h",
            "hgext/extlib/cfastmanifest/tree.h",
            "hgext/extlib/cfastmanifest/tree_iterator.h",
            "hgext/extlib/cfastmanifest/tree_path.h",
        ],
        include_dirs=include_dirs,
        library_dirs=library_dirs,
        libraries=[SHA1_LIBRARY],
        extra_compile_args=filter(None, [STDC99, WALL, WSTRICTPROTOTYPES] + cflags),
    ),
]

# Cython modules
# see http://cython.readthedocs.io/en/latest/src/reference/compilation.html
cythonopts = {"unraisable_tracebacks": False, "c_string_type": "bytes"}

extmodules += cythonize(
    [
        Extension(
            "hgext.clindex",
            sources=["hgext/clindex.pyx"],
            extra_compile_args=filter(None, [STDC99, PRODUCEDEBUGSYMBOLS]),
        ),
        Extension(
            "hgext.extlib.litemmap",
            sources=["hgext/extlib/litemmap.pyx"],
            extra_compile_args=filter(None, [STDC99, PRODUCEDEBUGSYMBOLS]),
        ),
        Extension(
            "hgext.patchrmdir",
            sources=["hgext/patchrmdir.pyx"],
            extra_compile_args=filter(None, [PRODUCEDEBUGSYMBOLS]),
        ),
        Extension(
            "hgext.traceprof",
            sources=["hgext/traceprof.pyx"],
            include_dirs=include_dirs,
            extra_compile_args=filter(None, [STDCPP11, PRODUCEDEBUGSYMBOLS]),
        ),
        Extension(
            "hgext.extlib.linelog",
            sources=["hgext/extlib/linelog.pyx"],
            include_dirs=include_dirs,
            extra_compile_args=filter(None, [STDC99, PRODUCEDEBUGSYMBOLS]),
        ),
    ],
    compiler_directives=cythonopts,
)

libraries = [
    (
        "datapack",
        {
            "sources": ["lib/cdatapack/cdatapack.c"],
            "depends": ["lib/cdatapack/cdatapack.h"],
            "include_dirs": ["."] + include_dirs,
            "libraries": ["lz4", SHA1_LIBRARY],
            "extra_args": filter(None, [STDC99, WALL, WSTRICTPROTOTYPES] + cflags),
        },
    ),
    (
        "sha1detectcoll",
        {
            "sources": [
                "lib/third-party/sha1dc/sha1.c",
                "lib/third-party/sha1dc/ubc_check.c",
            ],
            "depends": [
                "lib/third-party/sha1dc/sha1.h",
                "lib/third-party/sha1dc/ubc_check.h",
            ],
            "include_dirs": ["lib/third-party"] + include_dirs,
            "extra_args": filter(None, [STDC99, WALL, WSTRICTPROTOTYPES] + cflags),
        },
    ),
    (
        "mpatch",
        {
            "sources": ["mercurial/mpatch.c"],
            "depends": [
                "mercurial/bitmanipulation.h",
                "mercurial/compat.h",
                "mercurial/mpatch.h",
            ],
            "include_dirs": ["."] + include_dirs,
        },
    ),
]

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
    "mercurial": [
        "locale/*/LC_MESSAGES/hg.mo",
        "help/*.txt",
        "help/internals/*.txt",
        "help/subversion/*.rst",
        "default.d/*.rc",
        "dummycert.pem",
    ]
}


def ordinarypath(p):
    return p and p[0] != "." and p[-1] != "~"


for root in ("templates",):
    for curdir, dirs, files in os.walk(os.path.join("mercurial", root)):
        curdir = curdir.split(os.sep, 1)[1]
        dirs[:] = filter(ordinarypath, dirs)
        for f in filter(ordinarypath, files):
            f = os.path.join(curdir, f)
            packagedata["mercurial"].append(f)

datafiles = [("", ["CONTRIBUTING", "CONTRIBUTORS"])]
templatesdir = "mercurial/templates"
for parent, dirs, files in os.walk(templatesdir):
    dirfiles = [os.path.join(parent, fn) for fn in files]
    datafiles.append((os.path.join("templates", parent), dirfiles))


# distutils expects version to be str/unicode. Converting it to
# unicode on Python 2 still works because it won't contain any
# non-ascii bytes and will be implicitly converted back to bytes
# when operated on.
assert isinstance(version, bytes)
setupversion = version.decode("ascii")

extra = {}

if issetuptools:
    extra["python_requires"] = supportedpy

if os.name == "nt":
    # Windows binary file versions for exe/dll files must have the
    # form W.X.Y.Z, where W,X,Y,Z are numbers in the range 0..65535
    setupversion = version.split("+", 1)[0]

if sys.platform == "darwin" and os.path.exists("/usr/bin/xcodebuild"):
    version = runcmd(["/usr/bin/xcodebuild", "-version"], {})[1].splitlines()
    if version:
        version = version[0]
        if sys.version_info[0] == 3:
            version = version.decode("utf-8")
        xcode4 = version.startswith("Xcode") and StrictVersion(
            version.split()[1]
        ) >= StrictVersion("4.0")
        xcode51 = re.match(r"^Xcode\s+5\.1", version) is not None
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

rustvendoredcrates = []
cargoconfig = """
# On OS X targets, configure the linker to perform dynamic lookup of undefined
# symbols.  This allows the library to be used as a Python extension.

[target.i686-apple-darwin]
rustflags = ["-C", "link-args=-Wl,-undefined,dynamic_lookup"]

[target.x86_64-apple-darwin]
rustflags = ["-C", "link-args=-Wl,-undefined,dynamic_lookup"]
"""

if havefb:
    rustvendoredcrates.append(
        RustVendoredCrates("tp2-crates-io", dest="build/tp2-crates-io")
    )
    cargoconfig += """
# Vendor in Rust crates.  "build/hg-vendored-crates" is populated by the
# contents of a vendor package downloaded from Dewey with the hash in
# ".hg-vendored-crates".

[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "build/tp2-crates-io/vendor"
    """

try:
    os.mkdir(".cargo")
except OSError as e:
    if e.errno != errno.EEXIST:
        raise

with open(".cargo/config", "w") as f:
    f.write(cargoconfig)


rustextmodules = [
    RustExtension(
        "config", package="mercurial.rust", manifest="mercurial/rust/config/Cargo.toml"
    ),
    RustExtension(
        "indexes", package="hgext.extlib", manifest="hgext/extlib/indexes/Cargo.toml"
    ),
    RustExtension(
        "matcher",
        package="mercurial.rust",
        manifest="mercurial/rust/matcher/Cargo.toml",
    ),
    RustExtension(
        "pyrevisionstore",
        package="hgext.extlib",
        manifest="hgext/extlib/pyrevisionstore/Cargo.toml",
    ),
    RustExtension(
        "treestate",
        package="mercurial.rust",
        manifest="mercurial/rust/treestate/Cargo.toml",
    ),
    RustExtension(
        "bookmarkstore",
        package="mercurial.rust",
        manifest="mercurial/rust/bookmarkstore/Cargo.toml",
    ),
    RustExtension(
        "zstd", package="mercurial.rust", manifest="mercurial/rust/zstd/Cargo.toml"
    ),
]

rustextbinaries = [
    RustBinary("scm_daemon", manifest="exec/scm_daemon/Cargo.toml"),
    RustBinary(
        "hgmain",
        manifest="exec/hgmain/Cargo.toml",
        rename="hg.rust",
        features="hgdev" if os.environ.get("HGDEV") else None,
    ),
]

setup(
    name="mercurial",
    version=setupversion,
    author="Matt Mackall and many others",
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
    scripts=scripts,
    packages=packages,
    ext_modules=extmodules,
    libraries=libraries,
    rust_vendored_crates=rustvendoredcrates,
    rust_ext_modules=rustextmodules,
    rust_ext_binaries=rustextbinaries,
    data_files=datafiles,
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
    **extra
)
