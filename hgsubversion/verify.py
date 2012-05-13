import posixpath

from mercurial import error

import svnrepo
import util

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

    svnfiles = set()
    result = 0

    svndata = svn.list_files(branchpath, srev)
    for i, (fn, type) in enumerate(svndata):
        util.progress(ui, 'verify', i)
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

    hgfiles = set(ctx) - util.ignoredfiles
    if hgfiles != svnfiles:
        unexpected = hgfiles - svnfiles
        for f in sorted(unexpected):
            ui.write('unexpected file: %s\n' % f)
        missing = svnfiles - hgfiles
        for f in sorted(missing):
            ui.write('missing file: %s\n' % f)
        result = 1

    util.progress(ui, 'verify', None)

    return result
