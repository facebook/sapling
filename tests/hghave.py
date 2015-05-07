import os, stat
import re
import socket
import sys
import tempfile

tempprefix = 'hg-hghave-'

checks = {
    "true": (lambda: True, "yak shaving"),
    "false": (lambda: False, "nail clipper"),
}

def check(name, desc):
    def decorator(func):
        checks[name] = (func, desc)
        return func
    return decorator

def matchoutput(cmd, regexp, ignorestatus=False):
    """Return True if cmd executes successfully and its output
    is matched by the supplied regular expression.
    """
    r = re.compile(regexp)
    fh = os.popen(cmd)
    s = fh.read()
    try:
        ret = fh.close()
    except IOError:
        # Happen in Windows test environment
        ret = 1
    return (ignorestatus or ret is None) and r.search(s)

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

@check("bzr114", "Canonical's Bazaar client >= 1.14")
def has_bzr114():
    try:
        import bzrlib
        return (bzrlib.__doc__ is not None
                and bzrlib.version_info[:2] >= (1, 14))
    except ImportError:
        return False

@check("cvs", "cvs client/server")
def has_cvs():
    re = r'Concurrent Versions System.*?server'
    return matchoutput('cvs --version 2>&1', re) and not has_msys()

@check("cvs112", "cvs client/server >= 1.12")
def has_cvs112():
    re = r'Concurrent Versions System \(CVS\) 1.12.*?server'
    return matchoutput('cvs --version 2>&1', re) and not has_msys()

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
            m = os.stat(fn).st_mode & 0777
            new_file_has_exec = m & EXECFLAGS
            os.chmod(fn, m ^ EXECFLAGS)
            exec_flags_cannot_flip = ((os.stat(fn).st_mode & 0777) == m)
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

@check("gettext", "GNU Gettext (msgfmt)")
def has_gettext():
    return matchoutput('msgfmt --version', 'GNU gettext-tools')

@check("git", "git command line client")
def has_git():
    return matchoutput('git --version 2>&1', r'^git version')

@check("docutils", "Docutils text processing library")
def has_docutils():
    try:
        from docutils.core import publish_cmdline
        publish_cmdline # silence unused import
        return True
    except ImportError:
        return False

def getsvnversion():
    m = matchoutput('svn --version --quiet 2>&1', r'^(\d+)\.(\d+)')
    if not m:
        return (0, 0)
    return (int(m.group(1)), int(m.group(2)))

@check("svn15", "subversion client and admin tools >= 1.5")
def has_svn15():
    return getsvnversion() >= (1, 5)

@check("svn13", "subversion client and admin tools >= 1.3")
def has_svn13():
    return getsvnversion() >= (1, 3)

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
        for umask in (077, 007, 022):
            os.umask(umask)
            f = open(fname, 'w')
            f.close()
            mode = os.stat(fname).st_mode
            os.unlink(fname)
            if mode & 0777 != ~umask & 0666:
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

@check("json", "some json module available")
def has_json():
    try:
        import json
        json.dumps
        return True
    except ImportError:
        try:
            import simplejson as json
            json.dumps
            return True
        except ImportError:
            pass
    return False

@check("outer-repo", "outer repo")
def has_outer_repo():
    # failing for other reasons than 'no repo' imply that there is a repo
    return not matchoutput('hg root 2>&1',
                           r'abort: no repository found', True)

@check("ssl", ("(python >= 2.6 ssl module and python OpenSSL) "
               "OR python >= 2.7.9 ssl"))
def has_ssl():
    try:
        import ssl
        if getattr(ssl, 'create_default_context', False):
            return True
        import OpenSSL
        OpenSSL.SSL.Context
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

@check("absimport", "absolute_import in __future__")
def has_absimport():
    import __future__
    from mercurial import util
    return util.safehasattr(__future__, "absolute_import")

@check("py3k", "running with Python 3.x")
def has_py3k():
    return 3 == sys.version_info[0]
