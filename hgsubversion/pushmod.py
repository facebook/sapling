from mercurial import util as hgutil

import svnwrap
import svnexternals
import util

class NoFilesException(Exception):
    """Exception raised when you try and commit without files.
    """


def _isdir(svn, branchpath, svndir):
    try:
        path = ''
        if branchpath:
            path = branchpath + '/'
        svn.list_dir('%s%s' % (path, svndir))
        return True
    except svnwrap.SubversionException:
        return False


def _getdirchanges(svn, branchpath, parentctx, ctx, changedfiles, extchanges):
    """Compute directories to add or delete when moving from parentctx
    to ctx, assuming only 'changedfiles' files changed, and 'extchanges'
    external references changed (as returned by svnexternals.diff()).

    Return (added, deleted) where 'added' is the list of all added
    directories and 'deleted' the list of deleted directories.
    Intermediate directories are included: if a/b/c is new and requires
    the addition of a/b and a, those will be listed too. Intermediate
    deleted directories are also listed, but item order of undefined
    in either list.
    """
    def finddirs(path, includeself=False):
        if includeself and path:
            yield path
        pos = path.rfind('/')
        while pos != -1:
            yield path[:pos]
            pos = path.rfind('/', 0, pos)
        # Include the root path, properties can be set explicitely on it
        # (like externals), and you want to preserve it if there are any
        # other child item still existing.
        yield ''

    def getctxdirs(ctx, keptdirs, extdirs):
        dirs = {}
        for f in ctx.manifest():
            for d in finddirs(f):
                if d in dirs:
                    break
                if d in keptdirs:
                    dirs[d] = 1
        for extdir in extdirs:
            for d in finddirs(extdir, True):
                dirs[d] = 1
        return dirs

    deleted, added = [], []
    changeddirs = {}
    for f in changedfiles:
        if f in parentctx and f in ctx:
            # Updated files cannot cause directories to be created
            # or removed.
            continue
        for d in finddirs(f):
            changeddirs[d] = 1
    for e in extchanges:
        if not e[1] or not e[2]:
            for d in finddirs(e[0], True):
                changeddirs[d] = 1
    if not changeddirs:
        return added, deleted
    olddirs = getctxdirs(parentctx, changeddirs,
                         [e[0] for e in extchanges if e[1]])
    newdirs = getctxdirs(ctx, changeddirs,
                         [e[0] for e in extchanges if e[2]])

    for d in newdirs:
        if d not in olddirs and not _isdir(svn, branchpath, d):
            added.append(d)

    for d in olddirs:
        if not d:
            # Do not remove the root directory when the hg repo becomes
            # empty. hgsubversion cannot create branches, do not remove
            # them.
            continue
        if d not in newdirs and _isdir(svn, branchpath, d):
            deleted.append(d)

    return added, deleted


def commit(ui, repo, rev_ctx, meta, base_revision, svn):
    """Build and send a commit from Mercurial to Subversion.
    """
    file_data = {}
    parent = rev_ctx.parents()[0]
    parent_branch = rev_ctx.parents()[0].branch()
    branch_path = meta.layoutobj.remotename(parent_branch)

    extchanges = svnexternals.diff(svnexternals.parse(ui, parent),
                                   svnexternals.parse(ui, rev_ctx))
    addeddirs, deleteddirs = _getdirchanges(svn, branch_path, parent, rev_ctx,
                                            rev_ctx.files(), extchanges)
    deleteddirs = set(deleteddirs)

    props = {}
    copies = {}
    for file in rev_ctx.files():
        if file in util.ignoredfiles:
            continue
        new_data = base_data = ''
        action = ''
        if file in rev_ctx:
            fctx = rev_ctx.filectx(file)
            new_data = fctx.data()

            if 'x' in fctx.flags():
                props.setdefault(file, {})['svn:executable'] = '*'
            if 'l' in fctx.flags():
                props.setdefault(file, {})['svn:special'] = '*'
            isbinary = hgutil.binary(new_data)
            if isbinary:
                props.setdefault(file, {})['svn:mime-type'] = 'application/octet-stream'

            if file not in parent:
                renamed = fctx.renamed()
                if renamed:
                    # TODO current model (and perhaps svn model) does not support
                    # this kind of renames: a -> b, b -> c
                    copies[file] = renamed[0]
                    base_data = parent[renamed[0]].data()
                else:
                    autoprops = svn.autoprops_config.properties(file)
                    if autoprops:
                        props.setdefault(file, {}).update(autoprops)

                action = 'add'
                dirname = '/'.join(file.split('/')[:-1] + [''])
            else:
                base_data = parent.filectx(file).data()
                if ('x' in parent.filectx(file).flags()
                    and 'x' not in rev_ctx.filectx(file).flags()):
                    props.setdefault(file, {})['svn:executable'] = None
                if ('l' in parent.filectx(file).flags()
                    and 'l' not in rev_ctx.filectx(file).flags()):
                    props.setdefault(file, {})['svn:special'] = None
                if hgutil.binary(base_data) and not isbinary:
                    props.setdefault(file, {})['svn:mime-type'] = None
                action = 'modify'
        else:
            pos = file.rfind('/')
            if pos >= 0:
                if file[:pos] in deleteddirs:
                    # This file will be removed when its directory is removed
                    continue
            action = 'delete'
        file_data[file] = base_data, new_data, action

    def svnpath(p):
        return ('%s/%s' % (branch_path, p)).strip('/')

    changeddirs = []
    for d, v1, v2 in extchanges:
        props.setdefault(svnpath(d), {})['svn:externals'] = v2
        if d not in deleteddirs and d not in addeddirs:
            changeddirs.append(svnpath(d))

    # Now we are done with files, we can prune deleted directories
    # against themselves: ignore a/b if a/ is already removed
    deleteddirs2 = list(deleteddirs)
    deleteddirs2.sort(reverse=True)
    for d in deleteddirs2:
        pos = d.rfind('/')
        if pos >= 0 and d[:pos] in deleteddirs:
            deleteddirs.remove(d)

    newcopies = {}
    for source, dest in copies.iteritems():
        newcopies[svnpath(source)] = (svnpath(dest), base_revision)

    new_target_files = [svnpath(f) for f in file_data]
    for tf, ntf in zip(file_data, new_target_files):
        if tf in file_data and tf != ntf:
            file_data[ntf] = file_data[tf]
            if tf in props:
                props[ntf] = props.pop(tf)
            del file_data[tf]

    addeddirs = [svnpath(d) for d in addeddirs]
    deleteddirs = [svnpath(d) for d in deleteddirs]
    new_target_files += addeddirs + deleteddirs + changeddirs
    if not new_target_files:
        raise NoFilesException()
    try:
        return svn.commit(new_target_files, rev_ctx.description(), file_data,
                          base_revision, set(addeddirs), set(deleteddirs),
                          props, newcopies)
    except svnwrap.SubversionException, e:
        ui.traceback()

        if len(e.args) > 0 and e.args[1] in (svnwrap.ERR_FS_TXN_OUT_OF_DATE,
                                             svnwrap.ERR_FS_CONFLICT,
                                             svnwrap.ERR_FS_ALREADY_EXISTS):
            raise hgutil.Abort('Outgoing changesets parent is not at '
                               'subversion HEAD\n'
                               '(pull again and rebase on a newer revision)')
        elif len(e.args) > 0 and e.args[1] == svnwrap.ERR_REPOS_HOOK_FAILURE:
            # Special handling for svn hooks blocking error
            raise hgutil.Abort(e.args[0])
        else:
            raise
