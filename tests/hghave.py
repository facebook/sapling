import os, stat, socket
import re
import sys
import tempfile

tempprefix = 'hg-hghave-'

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

def has_baz():
    return matchoutput('baz --version 2>&1', r'baz Bazaar version')

def has_bzr():
    try:
        import bzrlib
        return bzrlib.__doc__ is not None
    except ImportError:
        return False

def has_bzr114():
    try:
        import bzrlib
        return (bzrlib.__doc__ is not None
                and bzrlib.version_info[:2] >= (1, 14))
    except ImportError:
        return False

def has_cvs():
    re = r'Concurrent Versions System.*?server'
    return matchoutput('cvs --version 2>&1', re) and not has_msys()

def has_cvs112():
    re = r'Concurrent Versions System \(CVS\) 1.12.*?server'
    return matchoutput('cvs --version 2>&1', re) and not has_msys()

def has_darcs():
    return matchoutput('darcs --version', r'2\.[2-9]', True)

def has_mtn():
    return matchoutput('mtn --version', r'monotone', True) and not matchoutput(
        'mtn --version', r'monotone 0\.', True)

def has_eol_in_paths():
    try:
        fd, path = tempfile.mkstemp(dir='.', prefix=tempprefix, suffix='\n\r')
        os.close(fd)
        os.remove(path)
        return True
    except (IOError, OSError):
        return False

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

def has_inotify():
    try:
        import hgext.inotify.linux.watcher
    except ImportError:
        return False
    name = tempfile.mktemp(dir='.', prefix=tempprefix)
    sock = socket.socket(socket.AF_UNIX)
    try:
        sock.bind(name)
    except socket.error:
        return False
    sock.close()
    os.unlink(name)
    return True

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

def has_killdaemons():
    return True

def has_cacheable_fs():
    from mercurial import util

    fd, path = tempfile.mkstemp(dir='.', prefix=tempprefix)
    os.close(fd)
    try:
        return util.cachestat(path).cacheable()
    finally:
        os.remove(path)

def has_lsprof():
    try:
        import _lsprof
        return True
    except ImportError:
        return False

def has_gettext():
    return matchoutput('msgfmt --version', 'GNU gettext-tools')

def has_git():
    return matchoutput('git --version 2>&1', r'^git version')

def has_docutils():
    try:
        from docutils.core import publish_cmdline
        return True
    except ImportError:
        return False

def getsvnversion():
    m = matchoutput('svn --version --quiet 2>&1', r'^(\d+)\.(\d+)')
    if not m:
        return (0, 0)
    return (int(m.group(1)), int(m.group(2)))

def has_svn15():
    return getsvnversion() >= (1, 5)

def has_svn13():
    return getsvnversion() >= (1, 3)

def has_svn():
    return matchoutput('svn --version 2>&1', r'^svn, version') and \
        matchoutput('svnadmin --version 2>&1', r'^svnadmin, version')

def has_svn_bindings():
    try:
        import svn.core
        version = svn.core.SVN_VER_MAJOR, svn.core.SVN_VER_MINOR
        if version < (1, 4):
            return False
        return True
    except ImportError:
        return False

def has_p4():
    return (matchoutput('p4 -V', r'Rev\. P4/') and
            matchoutput('p4d -V', r'Rev\. P4D/'))

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

def has_hardlink():
    from mercurial import util
    fh, fn = tempfile.mkstemp(dir='.', prefix=tempprefix)
    os.close(fh)
    name = tempfile.mktemp(dir='.', prefix=tempprefix)
    try:
        try:
            util.oslink(fn, name)
            os.unlink(name)
            return True
        except OSError:
            return False
    finally:
        os.unlink(fn)

def has_tla():
    return matchoutput('tla --version 2>&1', r'The GNU Arch Revision')

def has_gpg():
    return matchoutput('gpg --version 2>&1', r'GnuPG')

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

def has_pyflakes():
    return matchoutput("sh -c \"echo 'import re' 2>&1 | pyflakes\"",
                       r"<stdin>:1: 're' imported but unused",
                       True)

def has_pygments():
    try:
        import pygments
        return True
    except ImportError:
        return False

def has_outer_repo():
    # failing for other reasons than 'no repo' imply that there is a repo
    return not matchoutput('hg root 2>&1',
                           r'abort: no repository found', True)

def has_ssl():
    try:
        import ssl
        import OpenSSL
        OpenSSL.SSL.Context
        return True
    except ImportError:
        return False

def has_windows():
    return os.name == 'nt'

def has_system_sh():
    return os.name != 'nt'

def has_serve():
    return os.name != 'nt' # gross approximation

def has_tic():
    return matchoutput('test -x "`which tic`"', '')

def has_msys():
    return os.getenv('MSYSTEM')

def has_aix():
    return sys.platform.startswith("aix")

def has_absimport():
    import __future__
    from mercurial import util
    return util.safehasattr(__future__, "absolute_import")

def has_py3k():
    return 3 == sys.version_info[0]

checks = {
    "true": (lambda: True, "yak shaving"),
    "false": (lambda: False, "nail clipper"),
    "baz": (has_baz, "GNU Arch baz client"),
    "bzr": (has_bzr, "Canonical's Bazaar client"),
    "bzr114": (has_bzr114, "Canonical's Bazaar client >= 1.14"),
    "cacheable": (has_cacheable_fs, "cacheable filesystem"),
    "cvs": (has_cvs, "cvs client/server"),
    "cvs112": (has_cvs112, "cvs client/server >= 1.12"),
    "darcs": (has_darcs, "darcs client"),
    "docutils": (has_docutils, "Docutils text processing library"),
    "eol-in-paths": (has_eol_in_paths, "end-of-lines in paths"),
    "execbit": (has_executablebit, "executable bit"),
    "fifo": (has_fifo, "named pipes"),
    "gettext": (has_gettext, "GNU Gettext (msgfmt)"),
    "git": (has_git, "git command line client"),
    "gpg": (has_gpg, "gpg client"),
    "hardlink": (has_hardlink, "hardlinks"),
    "icasefs": (has_icasefs, "case insensitive file system"),
    "inotify": (has_inotify, "inotify extension support"),
    "killdaemons": (has_killdaemons, 'killdaemons.py support'),
    "lsprof": (has_lsprof, "python lsprof module"),
    "mtn": (has_mtn, "monotone client (>= 1.0)"),
    "outer-repo": (has_outer_repo, "outer repo"),
    "p4": (has_p4, "Perforce server and client"),
    "pyflakes": (has_pyflakes, "Pyflakes python linter"),
    "pygments": (has_pygments, "Pygments source highlighting library"),
    "serve": (has_serve, "platform and python can manage 'hg serve -d'"),
    "ssl": (has_ssl, "python >= 2.6 ssl module and python OpenSSL"),
    "svn": (has_svn, "subversion client and admin tools"),
    "svn13": (has_svn13, "subversion client and admin tools >= 1.3"),
    "svn15": (has_svn15, "subversion client and admin tools >= 1.5"),
    "svn-bindings": (has_svn_bindings, "subversion python bindings"),
    "symlink": (has_symlink, "symbolic links"),
    "system-sh": (has_system_sh, "system() uses sh"),
    "tic": (has_tic, "terminfo compiler"),
    "tla": (has_tla, "GNU Arch tla client"),
    "unix-permissions": (has_unix_permissions, "unix-style permissions"),
    "windows": (has_windows, "Windows"),
    "msys": (has_msys, "Windows with MSYS"),
    "aix": (has_aix, "AIX"),
    "absimport": (has_absimport, "absolute_import in __future__"),
    "py3k": (has_py3k, "running with Python 3.x"),
}
