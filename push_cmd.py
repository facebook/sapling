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
def push_revisions_to_subversion(ui, repo, hg_repo_path, svn_url, **opts):
    """Push revisions starting at a specified head back to Subversion.
    """
    #assert False # safety while the command is partially implemented.
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
        r = fetch_command.fetch_revisions(ui, svn_url, hg_repo_path)
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
    return 0


def commit_from_rev(ui, repo, rev_ctx, hg_editor, svn_url, base_revision):
    """Build and send a commit from Mercurial to Subversion.
    """
    target_files = []
    file_data = {}
    for file in rev_ctx.files():
        parent = rev_ctx.parents()[0]
        new_data = base_data = ''
        action = ''
        if file in rev_ctx:
            new_data = rev_ctx.filectx(file).data()
            if file not in parent:
                target_files.append(file)
                action = 'add'
                # TODO check for mime-type autoprops here
                # TODO check for directory adds here
            else:
                target_files.append(file)
                base_data = parent.filectx(file).data()
                action = 'modify'
        else:
            target_files.append(file)
            base_data = parent.filectx(file).data()
            action = 'delete'
        file_data[file] = base_data, new_data, action

    # TODO check for directory deletes here
    svn = svnwrap.SubversionRepo(svn_url)
    parent_branch = rev_ctx.parents()[0].branch()
    branch_path = 'trunk'
    if parent_branch and parent_branch != 'default':
        branch_path = 'branches/%s' % parent_branch
    new_target_files = ['%s/%s' % (branch_path, f) for f in target_files]
    for tf, ntf in zip(target_files, new_target_files):
        if tf in file_data:
            file_data[ntf] = file_data[tf]
            del file_data[tf]
    try:
        svn.commit(new_target_files, rev_ctx.description(), file_data,
                   base_revision, set([]))
    except core.SubversionException, e:
        if hasattr(e, 'apr_err') and e.apr_err == 160028:
            raise merc_util.Abort('Base text was out of date, maybe rebase?')
        else:
            raise
