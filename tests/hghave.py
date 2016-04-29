from __future__ import absolute_import

import errno
import os
import re
import socket
import stat
import subprocess
import sys
import tempfile

tempprefix = 'hg-hghave-'

checks = {
    "true": (lambda: True, "yak shaving"),
    "false": (lambda: False, "nail clipper"),
}

def check(name, desc):
    """Registers a check function for a feature."""
    def decorator(func):
        checks[name] = (func, desc)
        return func
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
            checks['%s%s' % (name, v.replace('.', ''))] = (f, desc % v)
        return func
    return decorator

def checkfeatures(features):
    result = {
        'error': [],
        'missing': [],
        'skipped': [],
    }

    for feature in features:
        negate = feature.startswith('no-')
        if negate:
            feature = feature[3:]

        if feature not in checks:
            result['missing'].append(feature)
            continue

        check, desc = checks[feature]
        try:
            available = check()
        except Exception:
            result['error'].append('hghave check failed: %s' % feature)
            continue

        if not negate and not available:
            result['skipped'].append('missing feature: %s' % desc)
        elif negate and available:
            result['skipped'].append('system supports %s' % desc)

    return result

def require(features):
    """Require that features are available, exiting if not."""
    result = checkfeatures(features)

    for missing in result['missing']:
        sys.stderr.write('skipped: unknown feature: %s\n' % missing)
    for msg in result['skipped']:
        sys.stderr.write('skipped: %s\n' % msg)
    for msg in result['error']:
        sys.stderr.write('%s\n' % msg)

    if result['missing']:
        sys.exit(2)

    if result['skipped'] or result['error']:
        sys.exit(1)

def matchoutput(cmd, regexp, ignorestatus=False):
    """Return the match object if cmd executes successfully and its output
    is matched by the supplied regular expression.
    """
    r = re.compile(regexp)
    try:
        p = subprocess.Popen(
            cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    except OSError as e:
        if e.errno != errno.ENOENT:
            raise
        ret = -1
    ret = p.wait()
    s = p.stdout.read()
    return (ignorestatus or not ret) and r.search(s)

@check("baz", "GNU Arch baz client")
def has_baz():
    return matchoutput('baz --version 2>&1', r'baz Bazaar version')

@check("bzr", "Canonical's Bazaar client")
def has_bzr():
    try:
        import bzrlib
        return bzrlib.__doc__ is not None
    except ImportError:
        return False

@checkvers("bzr", "Canonical's Bazaar client >= %s", (1.14,))
def has_bzr_range(v):
    major, minor = v.split('.')[0:2]
    try:
        import bzrlib
        return (bzrlib.__doc__ is not None
                and bzrlib.version_info[:2] >= (int(major), int(minor)))
    except ImportError:
        return False

@check("chg", "running with chg")
def has_chg():
    return 'CHGHG' in os.environ

@check("cvs", "cvs client/server")
def has_cvs():
    re = r'Concurrent Versions System.*?server'
    return matchoutput('cvs --version 2>&1', re) and not has_msys()

@check("cvs112", "cvs client/server 1.12.* (not cvsnt)")
def has_cvs112():
    re = r'Concurrent Versions System \(CVS\) 1.12.*?server'
    return matchoutput('cvs --version 2>&1', re) and not has_msys()

@check("cvsnt", "cvsnt client/server")
def has_cvsnt():
    re = r'Concurrent Versions System \(CVSNT\) (\d+).(\d+).*\(client/server\)'
    return matchoutput('cvsnt --version 2>&1', re)

@check("darcs", "darcs client")
def has_darcs():
    return matchoutput('darcs --version', r'2\.[2-9]', True)

@check("mtn", "monotone client (>= 1.0)")
def has_mtn():
    return matchoutput('mtn --version', r'monotone', True) and not matchoutput(
        'mtn --version', r'monotone 0\.', True)

@check("eol-in-paths", "end-of-lines in paths")
def has_eol_in_paths():
    try:
        fd, path = tempfile.mkstemp(dir='.', prefix=tempprefix, suffix='\n\r')
        os.close(fd)
        os.remove(path)
        return True
    except (IOError, OSError):
        return False

@check("execbit", "executable bit")
def has_executablebit():
    try:
        EXECFLAGS = stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
        fh, fn = tempfile.mkstemp(dir='.', prefix=tempprefix)
        try:
            os.close(fh)
            m = os.stat(fn).st_mode & 0o777
            new_file_has_exec = m & EXECFLAGS
            os.chmod(fn, m ^ EXECFLAGS)
            exec_flags_cannot_flip = ((os.stat(fn).st_mode & 0o777) == m)
        finally:
            os.unlink(fn)
    except (IOError, OSError):
        # we don't care, the user probably won't be able to commit anyway
        return False
    return not (new_file_has_exec or exec_flags_cannot_flip)

@check("icasefs", "case insensitive file system")
def has_icasefs():
    # Stolen from mercurial.util
    fd, path = tempfile.mkstemp(dir='.', prefix=tempprefix)
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
    name = tempfile.mktemp(dir='.', prefix=tempprefix)
    try:
        os.mkfifo(name)
        os.unlink(name)
        return True
    except OSError:
        return False

@check("killdaemons", 'killdaemons.py support')
def has_killdaemons():
    return True

@check("cacheable", "cacheable filesystem")
def has_cacheable_fs():
    from mercurial import util

    fd, path = tempfile.mkstemp(dir='.', prefix=tempprefix)
    os.close(fd)
    try:
        return util.cachestat(path).cacheable()
    finally:
        os.remove(path)

@check("lsprof", "python lsprof module")
def has_lsprof():
    try:
        import _lsprof
        _lsprof.Profiler # silence unused import warning
        return True
    except ImportError:
        return False

def gethgversion():
    m = matchoutput('hg --version --quiet 2>&1', r'(\d+)\.(\d+)')
    if not m:
        return (0, 0)
    return (int(m.group(1)), int(m.group(2)))

@checkvers("hg", "Mercurial >= %s",
            list([(1.0 * x) / 10 for x in range(9, 40)]))
def has_hg_range(v):
    major, minor = v.split('.')[0:2]
    return gethgversion() >= (int(major), int(minor))

@check("hg08", "Mercurial >= 0.8")
def has_hg08():
    if checks["hg09"][0]():
        return True
    return matchoutput('hg help annotate 2>&1', '--date')

@check("hg07", "Mercurial >= 0.7")
def has_hg07():
    if checks["hg08"][0]():
        return True
    return matchoutput('hg --version --quiet 2>&1', 'Mercurial Distributed SCM')

@check("hg06", "Mercurial >= 0.6")
def has_hg06():
    if checks["hg07"][0]():
        return True
    return matchoutput('hg --version --quiet 2>&1', 'Mercurial version')

@check("gettext", "GNU Gettext (msgfmt)")
def has_gettext():
    return matchoutput('msgfmt --version', 'GNU gettext-tools')

@check("git", "git command line client")
def has_git():
    return matchoutput('git --version 2>&1', r'^git version')

@check("docutils", "Docutils text processing library")
def has_docutils():
    try:
        import docutils.core
        docutils.core.publish_cmdline # silence unused import
        return True
    except ImportError:
        return False

def getsvnversion():
    m = matchoutput('svn --version --quiet 2>&1', r'^(\d+)\.(\d+)')
    if not m:
        return (0, 0)
    return (int(m.group(1)), int(m.group(2)))

@checkvers("svn", "subversion client and admin tools >= %s", (1.3, 1.5))
def has_svn_range(v):
    major, minor = v.split('.')[0:2]
    return getsvnversion() >= (int(major), int(minor))

@check("svn", "subversion client and admin tools")
def has_svn():
    return matchoutput('svn --version 2>&1', r'^svn, version') and \
        matchoutput('svnadmin --version 2>&1', r'^svnadmin, version')

@check("svn-bindings", "subversion python bindings")
def has_svn_bindings():
    try:
        import svn.core
        version = svn.core.SVN_VER_MAJOR, svn.core.SVN_VER_MINOR
        if version < (1, 4):
            return False
        return True
    except ImportError:
        return False

@check("p4", "Perforce server and client")
def has_p4():
    return (matchoutput('p4 -V', r'Rev\. P4/') and
            matchoutput('p4d -V', r'Rev\. P4D/'))

@check("symlink", "symbolic links")
def has_symlink():
    if getattr(os, "symlink", None) is None:
        return False
    name = tempfile.mktemp(dir='.', prefix=tempprefix)
    try:
        os.symlink(".", name)
        os.unlink(name)
        return True
    except (OSError, AttributeError):
        return False

@check("hardlink", "hardlinks")
def has_hardlink():
    from mercurial import util
    fh, fn = tempfile.mkstemp(dir='.', prefix=tempprefix)
    os.close(fh)
    name = tempfile.mktemp(dir='.', prefix=tempprefix)
    try:
        util.oslink(fn, name)
        os.unlink(name)
        return True
    except OSError:
        return False
    finally:
        os.unlink(fn)

@check("tla", "GNU Arch tla client")
def has_tla():
    return matchoutput('tla --version 2>&1', r'The GNU Arch Revision')

@check("gpg", "gpg client")
def has_gpg():
    return matchoutput('gpg --version 2>&1', r'GnuPG')

@check("unix-permissions", "unix-style permissions")
def has_unix_permissions():
    d = tempfile.mkdtemp(dir='.', prefix=tempprefix)
    try:
        fname = os.path.join(d, 'foo')
        for umask in (0o77, 0o07, 0o22):
            os.umask(umask)
            f = open(fname, 'w')
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
    return getattr(socket, 'AF_UNIX', None) is not None

@check("root", "root permissions")
def has_root():
    return getattr(os, 'geteuid', None) and os.geteuid() == 0

@check("pyflakes", "Pyflakes python linter")
def has_pyflakes():
    return matchoutput("sh -c \"echo 'import re' 2>&1 | pyflakes\"",
                       r"<stdin>:1: 're' imported but unused",
                       True)

@check("pygments", "Pygments source highlighting library")
def has_pygments():
    try:
        import pygments
        pygments.highlight # silence unused import warning
        return True
    except ImportError:
        return False

@check("outer-repo", "outer repo")
def has_outer_repo():
    # failing for other reasons than 'no repo' imply that there is a repo
    return not matchoutput('hg root 2>&1',
                           r'abort: no repository found', True)

@check("ssl", "ssl module available")
def has_ssl():
    try:
        import ssl
        ssl.CERT_NONE
        return True
    except ImportError:
        return False

@check("sslcontext", "python >= 2.7.9 ssl")
def has_sslcontext():
    try:
        import ssl
        ssl.SSLContext
        return True
    except (ImportError, AttributeError):
        return False

@check("defaultcacerts", "can verify SSL certs by system's CA certs store")
def has_defaultcacerts():
    from mercurial import sslutil
    return sslutil._defaultcacerts() != '!'

@check("windows", "Windows")
def has_windows():
    return os.name == 'nt'

@check("system-sh", "system() uses sh")
def has_system_sh():
    return os.name != 'nt'

@check("serve", "platform and python can manage 'hg serve -d'")
def has_serve():
    return os.name != 'nt' # gross approximation

@check("test-repo", "running tests from repository")
def has_test_repo():
    t = os.environ["TESTDIR"]
    return os.path.isdir(os.path.join(t, "..", ".hg"))

@check("tic", "terminfo compiler and curses module")
def has_tic():
    try:
        import curses
        curses.COLOR_BLUE
        return matchoutput('test -x "`which tic`"', '')
    except ImportError:
        return False

@check("msys", "Windows with MSYS")
def has_msys():
    return os.getenv('MSYSTEM')

@check("aix", "AIX")
def has_aix():
    return sys.platform.startswith("aix")

@check("osx", "OS X")
def has_osx():
    return sys.platform == 'darwin'

@check("osxpackaging", "OS X packaging tools")
def has_osxpackaging():
    try:
        return (matchoutput('pkgbuild', 'Usage: pkgbuild ', ignorestatus=1)
                and matchoutput(
                    'productbuild', 'Usage: productbuild ',
                    ignorestatus=1)
                and matchoutput('lsbom', 'Usage: lsbom', ignorestatus=1)
                and matchoutput(
                    'xar --help', 'Usage: xar', ignorestatus=1))
    except ImportError:
        return False

@check("docker", "docker support")
def has_docker():
    pat = r'A self-sufficient runtime for'
    if matchoutput('docker --help', pat):
        if 'linux' not in sys.platform:
            # TODO: in theory we should be able to test docker-based
            # package creation on non-linux using boot2docker, but in
            # practice that requires extra coordination to make sure
            # $TESTTEMP is going to be visible at the same path to the
            # boot2docker VM. If we figure out how to verify that, we
            # can use the following instead of just saying False:
            # return 'DOCKER_HOST' in os.environ
            return False

        return True
    return False

@check("debhelper", "debian packaging tools")
def has_debhelper():
    dpkg = matchoutput('dpkg --version',
                       "Debian `dpkg' package management program")
    dh = matchoutput('dh --help',
                     'dh is a part of debhelper.', ignorestatus=True)
    dh_py2 = matchoutput('dh_python2 --help',
                         'other supported Python versions')
    return dpkg and dh and dh_py2

@check("absimport", "absolute_import in __future__")
def has_absimport():
    import __future__
    from mercurial import util
    return util.safehasattr(__future__, "absolute_import")

@check("py3k", "running with Python 3.x")
def has_py3k():
    return 3 == sys.version_info[0]

@check("py3exe", "a Python 3.x interpreter is available")
def has_python3exe():
    return 'PYTHON3' in os.environ

@check("pure", "running with pure Python code")
def has_pure():
    return any([
        os.environ.get("HGMODULEPOLICY") == "py",
        os.environ.get("HGTEST_RUN_TESTS_PURE") == "--pure",
    ])

@check("slow", "allow slow tests")
def has_slow():
    return os.environ.get('HGTEST_SLOW') == 'slow'

@check("hypothesis", "Hypothesis automated test generation")
def has_hypothesis():
    try:
        import hypothesis
        hypothesis.given
        return True
    except ImportError:
        return False
