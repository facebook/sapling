import errno
import sys
import tempfile
import shutil
import os

from mercurial import util as hgutil
from mercurial import revlog
from mercurial import node

import svnwrap
import util
import svnexternals

class EditingError(Exception):
    pass

class FileStore(object):
    def __init__(self, maxsize=None):
        self._tempdir = None
        self._files = {}
        self._created = 0
        self._maxsize = maxsize
        if self._maxsize is None:
            self._maxsize = 100*(2**20)
        self._size = 0
        self._data = {}
        self._popped = set()

    def setfile(self, fname, data):
        if fname in self._popped:
            raise EditingError('trying to set a popped file %s' % fname)

        if fname in self._data:
            self._size -= len(self._data[fname])
            del self._data[fname]

        if fname in self._files:
            del self._files[fname]

        if self._maxsize < 0 or (len(data) + self._size) <= self._maxsize:
            self._data[fname] = data
            self._size += len(data)
        else:
            if self._tempdir is None:
                self._tempdir = tempfile.mkdtemp(prefix='hg-subversion-')
            # Avoid filename issues with these simple names
            fn = str(self._created)
            fp = hgutil.posixfile(os.path.join(self._tempdir, fn), 'wb')
            try:
                fp.write(data)
            finally:
                fp.close()
            self._created += 1
            self._files[fname] = fn

    def delfile(self, fname):
        if fname in self._popped:
            raise EditingError('trying to delete a popped file %s' % fname)

        if fname in self._data:
            del self._data[fname]
        elif fname in self._files:
            path = os.path.join(self._tempdir, self._files.pop(fname))
            os.unlink(path)

    def getfile(self, fname):
        if fname in self._popped:
            raise EditingError('trying to get a popped file %s' % fname)

        if fname in self._data:
            return self._data[fname]
        if self._tempdir is None or fname not in self._files:
            raise IOError
        path = os.path.join(self._tempdir, self._files[fname])
        fp = hgutil.posixfile(path, 'rb')
        try:
            return fp.read()
        finally:
            fp.close()

    def popfile(self, fname):
        self.delfile(fname)
        self._popped.add(fname)

    def files(self):
        return list(self._files) + list(self._data)

    def close(self):
        if self._tempdir is not None:
            tempdir, self._tempdir = self._tempdir, None
            shutil.rmtree(tempdir)
        self._files = None
        self._data = None

class RevisionData(object):

    __slots__ = [
        'file', 'added', 'deleted', 'rev', 'execfiles', 'symlinks',
        'copies', 'emptybranches', 'base', 'externals', 'ui',
        'exception', 'store',
    ]

    def __init__(self, ui):
        self.ui = ui
        self.clear()

    def clear(self):
        oldstore = getattr(self, 'store', None)
        if oldstore is not None:
            oldstore.close()
        self.store = FileStore(util.getfilestoresize(self.ui))
        self.added = set()
        self.deleted = {}
        self.rev = None
        self.execfiles = {}
        self.symlinks = {}
        # Map fully qualified destination file paths to module source path
        self.copies = {}
        self.emptybranches = {}
        self.externals = {}
        self.exception = None

    def set(self, path, data, isexec=False, islink=False, copypath=None):
        self.store.setfile(path, data)
        self.execfiles[path] = isexec
        self.symlinks[path] = islink
        if path in self.deleted:
            del self.deleted[path]
        if copypath is not None:
            self.copies[path] = copypath

    def get(self, path):
        if path in self.deleted:
            raise IOError(errno.ENOENT, '%s is deleted' % path)
        data = self.store.getfile(path)
        isexec = self.execfiles.get(path)
        islink = self.symlinks.get(path)
        copied = self.copies.get(path)
        return data, isexec, islink, copied

    def pop(self, path):
        ret = self.get(path)
        self.store.popfile(path)
        return ret

    def delete(self, path):
        self.deleted[path] = True
        self.store.delfile(path)
        self.execfiles[path] = False
        self.symlinks[path] = False
        self.ui.note('D %s\n' % path)

    def files(self):
        """Return a sorted list of changed files."""
        files = set(self.store.files())
        for g in (self.symlinks, self.execfiles, self.deleted):
            files.update(g)
        return sorted(files)

    def close(self):
        self.store.close()

class CopiedFile(object):
    def __init__(self, node, path, copypath):
        self.node = node
        self.path = path
        self.copypath = copypath

    def resolve(self, getctxfn, ctx=None):
        if ctx is None:
            ctx = getctxfn(self.node)
        fctx = ctx[self.path]
        data = fctx.data()
        flags = fctx.flags()
        islink = 'l' in flags
        if islink:
            data = 'link ' + data
        return data, 'x' in flags, islink, self.copypath

class HgEditor(svnwrap.Editor):

    def __init__(self, meta):
        self.meta = meta
        self.ui = meta.ui
        self.repo = meta.repo
        self.current = RevisionData(meta.ui)
        self._clear()

    def setsvn(self, svn):
        self._svn = svn

    def _clear(self):
        self._filecounter = 0
        # A mapping of svn paths to CopiedFile entries
        self._svncopies = {}
        # A mapping of batons to (path, data, isexec, islink, copypath) tuples
        # data is a SimpleStringIO if the file was edited, a string
        # otherwise.
        self._openfiles = {}
        # A mapping of file paths to batons
        self._openpaths = {}
        self._deleted = set()
        self._getctx = hgutil.lrucachefunc(self.repo.changectx)
        # A map from directory baton to path
        self._opendirs = {}
        self._missing = set()

    def _openfile(self, path, data, isexec, islink, copypath, create=False):
        if path in self._openpaths:
            raise EditingError('trying to open an already opened file %s'
                    % path)
        if not create and path in self._deleted:
            raise EditingError('trying to open a deleted file %s' % path)
        if path in self._deleted:
            self._deleted.remove(path)
        self._filecounter += 1
        baton = 'f%d-%s' % (self._filecounter, path)
        self._openfiles[baton] = (path, data, isexec, islink, copypath)
        self._openpaths[path] = baton
        return baton

    def _opendir(self, path):
        self._filecounter += 1
        baton = 'f%d-%s' % (self._filecounter, path)
        self._opendirs[baton] = path
        return baton

    def _checkparentdir(self, baton):
        if not self._opendirs or baton not in self._opendirs:
            raise EditingError('trying to operate on an already closed '
                'directory: %s' % baton)

    def _deletefile(self, path):
        if self.meta.is_path_valid(path):
            self._deleted.add(path)
        if path in self._svncopies:
            del self._svncopies[path]
        self._missing.discard(path)

    def addmissing(self, path, isdir=False):
        svn = self._svn
        root = svn.subdir and svn.subdir[1:] or ''
        if not isdir:
            self._missing.add(path[len(root):])
        else:
            # Resolve missing directories content immediately so the
            # missing files maybe processed by delete actions.
            # we remove the missing directory entries to deal with the case
            # where a directory is replaced from e.g. a closed branch
            # this will show up as a delete and then a copy
            # we process deletes after missing, so we can handle a directory
            # copy plus delete of file in that directory.  This means that we
            # need to be sure that only things whose final disposition is
            # deletion remain in self._deleted at the end of the editing process.
            rev = self.current.rev.revnum
            path = path + '/'
            parentdir = path[len(root):]
            for f, k in svn.list_files(parentdir, rev):
                if k != 'f':
                    continue
                f = parentdir + f
                if not self.meta.is_path_valid(f, False):
                    continue
                self._deleted.discard(f)
                self._missing.add(f)

    @svnwrap.ieditor
    def delete_entry(self, path, revision_bogus, parent_baton, pool=None):
        self._checkparentdir(parent_baton)
        br_path, branch = self.meta.split_branch_path(path)[:2]
        if br_path == '':
            if self.meta.get_path_tag(path):
                # Tag deletion is not handled as branched deletion
                return
            self.meta.closebranches.add(branch)

        # Delete copied entries, no need to check they exist in hg
        # parent revision.
        if path in self._svncopies:
            del self._svncopies[path]
        prefix = path + '/'
        for f in list(self._svncopies):
            if f.startswith(prefix):
                self._deletefile(f)
        if path in self._missing:
            self._missing.remove(path)
        else:
            for f in list(self._missing):
                if f.startswith(prefix):
                    self._missing.remove(f)

        if br_path is not None:
            ha = self.meta.get_parent_revision(self.current.rev.revnum, branch)
            if ha == revlog.nullid:
                return
            ctx = self._getctx(ha)
            if br_path not in ctx:
                br_path2 = ''
                if br_path != '':
                    br_path2 = br_path + '/'
                # assuming it is a directory
                self.current.externals[path] = None
                for f in ctx.walk(util.PrefixMatch(br_path2)):
                    f_p = '%s/%s' % (path, f[len(br_path2):])
                    self._deletefile(f_p)
            self._deletefile(path)

    @svnwrap.ieditor
    def open_file(self, path, parent_baton, base_revision, p=None):
        self._checkparentdir(parent_baton)
        if not self.meta.is_path_valid(path):
            return None
        fpath, branch = self.meta.split_branch_path(path)[:2]

        self.ui.note('M %s\n' % path)

        if path in self._svncopies:
            copy = self._svncopies.pop(path)
            base, isexec, islink, copypath = copy.resolve(self._getctx)
            return self._openfile(path, base, isexec, islink, copypath)

        baserev = base_revision
        if baserev is None or baserev == -1:
            baserev = self.current.rev.revnum - 1
        # Use exact=True because during replacements ('R' action) we select
        # replacing branch as parent, but svn delta editor provides delta
        # agains replaced branch.
        parent = self.meta.get_parent_revision(baserev + 1, branch, True)
        ctx = self._getctx(parent)
        if fpath not in ctx:
            self.addmissing(path)
            return None

        fctx = ctx.filectx(fpath)
        base = fctx.data()
        flags = fctx.flags()
        if 'l' in flags:
            base = 'link ' + base
        return self._openfile(path, base, 'x' in flags, 'l' in flags, None)

    @svnwrap.ieditor
    def add_file(self, path, parent_baton=None, copyfrom_path=None,
                 copyfrom_revision=None, file_pool=None):
        self._checkparentdir(parent_baton)
        # Use existing=False because we use the fact a file is being
        # added here to populate the branchmap which is used with
        # existing=True.
        fpath, branch = self.meta.split_branch_path(path, existing=False)[:2]
        if not fpath or fpath not in self.meta.filemap:
            return None
        if path in self._svncopies:
            raise EditingError('trying to replace copied file %s' % path)
        if path in self._deleted:
            self._deleted.remove(path)
        if (branch not in self.meta.branches and
            not self.meta.get_path_tag(self.meta.remotename(branch))):
            # we know this branch will exist now, because it has at
            # least one file. Rock.
            self.meta.branches[branch] = None, 0, self.current.rev.revnum
        if not copyfrom_path:
            self.ui.note('A %s\n' % path)
            self.current.added.add(path)
            return self._openfile(path, '', False, False, None, create=True)
        self.ui.note('A+ %s\n' % path)
        (from_file,
         from_branch) = self.meta.split_branch_path(copyfrom_path)[:2]
        if not from_file:
            self.addmissing(path)
            return None
        # Use exact=True because during replacements ('R' action) we select
        # replacing branch as parent, but svn delta editor provides delta
        # agains replaced branch.
        ha = self.meta.get_parent_revision(copyfrom_revision + 1,
                                           from_branch, True)
        ctx = self._getctx(ha)
        if from_file not in ctx:
            self.addmissing(path)
            return None

        fctx = ctx.filectx(from_file)
        flags = fctx.flags()
        base = fctx.data()
        if 'l' in flags:
            base = 'link ' + base
        self.current.set(path, base, 'x' in flags, 'l' in flags)
        copypath = None
        if from_branch == branch:
            parentid = self.meta.get_parent_revision(
                self.current.rev.revnum, branch)
            if parentid != revlog.nullid:
                parentctx = self._getctx(parentid)
                if util.issamefile(parentctx, ctx, from_file):
                    copypath = from_file
        return self._openfile(path, base, 'x' in flags, 'l' in flags,
                copypath, create=True)

    @svnwrap.ieditor
    def close_file(self, file_baton, checksum, pool=None):
        if file_baton is None:
            return
        if file_baton not in self._openfiles:
            raise EditingError('trying to close a non-open file %s'
                    % file_baton)
        path, data, isexec, islink, copypath = self._openfiles.pop(file_baton)
        del self._openpaths[path]
        if not isinstance(data, basestring):
            # Files can be opened, properties changed and apply_text
            # never called, in which case data is still a string.
            data = data.getvalue()
        self.current.set(path, data, isexec, islink, copypath)

    @svnwrap.ieditor
    def add_directory(self, path, parent_baton, copyfrom_path,
                      copyfrom_revision, dir_pool=None):
        self._checkparentdir(parent_baton)
        baton = self._opendir(path)

        br_path, branch = self.meta.split_branch_path(path)[:2]
        if br_path is not None:
            if not copyfrom_path and not br_path:
                # This handles the case where a branch root is
                # replaced without copy info.  It will show up as a
                # deletion and then an add.
                self.meta.closebranches.discard(branch)
                self.current.emptybranches[branch] = True
            else:
                self.current.emptybranches[branch] = False
        if br_path is None or not copyfrom_path:
            return baton
        if self.meta.get_path_tag(path):
            del self.current.emptybranches[branch]
            return baton
        tag = self.meta.get_path_tag(copyfrom_path)
        if tag not in self.meta.tags:
            tag = None
            if not self.meta.is_path_valid(copyfrom_path, existing=False):
                # The source path only exists at copyfrom_revision, use
                # existing=False to guess a possible branch location and
                # test it against the filemap. The actual path and
                # revision will be resolved below if necessary.
                self.addmissing(path, isdir=True)
                return baton
        if tag:
            changeid = self.meta.tags[tag]
            source_rev, source_branch = self.meta.get_source_rev(changeid)[:2]
            frompath = ''
        else:
            source_rev = copyfrom_revision
            frompath, source_branch = self.meta.split_branch_path(copyfrom_path)[:2]
        new_hash = self.meta.get_parent_revision(source_rev + 1, source_branch, True)
        if frompath is None or new_hash == node.nullid:
            self.addmissing(path, isdir=True)
            return baton
        fromctx = self._getctx(new_hash)
        if frompath != '/' and frompath != '':
            frompath = '%s/' % frompath
        else:
            frompath = ''

        copyfromparent = False
        if frompath == '' and br_path == '':
            pnode = self.meta.get_parent_revision(
                    self.current.rev.revnum, branch)
            if pnode == new_hash:
                # Data parent is topological parent and relative paths
                # are the same, not need to do anything but restore
                # files marked as deleted.
                copyfromparent = True
            # Get the parent which would have been used for this branch
            # without the replace action.
            oldpnode = self.meta.get_parent_revision(
                    self.current.rev.revnum, branch, exact=True)
            if (oldpnode != revlog.nullid
                    and util.isancestor(self._getctx(oldpnode), fromctx)):
                # Branch-wide replacement, unmark the branch as deleted
                self.meta.closebranches.discard(branch)

        svncopies = {}
        copies = {}
        for f in fromctx:
            if not f.startswith(frompath):
                continue
            dest = path + '/' + f[len(frompath):]
            if not self.meta.is_path_valid(dest):
                continue
            if dest in self._deleted:
                self._deleted.remove(dest)
            if copyfromparent:
                continue
            svncopies[dest] = CopiedFile(new_hash, f, None)
            if branch == source_branch:
                copies[dest] = f
        if copies:
            # Preserve the directory copy records if no file was changed between
            # the source and destination revisions, or discard it completely.
            parentid = self.meta.get_parent_revision(
                    self.current.rev.revnum, branch)
            if parentid != revlog.nullid:
                parentctx = self._getctx(parentid)
                for k, v in copies.iteritems():
                    if util.issamefile(parentctx, fromctx, v):
                        svncopies[k].copypath = v
        self._svncopies.update(svncopies)

        # Copy the externals definitions of copied directories
        fromext = svnexternals.parse(self.ui, fromctx)
        for p, v in fromext.iteritems():
            pp = p and (p + '/') or ''
            if pp.startswith(frompath):
                dest = (path + '/' + pp[len(frompath):]).rstrip('/')
                self.current.externals[dest] = v
        return baton

    @svnwrap.ieditor
    def change_file_prop(self, file_baton, name, value, pool=None):
        if file_baton is None:
            return
        path, data, isexec, islink, copypath = self._openfiles[file_baton]
        changed = False
        if name == 'svn:executable':
            changed = True
            isexec = bool(value is not None)
        elif name == 'svn:special':
            changed = True
            islink = bool(value is not None)
        if changed:
            self._openfiles[file_baton] = (path, data, isexec, islink, copypath)

    @svnwrap.ieditor
    def change_dir_prop(self, dir_baton, name, value, pool=None):
        self._checkparentdir(dir_baton)
        if len(self._opendirs) == 1:
            return
        path = self._opendirs[dir_baton]
        if name == 'svn:externals':
            self.current.externals[path] = value

    @svnwrap.ieditor
    def open_root(self, edit_baton, base_revision, dir_pool=None):
        # We should not have to reset these, unfortunately the editor is
        # reused for different revisions.
        self._clear()
        return self._opendir('')

    @svnwrap.ieditor
    def open_directory(self, path, parent_baton, base_revision, dir_pool=None):
        self._checkparentdir(parent_baton)
        baton = self._opendir(path)
        p_, branch = self.meta.split_branch_path(path)[:2]
        if p_ == '' or (self.meta.layout == 'single' and p_):
            if not self.meta.get_path_tag(path):
                self.current.emptybranches[branch] = False
        return baton

    @svnwrap.ieditor
    def close_directory(self, dir_baton, dir_pool=None):
        self._checkparentdir(dir_baton)
        del self._opendirs[dir_baton]

    @svnwrap.ieditor
    def apply_textdelta(self, file_baton, base_checksum, pool=None):
        if file_baton is None:
            return lambda x: None
        if file_baton not in self._openfiles:
            raise EditingError('trying to patch a closed file %s' % file_baton)
        path, base, isexec, islink, copypath = self._openfiles[file_baton]
        if not isinstance(base, basestring):
            raise EditingError('trying to edit a file again: %s' % path)
        if not self.meta.is_path_valid(path):
            return lambda x: None

        target = svnwrap.SimpleStringIO(closing=False)
        self.stream = target

        handler = svnwrap.apply_txdelta(base, target)
        if not callable(handler): # pragma: no cover
            raise hgutil.Abort('Error in Subversion bindings: '
                               'cannot call handler!')
        def txdelt_window(window):
            try:
                if not self.meta.is_path_valid(path):
                    return

                # are we skipping this branch entirely?
                br_path, branch = self.meta.split_branch_path(path)[:2]
                if self.meta.skipbranch(branch):
                    return

                try:
                    handler(window)
                except AssertionError, e: # pragma: no cover
                    # Enhance the exception message
                    msg, others = e.args[0], e.args[1:]

                    if msg:
                        msg += '\n'

                    msg += _TXDELT_WINDOW_HANDLER_FAILURE_MSG
                    e.args = (msg,) + others

                    # re-raising ensures that we show the full stack trace
                    raise

                # window being None means commit this file
                if not window:
                    self._openfiles[file_baton] = (
                        path, target, isexec, islink, copypath)
            except svnwrap.SubversionException, e: # pragma: no cover
                self.ui.traceback()
                if e.args[1] == svnwrap.ERR_INCOMPLETE_DATA:
                    self.addmissing(path)
                else: # pragma: no cover
                    raise hgutil.Abort(*e.args)
            except: # pragma: no cover
                self._exception_info = sys.exc_info()
                raise
        return txdelt_window

    def close(self):
        if self._openfiles:
            for e in self._openfiles.itervalues():
                self.ui.debug('error: %s was not closed\n' % e[0])
            raise EditingError('%d edited files were not closed'
                    % len(self._openfiles))

        if self._opendirs:
            raise EditingError('directory %s was not closed'
                % self._opendirs.keys()[-1])

        # Resolve by changelog entries to avoid extra reads
        nodes = {}
        for path, copy in self._svncopies.iteritems():
            nodes.setdefault(copy.node, []).append((path, copy))
        for node, copies in nodes.iteritems():
            for path, copy in copies:
                data, isexec, islink, copied = copy.resolve(self._getctx)
                self.current.set(path, data, isexec, islink, copied)
        self._svncopies.clear()

        # Resolve missing files
        if self._missing:
            missing = sorted(self._missing)
            self.ui.debug('fetching %s files that could not use replay.\n'
                    % len(missing))
            if self.ui.configbool('hgsubversion', 'failonmissing', False):
                raise EditingError('missing entry: %s' % missing[0])

            svn = self._svn
            rev = self.current.rev.revnum
            root = svn.subdir and svn.subdir[1:] or ''
            i = 1
            for f in missing:
                if self.ui.debugflag:
                    self.ui.debug('fetching %s\n' % f)
                else:
                    self.ui.note('.')
                self.ui.flush()
                if i % 50 == 0:
                    svn.init_ra_and_client()
                i += 1
                data, mode = svn.get_file(f, rev)
                self.current.set(root + f, data, 'x' in mode, 'l' in mode)
            if not self.ui.debugflag:
                self.ui.note('\n')

        for f in self._deleted:
            self.current.delete(f)
        self._deleted.clear()

_TXDELT_WINDOW_HANDLER_FAILURE_MSG = (
    "Your SVN repository may not be supplying correct replay deltas."
    " It is strongly"
    "\nadvised that you repull the entire SVN repository using"
    " hg pull --stupid."
    "\nAlternatively, re-pull just this revision using --stupid and verify"
    " that the"
    "\nchangeset is correct."
)
