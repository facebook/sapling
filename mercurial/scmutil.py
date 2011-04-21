# scmutil.py - Mercurial core utility functions
#
#  Copyright Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import util, error, osutil
import os, errno, stat

def checkfilename(f):
    '''Check that the filename f is an acceptable filename for a tracked file'''
    if '\r' in f or '\n' in f:
        raise util.Abort(_("'\\n' and '\\r' disallowed in filenames: %r") % f)

def checkportable(ui, f):
    '''Check if filename f is portable and warn or abort depending on config'''
    checkfilename(f)
    val = ui.config('ui', 'portablefilenames', 'warn')
    lval = val.lower()
    abort = os.name == 'nt' or lval == 'abort'
    bval = util.parsebool(val)
    if abort or lval == 'warn' or bval:
        msg = util.checkwinfilename(f)
        if msg:
            if abort:
                raise util.Abort("%s: %r" % (msg, f))
            ui.warn(_("warning: %s: %r\n") % (msg, f))
    elif bval is None and lval != 'ignore':
        raise error.ConfigError(
            _("ui.portablefilenames value is invalid ('%s')") % val)

class path_auditor(object):
    '''ensure that a filesystem path contains no banned components.
    the following properties of a path are checked:

    - ends with a directory separator
    - under top-level .hg
    - starts at the root of a windows drive
    - contains ".."
    - traverses a symlink (e.g. a/symlink_here/b)
    - inside a nested repository (a callback can be used to approve
      some nested repositories, e.g., subrepositories)
    '''

    def __init__(self, root, callback=None):
        self.audited = set()
        self.auditeddir = set()
        self.root = root
        self.callback = callback

    def __call__(self, path):
        '''Check the relative path.
        path may contain a pattern (e.g. foodir/**.txt)'''

        if path in self.audited:
            return
        # AIX ignores "/" at end of path, others raise EISDIR.
        if util.endswithsep(path):
            raise util.Abort(_("path ends in directory separator: %s") % path)
        normpath = os.path.normcase(path)
        parts = util.splitpath(normpath)
        if (os.path.splitdrive(path)[0]
            or parts[0].lower() in ('.hg', '.hg.', '')
            or os.pardir in parts):
            raise util.Abort(_("path contains illegal component: %s") % path)
        if '.hg' in path.lower():
            lparts = [p.lower() for p in parts]
            for p in '.hg', '.hg.':
                if p in lparts[1:]:
                    pos = lparts.index(p)
                    base = os.path.join(*parts[:pos])
                    raise util.Abort(_('path %r is inside nested repo %r')
                                     % (path, base))

        parts.pop()
        prefixes = []
        while parts:
            prefix = os.sep.join(parts)
            if prefix in self.auditeddir:
                break
            curpath = os.path.join(self.root, prefix)
            try:
                st = os.lstat(curpath)
            except OSError, err:
                # EINVAL can be raised as invalid path syntax under win32.
                # They must be ignored for patterns can be checked too.
                if err.errno not in (errno.ENOENT, errno.ENOTDIR, errno.EINVAL):
                    raise
            else:
                if stat.S_ISLNK(st.st_mode):
                    raise util.Abort(
                        _('path %r traverses symbolic link %r')
                        % (path, prefix))
                elif (stat.S_ISDIR(st.st_mode) and
                      os.path.isdir(os.path.join(curpath, '.hg'))):
                    if not self.callback or not self.callback(curpath):
                        raise util.Abort(_('path %r is inside nested repo %r') %
                                         (path, prefix))
            prefixes.append(prefix)
            parts.pop()

        self.audited.add(path)
        # only add prefixes to the cache after checking everything: we don't
        # want to add "foo/bar/baz" before checking if there's a "foo/.hg"
        self.auditeddir.update(prefixes)

class opener(object):
    '''Open files relative to a base directory

    This class is used to hide the details of COW semantics and
    remote file access from higher level code.
    '''
    def __init__(self, base, audit=True):
        self.base = base
        if audit:
            self.auditor = path_auditor(base)
        else:
            self.auditor = util.always
        self.createmode = None
        self._trustnlink = None

    @util.propertycache
    def _can_symlink(self):
        return util.checklink(self.base)

    def _fixfilemode(self, name):
        if self.createmode is None:
            return
        os.chmod(name, self.createmode & 0666)

    def __call__(self, path, mode="r", text=False, atomictemp=False):
        r = util.checkosfilename(path)
        if r:
            raise util.Abort("%s: %r" % (r, path))
        self.auditor(path)
        f = os.path.join(self.base, path)

        if not text and "b" not in mode:
            mode += "b" # for that other OS

        nlink = -1
        dirname, basename = os.path.split(f)
        # If basename is empty, then the path is malformed because it points
        # to a directory. Let the posixfile() call below raise IOError.
        if basename and mode not in ('r', 'rb'):
            if atomictemp:
                if not os.path.isdir(dirname):
                    util.makedirs(dirname, self.createmode)
                return util.atomictempfile(f, mode, self.createmode)
            try:
                if 'w' in mode:
                    util.unlink(f)
                    nlink = 0
                else:
                    # nlinks() may behave differently for files on Windows
                    # shares if the file is open.
                    fd = util.posixfile(f)
                    nlink = util.nlinks(f)
                    if nlink < 1:
                        nlink = 2 # force mktempcopy (issue1922)
                    fd.close()
            except (OSError, IOError), e:
                if e.errno != errno.ENOENT:
                    raise
                nlink = 0
                if not os.path.isdir(dirname):
                    util.makedirs(dirname, self.createmode)
            if nlink > 0:
                if self._trustnlink is None:
                    self._trustnlink = nlink > 1 or util.checknlink(f)
                if nlink > 1 or not self._trustnlink:
                    util.rename(util.mktempcopy(f), f)
        fp = util.posixfile(f, mode)
        if nlink == 0:
            self._fixfilemode(f)
        return fp

    def symlink(self, src, dst):
        self.auditor(dst)
        linkname = os.path.join(self.base, dst)
        try:
            os.unlink(linkname)
        except OSError:
            pass

        dirname = os.path.dirname(linkname)
        if not os.path.exists(dirname):
            util.makedirs(dirname, self.createmode)

        if self._can_symlink:
            try:
                os.symlink(src, linkname)
            except OSError, err:
                raise OSError(err.errno, _('could not symlink to %r: %s') %
                              (src, err.strerror), linkname)
        else:
            f = self(dst, "w")
            f.write(src)
            f.close()
            self._fixfilemode(dst)

def canonpath(root, cwd, myname, auditor=None):
    '''return the canonical path of myname, given cwd and root'''
    if util.endswithsep(root):
        rootsep = root
    else:
        rootsep = root + os.sep
    name = myname
    if not os.path.isabs(name):
        name = os.path.join(root, cwd, name)
    name = os.path.normpath(name)
    if auditor is None:
        auditor = path_auditor(root)
    if name != rootsep and name.startswith(rootsep):
        name = name[len(rootsep):]
        auditor(name)
        return util.pconvert(name)
    elif name == root:
        return ''
    else:
        # Determine whether `name' is in the hierarchy at or beneath `root',
        # by iterating name=dirname(name) until that causes no change (can't
        # check name == '/', because that doesn't work on windows).  For each
        # `name', compare dev/inode numbers.  If they match, the list `rel'
        # holds the reversed list of components making up the relative file
        # name we want.
        root_st = os.stat(root)
        rel = []
        while True:
            try:
                name_st = os.stat(name)
            except OSError:
                break
            if util.samestat(name_st, root_st):
                if not rel:
                    # name was actually the same as root (maybe a symlink)
                    return ''
                rel.reverse()
                name = os.path.join(*rel)
                auditor(name)
                return util.pconvert(name)
            dirname, basename = os.path.split(name)
            rel.append(basename)
            if dirname == name:
                break
            name = dirname

        raise util.Abort('%s not under root' % myname)

def walkrepos(path, followsym=False, seen_dirs=None, recurse=False):
    '''yield every hg repository under path, recursively.'''
    def errhandler(err):
        if err.filename == path:
            raise err
    if followsym and hasattr(os.path, 'samestat'):
        def _add_dir_if_not_there(dirlst, dirname):
            match = False
            samestat = os.path.samestat
            dirstat = os.stat(dirname)
            for lstdirstat in dirlst:
                if samestat(dirstat, lstdirstat):
                    match = True
                    break
            if not match:
                dirlst.append(dirstat)
            return not match
    else:
        followsym = False

    if (seen_dirs is None) and followsym:
        seen_dirs = []
        _add_dir_if_not_there(seen_dirs, path)
    for root, dirs, files in os.walk(path, topdown=True, onerror=errhandler):
        dirs.sort()
        if '.hg' in dirs:
            yield root # found a repository
            qroot = os.path.join(root, '.hg', 'patches')
            if os.path.isdir(os.path.join(qroot, '.hg')):
                yield qroot # we have a patch queue repo here
            if recurse:
                # avoid recursing inside the .hg directory
                dirs.remove('.hg')
            else:
                dirs[:] = [] # don't descend further
        elif followsym:
            newdirs = []
            for d in dirs:
                fname = os.path.join(root, d)
                if _add_dir_if_not_there(seen_dirs, fname):
                    if os.path.islink(fname):
                        for hgname in walkrepos(fname, True, seen_dirs):
                            yield hgname
                    else:
                        newdirs.append(d)
            dirs[:] = newdirs

_rcpath = None

def rcpath():
    '''return hgrc search path. if env var HGRCPATH is set, use it.
    for each item in path, if directory, use files ending in .rc,
    else use item.
    make HGRCPATH empty to only look in .hg/hgrc of current repo.
    if no HGRCPATH, use default os-specific path.'''
    global _rcpath
    if _rcpath is None:
        if 'HGRCPATH' in os.environ:
            _rcpath = []
            for p in os.environ['HGRCPATH'].split(os.pathsep):
                if not p:
                    continue
                p = util.expandpath(p)
                if os.path.isdir(p):
                    for f, kind in osutil.listdir(p):
                        if f.endswith('.rc'):
                            _rcpath.append(os.path.join(p, f))
                else:
                    _rcpath.append(p)
        else:
            _rcpath = util.os_rcpath()
    return _rcpath
