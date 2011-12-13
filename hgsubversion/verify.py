import posixpath

from mercurial import util as hgutil
from mercurial import error

import svnwrap
import svnrepo
import util
import editor

def verify(ui, repo, args=None, **opts):
    '''verify current revision against Subversion repository
    '''

    if repo is None:
        raise error.RepoError("There is no Mercurial repository"
                              " here (.hg not found)")

    ctx = repo[opts.get('rev', '.')]
    if 'close' in ctx.extra():
        ui.write('cannot verify closed branch')
        return 0
    convert_revision = ctx.extra().get('convert_revision')
    if convert_revision is None or not convert_revision.startswith('svn:'):
        raise hgutil.Abort('revision %s not from SVN' % ctx)

    if args:
        url = repo.ui.expandpath(args[0])
    else:
        url = repo.ui.expandpath('default')

    svn = svnrepo.svnremoterepo(ui, url).svn
    meta = repo.svnmeta(svn.uuid, svn.subdir)
    srev, branch, branchpath = meta.get_source_rev(ctx=ctx)

    branchpath = branchpath[len(svn.subdir.lstrip('/')):]
    branchurl = ('%s/%s' % (url, branchpath)).strip('/')

    ui.write('verifying %s against %s@%i\n' % (ctx, branchurl, srev))

    if opts.get('stupid', ui.configbool('hgsubversion', 'stupid')):
        svnfiles = set()
        result = 0

        hgfiles = set(ctx) - util.ignoredfiles

        svndata = svn.list_files(branchpath, srev)
        for i, (fn, type) in enumerate(svndata):
            util.progress(ui, 'verify', i, total=len(hgfiles))

            if type != 'f':
                continue
            svnfiles.add(fn)
            fp = fn
            if branchpath:
                fp = branchpath + '/' + fn
            data, mode = svn.get_file(posixpath.normpath(fp), srev)
            try:
                fctx = ctx[fn]
            except error.LookupError:
                result = 1
                continue
            if not fctx.data() == data:
                ui.write('difference in: %s\n' % fn)
                result = 1
            if not fctx.flags() == mode:
                ui.write('wrong flags for: %s\n' % fn)
                result = 1

        if hgfiles != svnfiles:
            unexpected = hgfiles - svnfiles
            for f in sorted(unexpected):
                ui.write('unexpected file: %s\n' % f)
            missing = svnfiles - hgfiles
            for f in sorted(missing):
                ui.write('missing file: %s\n' % f)
            result = 1

        util.progress(ui, 'verify', None, total=len(hgfiles))

    else:
        class VerifyEditor(svnwrap.Editor):
            """editor that verifies a repository against the given context."""
            def __init__(self, ui, ctx):
                self.ui = ui
                self.ctx = ctx
                self.unexpected = set(ctx) - util.ignoredfiles
                self.missing = set()
                self.failed = False

                self.total = len(self.unexpected)
                self.seen = 0

            def open_root(self, base_revnum, pool=None):
                pass

            def add_directory(self, path, parent_baton, copyfrom_path,
                              copyfrom_revision, pool=None):
                self.file = None
                self.props = None

            def open_directory(self, path, parent_baton, base_revision, pool=None):
                self.file = None
                self.props = None

            def add_file(self, path, parent_baton=None, copyfrom_path=None,
                         copyfrom_revision=None, file_pool=None):

                if path in self.unexpected:
                    self.unexpected.remove(path)
                    self.file = path
                    self.props = {}
                else:
                    self.total += 1
                    self.missing.add(path)
                    self.failed = True
                    self.file = None
                    self.props = None

                self.seen += 1
                util.progress(self.ui, 'verify', self.seen, total=self.total)

            def open_file(self, path, base_revnum):
                raise NotImplementedError()

            def apply_textdelta(self, file_baton, base_checksum, pool=None):
                stream = editor.NeverClosingStringIO()
                handler = svnwrap.apply_txdelta('', stream)
                if not callable(handler):
                    raise hgutil.Abort('Error in Subversion bindings: '
                                       'cannot call handler!')
                def txdelt_window(window):
                    handler(window)
                    # window being None means we're done
                    if window:
                        return

                    fctx = self.ctx[self.file]
                    hgdata = fctx.data()
                    svndata = stream.getvalue()

                    if 'svn:executable' in self.props:
                        if fctx.flags() != 'x':
                            self.ui.warn('wrong flags for: %s\n' % self.file)
                            self.failed = True
                    elif 'svn:special' in self.props:
                        hgdata = 'link ' + hgdata
                        if fctx.flags() != 'l':
                            self.ui.warn('wrong flags for: %s\n' % self.file)
                            self.failed = True
                    elif fctx.flags():
                        self.ui.warn('wrong flags for: %s\n' % self.file)
                        self.failed = True

                    if hgdata != svndata:
                        self.ui.warn('difference in: %s\n' % self.file)
                        self.failed = True

                if self.file is not None:
                    return txdelt_window

            def change_dir_prop(self, dir_baton, name, value, pool=None):
                pass

            def change_file_prop(self, file_baton, name, value, pool=None):
                if self.props is not None:
                    self.props[name] = value

            def close_directory(self, dir_baton, pool=None):
                pass

            def delete_entry(self, path, revnum, pool=None):
                raise NotImplementedError()

            def check(self):
                util.progress(self.ui, 'verify', None, total=self.total)

                for f in self.unexpected:
                    self.ui.warn('unexpected file: %s\n' % f)
                    self.failed = True
                for f in self.missing:
                    self.ui.warn('missing file: %s\n' % f)
                    self.failed = True
                return not self.failed

        v = VerifyEditor(ui, ctx)
        svnrepo.svnremoterepo(ui, branchurl).svn.get_revision(srev, v)
        if v.check():
            result = 0
        else:
            result = 1

    return result
