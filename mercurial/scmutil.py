# scmutil.py - Mercurial core utility functions
#
#  Copyright Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import util, error
import os, errno

def checkportable(ui, f):
    '''Check if filename f is portable and warn or abort depending on config'''
    util.checkfilename(f)
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

class opener(object):
    '''Open files relative to a base directory

    This class is used to hide the details of COW semantics and
    remote file access from higher level code.
    '''
    def __init__(self, base, audit=True):
        self.base = base
        if audit:
            self.auditor = util.path_auditor(base)
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
            raise Abort("%s: %r" % (r, path))
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
        auditor = util.path_auditor(root)
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
