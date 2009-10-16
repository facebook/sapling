import cStringIO
import sys

from mercurial import util as hgutil
from mercurial import revlog
from mercurial import node
from svn import delta
from svn import core

import util


def ieditor(fn):
    """Helps identify methods used by the SVN editor interface.

    Stash any exception raised in the method on self.

    This is required because the SWIG bindings just mutate any exception into
    a generic Subversion exception with no way of telling what the original was.
    This allows the editor object to notice when you try and commit and really
    got an exception in the replay process.
    """
    def fun(self, *args, **kwargs):
        try:
            return fn(self, *args, **kwargs)
        except: #pragma: no cover
            if self.current.exception is not None:
                self.current.exception = sys.exc_info()
            raise
    return fun


class RevisionData(object):

    __slots__ = [
        'file', 'files', 'deleted', 'rev', 'execfiles', 'symlinks', 'batons',
        'copies', 'missing', 'emptybranches', 'base', 'externals', 'ui',
        'exception',
    ]

    def __init__(self, ui):
        self.ui = ui
        self.clear()

    def clear(self):
        self.file = None
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
        self.base = None
        self.externals = {}
        self.exception = None

    def set(self, path, data, isexec=False, islink=False):
        if islink:
            data = 'link ' + data
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
                new = [dir + f for f, k in svn.list_files(dir, r) if k == 'f']
                files.update(new)
            else:
                files.add(p[len(root):])

        i = 1
        self.ui.note('\nfetching files...\n')
        for p in files:
            self.ui.note('.')
            self.ui.flush()
            if i % 50 == 0:
                svn.init_ra_and_client()
            i += 1
            data, mode = svn.get_file(p, r)
            self.set(p, data, 'x' in mode, 'l' in mode)

        self.missing = set()
        self.ui.note('\n')


class HgEditor(delta.Editor):

    def __init__(self, meta):
        self.meta = meta
        self.ui = meta.ui
        self.repo = meta.repo
        self.current = RevisionData(meta.ui)

    @ieditor
    def delete_entry(self, path, revision_bogus, parent_baton, pool=None):
        br_path, branch = self.meta.split_branch_path(path)[:2]
        if br_path == '':
            self.meta.closebranches.add(branch)
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
                map(self.current.delete, [pat for pat in self.current.files.iterkeys()
                                          if pat.startswith(path+'/')])
                for f in ctx.walk(util.PrefixMatch(br_path2)):
                    f_p = '%s/%s' % (path, f[len(br_path2):])
                    if f_p not in self.current.files:
                        self.current.delete(f_p)
            self.current.delete(path)

    @ieditor
    def open_file(self, path, parent_baton, base_revision, p=None):
        self.current.file = None
        fpath, branch = self.meta.split_branch_path(path)[:2]
        if not fpath:
            self.ui.debug('WARNING: Opening non-existant file %s\n' % path)
            return

        self.current.file = path
        self.ui.note('M %s\n' % path)
        if base_revision != -1:
            self.current.base = base_revision
        else:
            self.current.base = None

        if self.current.file in self.current.files:
            return

        baserev = base_revision
        if baserev is None or baserev == -1:
            baserev = self.current.rev.revnum - 1
        parent = self.meta.get_parent_revision(baserev + 1, branch)

        ctx = self.repo[parent]
        if not self.meta.is_path_valid(path):
            return

        if fpath not in ctx:
            self.current.missing.add(path)

        fctx = ctx.filectx(fpath)
        base = fctx.data()
        if 'l' in fctx.flags():
            base = 'link ' + base
        self.current.set(path, base, 'x' in fctx.flags(), 'l' in fctx.flags())

    @ieditor
    def add_file(self, path, parent_baton=None, copyfrom_path=None,
                 copyfrom_revision=None, file_pool=None):
        self.current.file = None
        self.current.base = None
        if path in self.current.deleted:
            del self.current.deleted[path]
        fpath, branch = self.meta.split_branch_path(path, existing=False)[:2]
        if not fpath:
            return
        if (branch not in self.meta.branches and
            not self.meta.is_path_tag(self.meta.remotename(branch))):
            # we know this branch will exist now, because it has at least one file. Rock.
            self.meta.branches[branch] = None, 0, self.current.rev.revnum
        self.current.file = path
        if not copyfrom_path:
            self.ui.note('A %s\n' % path)
            self.current.set(path, '', False, False)
            return
        self.ui.note('A+ %s\n' % path)
        (from_file,
         from_branch) = self.meta.split_branch_path(copyfrom_path)[:2]
        if not from_file:
            self.current.missing.add(path)
            return
        ha = self.meta.get_parent_revision(copyfrom_revision + 1,
                                           from_branch)
        ctx = self.repo.changectx(ha)
        if from_file in ctx:
            fctx = ctx.filectx(from_file)
            flags = fctx.flags()
            self.current.set(path, fctx.data(), 'x' in flags, 'l' in flags)
        if from_branch == branch:
            parentid = self.meta.get_parent_revision(self.current.rev.revnum,
                                                     branch)
            if parentid != revlog.nullid:
                parentctx = self.repo.changectx(parentid)
                if util.aresamefiles(parentctx, ctx, [from_file]):
                    self.current.copies[path] = from_file

    @ieditor
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
        if copyfrom_path:
            tag = self.meta.is_path_tag(copyfrom_path)
            if tag not in self.meta.tags:
                tag = None
            if not self.meta.is_path_valid(copyfrom_path) and not tag:
                self.current.missing.add('%s/' % path)
                return path
        if tag:
            ci = self.meta.repo[self.meta.tags[tag]].extra()['convert_revision']
            source_rev, source_branch, = self.meta.parse_converted_revision(ci)
            cp_f = ''
        else:
            source_rev = copyfrom_revision
            cp_f, source_branch = self.meta.split_branch_path(copyfrom_path)[:2]
            if cp_f == '' and br_path == '':
                assert br_path is not None
                tmp = source_branch, source_rev, self.current.rev.revnum
                self.meta.branches[branch] = tmp
        new_hash = self.meta.get_parent_revision(source_rev + 1, source_branch)
        if new_hash == node.nullid:
            self.current.missing.add('%s/' % path)
            return path
        cp_f_ctx = self.repo.changectx(new_hash)
        if cp_f != '/' and cp_f != '':
            cp_f = '%s/' % cp_f
        else:
            cp_f = ''
        copies = {}
        for f in cp_f_ctx:
            if not f.startswith(cp_f):
                continue
            f2 = f[len(cp_f):]
            fctx = cp_f_ctx.filectx(f)
            fp_c = path + '/' + f2
            self.current.set(fp_c, fctx.data(), 'x' in fctx.flags(), 'l' in fctx.flags())
            if fp_c in self.current.deleted:
                del self.current.deleted[fp_c]
            if branch == source_branch:
                copies[fp_c] = f
        if copies:
            # Preserve the directory copy records if no file was changed between
            # the source and destination revisions, or discard it completely.
            parentid = self.meta.get_parent_revision(self.current.rev.revnum, branch)
            if parentid != revlog.nullid:
                parentctx = self.repo.changectx(parentid)
                for k, v in copies.iteritems():
                    if util.aresamefiles(parentctx, cp_f_ctx, [v]):
                        self.current.copies.update({k: v})
        return path

    @ieditor
    def change_file_prop(self, file_baton, name, value, pool=None):
        if name == 'svn:executable':
            self.current.execfiles[self.current.file] = bool(value is not None)
        elif name == 'svn:special':
            self.current.symlinks[self.current.file] = bool(value is not None)

    @ieditor
    def change_dir_prop(self, dir_baton, name, value, pool=None):
        if dir_baton is None:
            return
        path = self.current.batons[dir_baton]
        if name == 'svn:externals':
            self.current.externals[path] = value

    @ieditor
    def open_directory(self, path, parent_baton, base_revision, dir_pool=None):
        self.current.batons[path] = path
        p_, branch = self.meta.split_branch_path(path)[:2]
        if p_ == '':
            self.current.emptybranches[branch] = False
        return path

    @ieditor
    def close_directory(self, dir_baton, dir_pool=None):
        if dir_baton is not None:
            del self.current.batons[dir_baton]

    @ieditor
    def apply_textdelta(self, file_baton, base_checksum, pool=None):
        # We know coming in here the file must be one of the following options:
        # 1) Deleted (invalid, fail an assertion)
        # 2) Missing a base text (bail quick since we have to fetch a full plaintext)
        # 3) Has a base text in self.current.files, apply deltas
        base = ''
        if not self.meta.is_path_valid(self.current.file):
            return lambda x: None
        assert self.current.file not in self.current.deleted, (
            'Cannot apply_textdelta to a deleted file: %s' % self.current.file)
        assert (self.current.file in self.current.files
                or self.current.file in self.current.missing), '%s not found' % self.current.file
        if self.current.file in self.current.missing:
            return lambda x: None
        base = self.current.files[self.current.file]
        source = cStringIO.StringIO(base)
        target = cStringIO.StringIO()
        self.stream = target

        handler, baton = delta.svn_txdelta_apply(source, target, None)
        if not callable(handler): #pragma: no cover
            raise hgutil.Abort('Error in Subversion bindings: '
                               'cannot call handler!')
        def txdelt_window(window):
            try:
                if not self.meta.is_path_valid(self.current.file):
                    return
                handler(window, baton)
                # window being None means commit this file
                if not window:
                    self.current.files[self.current.file] = target.getvalue()
            except core.SubversionException, e: #pragma: no cover
                if e.apr_err == core.SVN_ERR_INCOMPLETE_DATA:
                    self.current.missing.add(self.current.file)
                else: #pragma: no cover
                    raise hgutil.Abort(*e.args)
            except: #pragma: no cover
                print len(base), self.current.file
                self._exception_info = sys.exc_info()
                raise
        return txdelt_window
