from mercurial import util as merc_util
from mercurial import hg
from mercurial import node
from svn import core

import util
import hg_delta_editor
import svnexternals
import svnwrap
import fetch_command
import utility_commands


def push_revisions_to_subversion(ui, repo, hg_repo_path, svn_url,
                                 stupid=False, **opts):
    """Push revisions starting at a specified head back to Subversion.
    """
    oldencoding = merc_util._encoding
    merc_util._encoding = 'UTF-8'
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    # Strategy:
    # 1. Find all outgoing commits from this head
    if len(repo.parents()) != 1:
        ui.status('Cowardly refusing to push branch merge')
        return 1
    workingrev = repo.parents()[0]
    outgoing = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, workingrev.node())
    if not (outgoing and len(outgoing)):
        ui.status('No revisions to push.')
        return 0
    while outgoing:
        oldest = outgoing.pop(-1)
        old_ctx = repo[oldest]
        if len(old_ctx.parents()) != 1:
            ui.status('Found a branch merge, this needs discussion and '
                      'implementation.')
            return 1
        base_n = old_ctx.parents()[0].node()
        old_children = repo[base_n].children()
        svnbranch = repo[base_n].branch()
        oldtip = base_n
        samebranchchildren = [c for c in repo[oldtip].children() if c.branch() == svnbranch
                              and c.node() in svn_commit_hashes]
        while samebranchchildren:
            oldtip = samebranchchildren[0].node()
            samebranchchildren = [c for c in repo[oldtip].children() if c.branch() == svnbranch
                                  and c.node() in svn_commit_hashes]
        # 2. Commit oldest revision that needs to be pushed
        base_revision = svn_commit_hashes[base_n][0]
        commit_from_rev(ui, repo, old_ctx, hge, svn_url, base_revision)
        # 3. Fetch revisions from svn
        r = fetch_command.fetch_revisions(ui, svn_url, hg_repo_path,
                                          stupid=stupid)
        assert not r or r == 0
        # 4. Find the new head of the target branch
        repo = hg.repository(ui, hge.path)
        oldtipctx = repo[oldtip]
        replacement = [c for c in oldtipctx.children() if c not in old_children
                       and c.branch() == oldtipctx.branch()]
        assert len(replacement) == 1, 'Replacement node came back as: %r' % replacement
        replacement = replacement[0]
        # 5. Rebase all children of the currently-pushing rev to the new branch
        heads = repo.heads(old_ctx.node())
        for needs_transplant in heads:
            def extrafn(ctx, extra):
                if ctx.node() == oldest:
                    return
                extra['branch'] = ctx.branch()
            utility_commands.rebase_commits(ui, repo, hg_repo_path,
                                            extrafn=extrafn,
                                            sourcerev=needs_transplant,
                                            **opts)
            repo = hg.repository(ui, hge.path)
            for child in repo[replacement.node()].children():
                rebasesrc = node.bin(child.extra().get('rebase_source', node.hex(node.nullid)))
                if rebasesrc in outgoing:
                    while rebasesrc in outgoing:
                        rebsrcindex = outgoing.index(rebasesrc)
                        outgoing = (outgoing[0:rebsrcindex] +
                                    [child.node(), ] + outgoing[rebsrcindex+1:])
                        children = [c for c in child.children() if c.branch() == child.branch()]
                        if children:
                            child = children[0]
                        rebasesrc = node.bin(child.extra().get('rebase_source', node.hex(node.nullid)))
        hge = hg_delta_editor.HgChangeReceiver(hg_repo_path, ui_=ui)
        svn_commit_hashes = dict(zip(hge.revmap.itervalues(), hge.revmap.iterkeys()))
    merc_util._encoding = oldencoding
    return 0
push_revisions_to_subversion = util.register_subcommand('push')(push_revisions_to_subversion)
# for git expats
push_revisions_to_subversion = util.register_subcommand('dcommit')(push_revisions_to_subversion)

def _isdir(svn, branchpath, svndir):
    try:
        svn.list_dir('%s/%s' % (branchpath, svndir))
        return True
    except core.SubversionException:
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
        if includeself:
            yield path
        pos = path.rfind('/')
        while pos != -1:
            yield path[:pos]
            pos = path.rfind('/', 0, pos)

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
        if d not in newdirs and _isdir(svn, branchpath, d):
            deleted.append(d)

    return added, deleted

def _externals(ctx):
    ext = svnexternals.externalsfile()
    if '.hgsvnexternals' in ctx:
        ext.read(ctx['.hgsvnexternals'].data())
    return ext

def commit_from_rev(ui, repo, rev_ctx, hg_editor, svn_url, base_revision):
    """Build and send a commit from Mercurial to Subversion.
    """
    file_data = {}
    svn = svnwrap.SubversionRepo(svn_url, username=merc_util.getuser())
    parent = rev_ctx.parents()[0]
    parent_branch = rev_ctx.parents()[0].branch()
    branch_path = 'trunk'

    if parent_branch and parent_branch != 'default':
        branch_path = 'branches/%s' % parent_branch

    extchanges = list(svnexternals.diff(_externals(parent), 
                                        _externals(rev_ctx)))
    addeddirs, deleteddirs = _getdirchanges(svn, branch_path, parent, rev_ctx,
                                            rev_ctx.files(), extchanges)
    deleteddirs = set(deleteddirs)

    props = {}
    copies = {}
    for file in rev_ctx.files():
        if file == '.hgsvnexternals':
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

            if file not in parent:
                renamed = fctx.renamed()
                if renamed:
                    # TODO current model (and perhaps svn model) does not support
                    # this kind of renames: a -> b, b -> c
                    copies[file] = renamed[0]
                    base_data = parent[renamed[0]].data()

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
        return '%s/%s' % (branch_path, p)

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
            deleteddirs.remove(d[:pos])

    newcopies = {}
    for source, dest in copies.iteritems():
        newcopies[svnpath(source)] = (svnpath(dest), base_revision)

    new_target_files = [svnpath(f) for f in file_data]
    for tf, ntf in zip(file_data, new_target_files):
        if tf in file_data:
            file_data[ntf] = file_data[tf]
            if tf in props:
                props[ntf] = props[tf]
                del props[tf]
            if merc_util.binary(file_data[ntf][1]):
                props.setdefault(ntf, {}).update(props.get(ntf, {}))
                props.setdefault(ntf, {})['svn:mime-type'] = 'application/octet-stream'
            del file_data[tf]

    addeddirs = [svnpath(d) for d in addeddirs]
    deleteddirs = [svnpath(d) for d in deleteddirs]
    new_target_files += addeddirs + deleteddirs + changeddirs
    try:
        svn.commit(new_target_files, rev_ctx.description(), file_data,
                   base_revision, set(addeddirs), set(deleteddirs),
                   props, newcopies)
    except core.SubversionException, e:
        if hasattr(e, 'apr_err') and e.apr_err == 160028:
            raise merc_util.Abort('Base text was out of date, maybe rebase?')
        else:
            raise
