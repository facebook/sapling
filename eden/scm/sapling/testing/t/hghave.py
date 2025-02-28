# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


import errno
import logging
import os
import re
import socket
import stat
import subprocess
import sys
import tempfile

logger = logging.getLogger(__name__)

tempprefix = "hg-hghave-"

checks = {
    "true": (lambda: True, "yak shaving"),
    "false": (lambda: False, "nail clipper"),
}

exes = set()


def check(name, desc, exe: bool = False):
    """Registers a check function for a feature."""

    def decorator(func):
        checks[name] = (func, desc)
        return func

    if exe:
        exes.add(name)

    return decorator


def checkvers(name, desc, vers):
    """Registers a check function for each of a series of versions.

    vers can be a list or an iterator"""

    def decorator(func):
        def funcv(v):
            def f():
                return func(v)

            return f

        for v in vers:
            v = str(v)
            f = funcv(v)
            checks["%s%s" % (name, v.replace(".", ""))] = (f, desc % v)
        return func

    return decorator


def checkexe(name):
    f = lambda name=name: os.path.isfile(f"/bin/{name}") or os.path.isfile(
        f"/usr/bin/{name}"
    )
    checks[name] = (f, f"{name} executable")
    exes.add(name)


checkexe("cmp")
checkexe("dd")
checkexe("diff")
checkexe("echo")
checkexe("env")
checkexe("gpg")
checkexe("gunzip")
checkexe("gzip")
checkexe("mkfifo")
checkexe("python3.8")
checkexe("tar")
checkexe("tr")
checkexe("umask")
checkexe("unzip")
checkexe("xargs")


_checkfeaturecache = {}


def checkfeatures(features):
    result = {"error": [], "missing": [], "skipped": []}

    logger.debug("available features: %s", list(checks.keys()))
    for feature in features:
        if feature.startswith("/"):
            # feature is a path to a binary on POSIX
            if not os.access(feature, os.X_OK):
                result["skipped"].append(f"missing binary: {feature}")
            continue

        negate = feature.startswith("no-")
        if negate:
            feature = feature[3:]

        if feature not in checks:
            if not negate:
                logger.debug("unknown feature: %s", feature)
                result["missing"].append(feature)
            continue

        available, desc = checks[feature]
        if callable(available):
            check = available
            available = _checkfeaturecache.get(feature)
            try:
                if available is None:
                    available = check()
                    _checkfeaturecache[feature] = available
            except Exception:
                result["error"].append("hghave check failed: %s" % feature)
                continue

        if not negate and not available:
            result["skipped"].append("missing feature: %s" % desc)
        elif negate and available:
            result["skipped"].append("system supports %s" % desc)

    return result


def require(features):
    """Require that features are available, exiting if not."""
    result = checkfeatures(features)

    for missing in result["missing"]:
        sys.stderr.write("skipped: unknown feature: %s\n" % missing)
    for msg in result["skipped"]:
        sys.stderr.write("skipped: %s\n" % msg)
    for msg in result["error"]:
        sys.stderr.write("%s\n" % msg)

    if result["missing"]:
        sys.exit(2)

    if result["skipped"]:
        sys.exit(80)

    if result["error"]:
        sys.exit(1)


def matchoutput(cmd, regexp, ignorestatus=False):
    """Return the match object if cmd executes successfully and its output
    is matched by the supplied regular expression.
    """
    r = re.compile(regexp)
    try:
        p = subprocess.Popen(
            cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
        )
    except OSError as e:
        if e.errno != errno.ENOENT:
            raise
        ret = -1
    ret = p.wait()
    s = p.stdout.read()
    return (ignorestatus or not ret) and r.search(s)


def tempdir():
    # Prefer TESTTMP for detecting fs capabilities on the same mount
    testtmp = os.getenv("TESTTMP")
    return testtmp or tempfile.gettempdir()


@check("chg", "running with chg")
def has_chg():
    return "CHGHG" in os.environ


_zlibsamples = {
    b"c7667dad766d": "789c4b363733334f494c01522900160b036e",
    b"36a25358b7f16835db5a8e4ecc68328f42": "789c33364b34323536b548324f3"
    "334b330364d49324db44835494d4e06f28c2cd24c8c009b180907",
    b"1e0ed22dfcf821b7368535f0d41099f35a139451aec6dfde551a4808c8fc5f": "789c0dc6c901c0300803b095b89cc23829e0fd4768f592aeec980d9b69fa3e"
    "7e120eca844a151d57bd027ab7cf70167f23253bd9e007160f1121",
}


@check("common-zlib", "common zlib that produces consistent result")
def has_common_zlib():
    import binascii
    import zlib

    return all(
        zlib.compress(k) == binascii.unhexlify(v) for k, v in _zlibsamples.items()
    )


@check("eol-in-paths", "end-of-lines in paths")
def has_eol_in_paths():
    try:
        fd, path = tempfile.mkstemp(dir=tempdir(), prefix=tempprefix, suffix="\n\r")
        os.close(fd)
        os.remove(path)
        return True
    except (IOError, OSError):
        return False


@check("execbit", "executable bit")
def has_executablebit():
    try:
        EXECFLAGS = stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
        fh, fn = tempfile.mkstemp(dir=tempdir(), prefix=tempprefix)
        try:
            os.close(fh)
            m = os.stat(fn).st_mode & 0o777
            new_file_has_exec = m & EXECFLAGS
            os.chmod(fn, m ^ EXECFLAGS)
            exec_flags_cannot_flip = (os.stat(fn).st_mode & 0o777) == m
        finally:
            os.unlink(fn)
    except (IOError, OSError):
        # we don't care, the user probably won't be able to commit anyway
        return False
    return not (new_file_has_exec or exec_flags_cannot_flip)


@check("icasefs", "case insensitive file system")
def has_icasefs():
    # Stolen from sapling.util
    fd, path = tempfile.mkstemp(dir=tempdir(), prefix=tempprefix)
    os.close(fd)
    try:
        s1 = os.stat(path)
        d, b = os.path.split(path)
        p2 = os.path.join(d, b.upper())
        if path == p2:
            p2 = os.path.join(d, b.lower())
        try:
            s2 = os.stat(p2)
            return s2 == s1
        except OSError:
            return False
    finally:
        os.remove(path)


@check("fifo", "named pipes")
def has_fifo():
    if getattr(os, "mkfifo", None) is None:
        return False
    name = tempfile.mktemp(dir=tempdir(), prefix=tempprefix)
    try:
        os.mkfifo(name)
        os.unlink(name)
        return True
    except OSError:
        return False


@check("normal-layout", "common file layout that hg is not a packed binary")
def has_normal_layout():
    # Cannot test this reliably. So test an environment variable set by the
    # test runner.
    return os.environ.get("HGTEST_NORMAL_LAYOUT", "1") == "1"


@check("killdaemons", "killdaemons.py support")
def has_killdaemons():
    return True


@check("git", "git command line client", exe=True)
def has_git():
    return matchoutput("git --version 2>&1", rb"^git version")


def getgitversion():
    m = matchoutput("git --version 2>&1", rb"git version (\d+)\.(\d+)")
    if not m:
        return (0, 0)
    return (int(m.group(1)), int(m.group(2)))


@check("lldb", "lldb debugger from LLVM", exe=True)
def has_lldb():
    return matchoutput("lldb -P 2>&1", b"python")


# https://github.com/git-lfs/lfs-test-server
@check("lfs-test-server", "git-lfs test server")
def has_lfsserver():
    exe = "lfs-test-server"
    if has_windows():
        exe = "lfs-test-server.exe"
    return any(
        os.access(os.path.join(path, exe), os.X_OK)
        for path in os.environ["PATH"].split(os.pathsep)
    )


@checkvers("git", "git client (with ext::sh support) version >= %s", (1.9,))
def has_git_range(v):
    major, minor = v.split(".")[0:2]
    return getgitversion() >= (int(major), int(minor))


@check("jq", "json processing tool", exe=True)
def has_jq():
    return matchoutput("jq --help", rb"Usage:\W+jq .*")


@check("symlink", "symbolic links")
def has_symlink():
    if getattr(os, "symlink", None) is None:
        return False
    name = tempfile.mktemp(dir=tempdir(), prefix=tempprefix)
    try:
        os.symlink(".", name)
        os.unlink(name)
        return True
    except (OSError, AttributeError):
        return False


@check("hardlink", "hardlinks")
def has_hardlink():
    from sapling import util

    fh, fn = tempfile.mkstemp(dir=tempdir(), prefix=tempprefix)
    os.close(fh)
    name = tempfile.mktemp(dir=tempdir(), prefix=tempprefix)
    try:
        util.oslink(fn, name)
        os.unlink(name)
        return True
    except OSError:
        return False
    finally:
        os.unlink(fn)


@check("rmcwd", "can remove current working directory")
def has_rmcwd():
    ocwd = os.getcwd()
    temp = tempfile.mkdtemp(dir=tempdir(), prefix=tempprefix)
    try:
        os.chdir(temp)
        # On Linux, 'rmdir .' isn't allowed, but the other names are okay.
        # On Solaris and Windows, the cwd can't be removed by any names.
        os.rmdir(os.getcwd())
        return True
    except OSError:
        return False
    finally:
        os.chdir(ocwd)
        # clean up temp dir on platforms where cwd can't be removed
        try:
            os.rmdir(temp)
        except OSError:
            pass


@check("gpg2", "gpg client v2")
def has_gpg2():
    return matchoutput("gpg --version 2>&1", rb"GnuPG[^0-9]+2\.")


@check("unix-permissions", "unix-style permissions")
def has_unix_permissions():
    d = tempfile.mkdtemp(dir=tempdir(), prefix=tempprefix)
    try:
        fname = os.path.join(d, "foo")
        for umask in (0o77, 0o07, 0o22):
            os.umask(umask)
            f = open(fname, "w")
            f.close()
            mode = os.stat(fname).st_mode
            os.unlink(fname)
            if mode & 0o777 != ~umask & 0o666:
                return False
        return True
    finally:
        os.rmdir(d)


@check("unix-socket", "AF_UNIX socket family")
def has_unix_socket():
    return getattr(socket, "AF_UNIX", None) is not None


@check("root", "root permissions")
def has_root():
    return getattr(os, "geteuid", None) and os.geteuid() == 0


@check("clang-format", "clang-format C code formatter", exe=True)
def has_clang_format():
    return matchoutput(
        "clang-format --help", rb"^OVERVIEW: A tool to format C/C\+\+[^ ]+ code."
    )


@check("pygments", "Pygments source highlighting library")
def has_pygments():
    try:
        import pygments

        pygments.highlight  # silence unused import warning
        return True
    except ImportError:
        return False


@check("outer-repo", "outer repo")
def has_outer_repo():
    # failing for other reasons than 'no repo' imply that there is a repo
    return not matchoutput("hg root 2>&1", rb"abort: no repository found", True)


@check("windows", "Windows")
def has_windows():
    return os.name == "nt"


@check("system-sh", "system() uses sh")
def has_system_sh():
    return os.name != "nt"


@check("serve", "platform and python can manage 'hg serve -d'")
def has_serve():
    return True


@check("test-repo", "running tests from repository")
def has_test_repo():
    # test-check-*.t tests. They are confusing as the "hg"
    # might have to be the system hg, not the one for testing.
    # Drop support for them to avoid supporting running tests
    # using system hg.
    # Those tests might want to be written as separate linters
    # instead.
    return False


@check("tic", "terminfo compiler and curses module")
def has_tic():
    try:
        import curses

        curses.COLOR_BLUE
        return matchoutput('test -x "`which tic`"', rb"")
    except ImportError:
        return False


@check("msys", "Windows with MSYS")
def has_msys():
    return os.getenv("MSYSTEM")


@check("aix", "AIX")
def has_aix():
    return sys.platform.startswith("aix")


@check("osx", "OS X")
def has_osx():
    return sys.platform == "darwin"


@check("linux", "Linux")
def has_linux():
    return sys.platform == "linux"


@check("security", "OS X security helper", exe=True)
def has_security():
    return matchoutput("security", rb"security commands are", ignorestatus=1)


@check("linuxormacos", "Linux or MacOS")
def has_linuxormacos():
    # This isn't a perfect test for MacOS. But it is sufficient for our needs.
    return sys.platform.startswith(("linux", "darwin"))


@check("demandimport", "demandimport enabled")
def has_demandimport():
    # chg disables demandimport intentionally for performance wins.
    return (not has_chg()) and os.environ.get("HGDEMANDIMPORT") != "disable"


@check("slow", "allow slow tests (use --allow-slow-tests)")
def has_slow():
    return os.environ.get("HGTEST_SLOW") == "slow"


@check("unziplinks", "unzip(1) understands and extracts symlinks")
def unzip_understands_symlinks():
    return matchoutput("unzip --help", rb"Info-ZIP")


@check("zstd", "zstd Python module available")
def has_zstd():
    try:
        from bindings import zstd

        zstd.apply
        return True
    except ImportError:
        return False


@check("devfull", "/dev/full special file")
def has_dev_full():
    return os.path.exists("/dev/full")


@check("fsmonitor", "running tests with fsmonitor")
def has_fsmonitor():
    return "HGFSMONITOR_TESTS" in os.environ


@check("eden", "Eden HG extension", exe=True)
def has_eden():
    return os.environ.get("HGTEST_USE_EDEN", None) == "1" and matchoutput(
        "eden version",
        re.compile(rb"^Installed:\s.*\sRunning:\s.*", re.MULTILINE),
    )


@check("node", "nodejs", exe=True)
def has_node():
    return matchoutput(
        '''node --input-type=module -e "import * as assert from 'node:assert'; console.log(1+2)"''',
        b"3\n",
    )


@check("mononoke", "Mononoke server available")
def has_mononoke():
    return "USE_MONONOKE" in os.environ


@check("bucktest", "Tests are being run from Buck")
def has_bucktest():
    return "HGTEST_HG" in os.environ


@check("bash", "running via real bash")
def has_bash():
    return False


@check("ipython", "can import IPython")
def has_ipython():
    try:
        import IPython

        # force demandimport to load the module
        IPython.embed
    except Exception:
        return False
    return True


@check("py3.10", "Python is 3.10")
def has_python310():
    return sys.version_info[:2] == (3, 10)
