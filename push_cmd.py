from mercurial import util as merc_util
from mercurial import hg
from svn import core

import util
import hg_delta_editor
import svnwrap
import fetch_command
import utility_commands


@util.register_subcommand('push')
@util.register_subcommand('dcommit') # for git expats
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
    outgoing = utility_commands.outgoing_revisions(ui, repo, hge,
                                                   svn_commit_hashes)
    if not (outgoing and len(outgoing)):
        ui.status('No revisions to push.')
        return 0
    if len(repo.parents()) != 1:
        ui.status('Cowardly refusing to push branch merge')
        return 1
    while outgoing:
        oldest = outgoing.pop(-1)
        old_ctx = repo[oldest]
        if len(old_ctx.parents()) != 1:
            ui.status('Found a branch merge, this needs discussion and '
                      'implementation.')
            return 1
        base_n = old_ctx.parents()[0].node()
        old_children = repo[base_n].children()
        # 2. Commit oldest revision that needs to be pushed
        base_revision = svn_commit_hashes[old_ctx.parents()[0].node()][0]
        commit_from_rev(ui, repo, old_ctx, hge, svn_url, base_revision)
        # 3. Fetch revisions from svn
        r = fetch_command.fetch_revisions(ui, svn_url, hg_repo_path,
                                          stupid=stupid)
        assert not r or r == 0
        # 4. Find the new head of the target branch
        repo = hg.repository(ui, hge.path)
        base_c = repo[base_n]
        replacement = [c for c in base_c.children() if c not in old_children
                       and c.branch() == old_ctx.branch()]
        assert len(replacement) == 1
        replacement = replacement[0]
        # 5. Rebase all children of the currently-pushing rev to the new branch
        heads = repo.heads(old_ctx.node())
        for needs_transplant in heads:
            hg.clean(repo, needs_transplant)
            utility_commands.rebase_commits(ui, repo, hg_repo_path, **opts)
            repo = hg.repository(ui, hge.path)
            if needs_transplant in outgoing:
                hg.clean(repo, repo['tip'].node())
                hge = hg_delta_editor.HgChangeReceiver(hg_repo_path, ui_=ui)
                svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                             hge.revmap.iterkeys()))
                outgoing = utility_commands.outgoing_revisions(ui, repo, hge,
                                                              svn_commit_hashes)
    merc_util._encoding = oldencoding
    return 0

def _getdirchanges(svn, branchpath, parentctx, ctx, changedfiles):
    """Compute directories to add or delete when moving from parentctx
    to ctx, assuming only 'changedfiles' files changed.

    Return (added, deleted) where 'added' is the list of all added
    directories and 'deleted' the list of deleted directories.
    Intermediate directories are included: if a/b/c is new and requires
    the addition of a/b and a, those will be listed too. Intermediate
    deleted directories are also listed, but item order of undefined
    in either list.
    """
    def exists(svndir):
        try:
            svn.list_dir('%s/%s' % (branchpath, svndir))
            return True
        except core.SubversionException:
            return False

    def finddirs(path):
        pos = path.rfind('/')
        while pos != -1:
            yield path[:pos]
            pos = path.rfind('/', 0, pos)

    def getctxdirs(ctx, keptdirs):
        dirs = {}
        for f in ctx.manifest():
            for d in finddirs(f):
                if d in dirs:
                    break
                if d in keptdirs:
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
    if not changeddirs:
        return added, deleted
    olddirs = getctxdirs(parentctx, changeddirs)
    newdirs = getctxdirs(ctx, changeddirs)

    for d in newdirs:
        if d not in olddirs and not exists(d):
            added.append(d)

    for d in olddirs:
        if d not in newdirs and exists(d):
            deleted.append(d)

    return added, deleted
        

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

    addeddirs, deleteddirs = _getdirchanges(svn, branch_path, parent, 
                                            rev_ctx, rev_ctx.files())
    deleteddirs = set(deleteddirs)

    props = {}
    copies = {}
    for file in rev_ctx.files():
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
            base_data = parent.filectx(file).data()
            action = 'delete'
        file_data[file] = base_data, new_data, action

    # Now we are done with files, we can prune deleted directories
    # against themselves: ignore a/b if a/ is already removed
    deleteddirs2 = list(deleteddirs)
    deleteddirs2.sort()
    deleteddirs2.reverse()
    for d in deleteddirs2:
        pos = d.rfind('/')
        if pos >= 0 and d[:pos] in deleteddirs:
            deleteddirs.remove(d[:pos])

    def svnpath(p):
        return '%s/%s' % (branch_path, p)

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
    new_target_files += addeddirs + deleteddirs
    try:
        svn.commit(new_target_files, rev_ctx.description(), file_data,
                   base_revision, set(addeddirs), set(deleteddirs), 
                   props, newcopies)
    except core.SubversionException, e:
        if hasattr(e, 'apr_err') and e.apr_err == 160028:
            raise merc_util.Abort('Base text was out of date, maybe rebase?')
        else:
            raise
