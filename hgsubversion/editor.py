import errno
import cStringIO
import sys

from mercurial import util as hgutil
from mercurial import revlog
from mercurial import node

import svnwrap
import util
import svnexternals

class NeverClosingStringIO(object):
    def __init__(self):
        self._fp = cStringIO.StringIO()

    def __getattr__(self, name):
        return getattr(self._fp, name)

    def close(self):
        # svn 1.7 apply_delta driver now calls close() on passed file
        # object which prevent us from calling getvalue() afterwards.
        pass

class RevisionData(object):

    __slots__ = [
        'file', 'added', 'files', 'deleted', 'rev', 'execfiles', 'symlinks', 'batons',
        'copies', 'missing', 'emptybranches', 'base', 'externals', 'ui',
        'exception',
    ]

    def __init__(self, ui):
        self.ui = ui
        self.clear()

    def clear(self):
        self.added = set()
        self.files = {}
        self.deleted = {}
        self.rev = None
        self.execfiles = {}
        self.symlinks = {}
        self.batons = {}
        # Map fully qualified destination file paths to module source path
        self.copies = {}
        self.missing = set()
        self.emptybranches = {}
        self.externals = {}
        self.exception = None

    def set(self, path, data, isexec=False, islink=False):
        self.files[path] = data
        self.execfiles[path] = isexec
        self.symlinks[path] = islink
        if path in self.deleted:
            del self.deleted[path]
        if path in self.missing:
            self.missing.remove(path)

    def delete(self, path):
        self.deleted[path] = True
        if path in self.files:
            del self.files[path]
        self.execfiles[path] = False
        self.symlinks[path] = False
        self.ui.note('D %s\n' % path)

    def findmissing(self, svn):

        if not self.missing:
            return

        msg = 'fetching %s files that could not use replay.\n'
        self.ui.debug(msg % len(self.missing))
        root = svn.subdir and svn.subdir[1:] or ''
        r = self.rev.revnum

        files = set()
        for p in self.missing:
            self.ui.note('.')
            self.ui.flush()
            if p[-1] == '/':
                dir = p[len(root):]
                new = [p + f for f, k in svn.list_files(dir, r) if k == 'f']
                files.update(new)
            else:
                files.add(p)

        i = 1
        self.ui.note('\nfetching files...\n')
        for p in files:
            self.ui.note('.')
            self.ui.flush()
            if i % 50 == 0:
                svn.init_ra_and_client()
            i += 1
            data, mode = svn.get_file(p[len(root):], r)
            self.set(p, data, 'x' in mode, 'l' in mode)

        self.missing = set()
        self.ui.note('\n')

class EditingError(Exception):
    pass

class HgEditor(svnwrap.Editor):

    def __init__(self, meta):
        self.meta = meta
        self.ui = meta.ui
        self.repo = meta.repo
        self.current = RevisionData(meta.ui)
        self._clear()

    def _clear(self):
        self._filecounter = 0
        # A mapping of svn paths to (data, isexec, islink, copypath) tuples
        self._svncopies = {}
        # A mapping of batons to (path, data, isexec, islink, copypath) tuples
        self._openfiles = {}
        # A mapping of file paths to batons
        self._openpaths = {}

    def _openfile(self, path, data, isexec, islink, copypath, create=False):
        if path in self._openpaths:
            raise EditingError('trying to open an already opened file %s'
                    % path)
        if not create and path in self.current.deleted:
            raise EditingError('trying to open a deleted file %s' % path)
        if path in self.current.deleted:
            del self.current.deleted[path]
        self._filecounter += 1
        baton = '%d-%s' % (self._filecounter, path)
        self._openfiles[baton] = (path, data, isexec, islink, copypath)
        self._openpaths[path] = baton
        return baton

    @svnwrap.ieditor
    def delete_entry(self, path, revision_bogus, parent_baton, pool=None):
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
                del self._svncopies[f]

        if br_path is not None:
            ha = self.meta.get_parent_revision(self.current.rev.revnum, branch)
            if ha == revlog.nullid:
                return
            ctx = self.repo.changectx(ha)
            if br_path not in ctx:
                br_path2 = ''
                if br_path != '':
                    br_path2 = br_path + '/'
                # assuming it is a directory
                self.current.externals[path] = None
                for f in ctx.walk(util.PrefixMatch(br_path2)):
                    f_p = '%s/%s' % (path, f[len(br_path2):])
                    self.current.delete(f_p)
                    if f_p in self._svncopies:
                        del self._svncopies[f_p]
            self.current.delete(path)

    @svnwrap.ieditor
    def open_file(self, path, parent_baton, base_revision, p=None):
        fpath, branch = self.meta.split_branch_path(path)[:2]
        if not fpath:
            self.ui.debug('WARNING: Opening non-existant file %s\n' % path)
            return None

        self.ui.note('M %s\n' % path)

        if path in self._svncopies:
            base, isexec, islink, copypath = self._svncopies.pop(path)
            return self._openfile(path, base, isexec, islink, copypath)

        if not self.meta.is_path_valid(path):
            return None

        baserev = base_revision
        if baserev is None or baserev == -1:
            baserev = self.current.rev.revnum - 1
        # Use exact=True because during replacements ('R' action) we select
        # replacing branch as parent, but svn delta editor provides delta
        # agains replaced branch.
        parent = self.meta.get_parent_revision(baserev + 1, branch, True)
        ctx = self.repo[parent]
        if fpath not in ctx:
            self.current.missing.add(path)
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
        if path in self._svncopies:
            raise EditingError('trying to replace copied file %s' % path)
        if path in self.current.deleted:
            del self.current.deleted[path]
        fpath, branch = self.meta.split_branch_path(path, existing=False)[:2]
        if not fpath:
            return None
        if (branch not in self.meta.branches and
            not self.meta.get_path_tag(self.meta.remotename(branch))):
            # we know this branch will exist now, because it has at least one file. Rock.
            self.meta.branches[branch] = None, 0, self.current.rev.revnum
        if not copyfrom_path:
            self.ui.note('A %s\n' % path)
            self.current.added.add(path)
            return self._openfile(path, '', False, False, None, create=True)
        self.ui.note('A+ %s\n' % path)
        (from_file,
         from_branch) = self.meta.split_branch_path(copyfrom_path)[:2]
        if not from_file:
            self.current.missing.add(path)
            return None
        # Use exact=True because during replacements ('R' action) we select
        # replacing branch as parent, but svn delta editor provides delta
        # agains replaced branch.
        ha = self.meta.get_parent_revision(copyfrom_revision + 1,
                                           from_branch, True)
        ctx = self.repo.changectx(ha)
        if from_file not in ctx:
            self.current.missing.add(path)
            return None

        fctx = ctx.filectx(from_file)
        flags = fctx.flags()
        self.current.set(path, fctx.data(), 'x' in flags, 'l' in flags)
        copypath = None
        if from_branch == branch:
            parentid = self.meta.get_parent_revision(
                self.current.rev.revnum, branch)
            if parentid != revlog.nullid:
                parentctx = self.repo.changectx(parentid)
                if util.issamefile(parentctx, ctx, from_file):
                    copypath = from_file
        return self._openfile(path, fctx.data(), 'x' in flags, 'l' in flags,
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
        self.current.set(path, data, isexec, islink)
        if copypath is not None:
            self.current.copies[path] = copypath

    @svnwrap.ieditor
    def add_directory(self, path, parent_baton, copyfrom_path,
                      copyfrom_revision, dir_pool=None):
        self.current.batons[path] = path
        br_path, branch = self.meta.split_branch_path(path)[:2]
        if br_path is not None:
            if not copyfrom_path and not br_path:
                self.current.emptybranches[branch] = True
            else:
                self.current.emptybranches[branch] = False
        if br_path is None or not copyfrom_path:
            return path
        if self.meta.get_path_tag(path):
            del self.current.emptybranches[branch]
            return path
        tag = self.meta.get_path_tag(copyfrom_path)
        if tag not in self.meta.tags:
            tag = None
            if not self.meta.is_path_valid(copyfrom_path):
                self.current.missing.add('%s/' % path)
                return path
        if tag:
            changeid = self.meta.tags[tag]
            source_rev, source_branch = self.meta.get_source_rev(changeid)[:2]
            frompath = ''
        else:
            source_rev = copyfrom_revision
            frompath, source_branch = self.meta.split_branch_path(copyfrom_path)[:2]
            if frompath == '' and br_path == '':
                assert br_path is not None
                tmp = source_branch, source_rev, self.current.rev.revnum
                self.meta.branches[branch] = tmp
        new_hash = self.meta.get_parent_revision(source_rev + 1, source_branch, True)
        if new_hash == node.nullid:
            self.current.missing.add('%s/' % path)
            return path
        fromctx = self.repo.changectx(new_hash)
        if frompath != '/' and frompath != '':
            frompath = '%s/' % frompath
        else:
            frompath = ''
        svncopies = {}
        copies = {}
        for f in fromctx:
            if not f.startswith(frompath):
                continue
            fctx = fromctx.filectx(f)
            dest = path + '/' + f[len(frompath):]
            flags = fctx.flags()
            islink = 'l' in flags
            data = fctx.data()
            if islink:
                data = 'link ' + data
            svncopies[dest] = (data, 'x' in flags, islink, None)
            if dest in self.current.deleted:
                # Remove this once svn copies and edited files are
                # clearly separated.
                del self.current.deleted[dest]
            if branch == source_branch:
                copies[dest] = f
        if copies:
            # Preserve the directory copy records if no file was changed between
            # the source and destination revisions, or discard it completely.
            parentid = self.meta.get_parent_revision(
                    self.current.rev.revnum, branch)
            if parentid != revlog.nullid:
                parentctx = self.repo.changectx(parentid)
                for k, v in copies.iteritems():
                    if util.issamefile(parentctx, fromctx, v):
                        data, isexec, islink = svncopies[k][:-1]
                        svncopies[k] = (data, isexec, islink, v)
        self._svncopies.update(svncopies)

        # Copy the externals definitions of copied directories
        fromext = svnexternals.parse(self.ui, fromctx)
        for p, v in fromext.iteritems():
            pp = p and (p + '/') or ''
            if pp.startswith(frompath):
                dest = (path + '/' + pp[len(frompath):]).rstrip('/')
                self.current.externals[dest] = v
        return path

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
        if dir_baton is None:
            return
        path = self.current.batons[dir_baton]
        if name == 'svn:externals':
            self.current.externals[path] = value

    @svnwrap.ieditor
    def open_root(self, edit_baton, base_revision, dir_pool=None):
        # We should not have to reset these, unfortunately the editor is
        # reused for different revisions.
        self._clear()
        return None

    @svnwrap.ieditor
    def open_directory(self, path, parent_baton, base_revision, dir_pool=None):
        self.current.batons[path] = path
        p_, branch = self.meta.split_branch_path(path)[:2]
        if p_ == '' or (self.meta.layout == 'single' and p_):
            if not self.meta.get_path_tag(path):
                self.current.emptybranches[branch] = False
        return path

    @svnwrap.ieditor
    def close_directory(self, dir_baton, dir_pool=None):
        if dir_baton is not None:
            del self.current.batons[dir_baton]

    @svnwrap.ieditor
    def apply_textdelta(self, file_baton, base_checksum, pool=None):
        if file_baton is None:
            return lambda x: None
        if file_baton not in self._openfiles:
            raise EditingError('trying to patch a closed file %s' % file_baton)
        path, base, isexec, islink, copypath = self._openfiles[file_baton]
        if not self.meta.is_path_valid(path):
            return lambda x: None

        target = NeverClosingStringIO()
        self.stream = target

        handler = svnwrap.apply_txdelta(base, target)
        if not callable(handler): # pragma: no cover
            raise hgutil.Abort('Error in Subversion bindings: '
                               'cannot call handler!')
        def txdelt_window(window):
            try:
                if not self.meta.is_path_valid(path):
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
                    raise e

                # window being None means commit this file
                if not window:
                    self._openfiles[file_baton] = (
                        path, target.getvalue(), isexec, islink, copypath)
            except svnwrap.SubversionException, e: # pragma: no cover
                if e.args[1] == svnwrap.ERR_INCOMPLETE_DATA:
                    self.current.missing.add(path)
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

        for c in self._svncopies.iteritems():
            dest, (data, isexec, islink, copied) = c
            self.current.set(dest, data, isexec, islink)
            self.current.copies[dest] = copied
        self._svncopies.clear()

_TXDELT_WINDOW_HANDLER_FAILURE_MSG = (
    "Your SVN repository may not be supplying correct replay deltas."
    " It is strongly"
    "\nadvised that you repull the entire SVN repository using"
    " hg pull --stupid."
    "\nAlternatively, re-pull just this revision using --stupid and verify"
    " that the"
    "\nchangeset is correct."
)
