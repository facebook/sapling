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
    merc_util._encoding = oldencoding
    return 0


def commit_from_rev(ui, repo, rev_ctx, hg_editor, svn_url, base_revision):
    """Build and send a commit from Mercurial to Subversion.
    """
    target_files = []
    file_data = {}
    svn = svnwrap.SubversionRepo(svn_url, username=merc_util.getuser())
    parent = rev_ctx.parents()[0]
    parent_branch = rev_ctx.parents()[0].branch()
    branch_path = 'trunk'

    if parent_branch and parent_branch != 'default':
        branch_path = 'branches/%s' % parent_branch

    added_dirs = []
    props = {}
    for file in rev_ctx.files():
        new_data = base_data = ''
        action = ''
        if file in rev_ctx:
            new_data = rev_ctx.filectx(file).data()

            if 'x' in rev_ctx.filectx(file).flags():
                props.setdefault(file, {})['svn:executable'] = '*'
            if 'l' in rev_ctx.filectx(file).flags():
                props.setdefault(file, {})['svn:special'] = '*'

            if file not in parent:
                target_files.append(file)
                action = 'add'
                dirname = '/'.join(file.split('/')[:-1] + [''])
                # check for new directories
                if not list(parent.walk(util.PrefixMatch(dirname))):
                    # check and see if the dir exists svn-side.
                    try:
                        assert svn.list_dir('%s/%s' % (branch_path, dirname))
                    except core.SubversionException, e:
                        # dir must not exist
                        added_dirs.append(dirname[:-1])
            else:
                target_files.append(file)
                base_data = parent.filectx(file).data()
                if 'x' in parent.filectx(file).flags():
                    if 'svn:executable' in props.setdefault(file, {}):
                        del props[file]['svn:executable']
                    else:
                        props.setdefault(file, {})['svn:executable'] = None
                if 'l' in parent.filectx(file).flags():
                    if props.setdefault(file, {})['svn:special']:
                        del props[file]['svn:special']
                    else:
                        props.setdefault(file, {})['svn:special'] = None
                action = 'modify'
        else:
            target_files.append(file)
            base_data = parent.filectx(file).data()
            action = 'delete'
        file_data[file] = base_data, new_data, action

    # TODO check for directory deletes here
    new_target_files = ['%s/%s' % (branch_path, f) for f in target_files]
    for tf, ntf in zip(target_files, new_target_files):
        if tf in file_data:
            file_data[ntf] = file_data[tf]
            if tf in props:
                props[ntf] = props[tf]
                del props[tf]
            if merc_util.binary(file_data[ntf][1]):
                props.setdefault(ntf, {}).update(props.get(ntf, {}))
                props.setdefault(ntf, {})['svn:mime-type'] = 'application/octet-stream'
            del file_data[tf]
    added_dirs = ['%s/%s' % (branch_path, f) for f in added_dirs]
    new_target_files += added_dirs
    try:
        svn.commit(new_target_files, rev_ctx.description(), file_data,
                   base_revision, set(added_dirs), props)
    except core.SubversionException, e:
        if hasattr(e, 'apr_err') and e.apr_err == 160028:
            raise merc_util.Abort('Base text was out of date, maybe rebase?')
        else:
            raise
