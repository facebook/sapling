from mercurial import cmdutil
from mercurial import node
from mercurial import util as mutil
from hgext import rebase

import util
import hg_delta_editor

def print_wc_url(ui, repo, hg_repo_path, **opts):
    """Url of Subversion repository.
    """
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    ui.status(hge.url, '\n')
print_wc_url = util.register_subcommand('url')(print_wc_url)


def run_svn_info(ui, repo, hg_repo_path, **opts):
    """Like svn info details.
    """
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    workingctx = repo.parents()[0]
    o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, workingctx.node())
    if o_r:
        workingctx = repo[o_r[-1]].parents()[0]
    r, br = svn_commit_hashes[workingctx.node()]
    if br == None:
        branchpath = '/trunk'
    else:
        branchpath = '/branches/%s' % br
    url = hge.url
    if url[-1] == '/':
        url = url[:-1]
    url = '%s%s' % (url, branchpath)
    author = '@'.join(workingctx.user().split('@')[:-1])
    # cleverly figure out repo root w/o actually contacting the server
    subdir = workingctx.extra()['convert_revision'][40:].split('@')[0]
    reporoot = url[:len(url)-len(subdir)]
    ui.status('''URL: %(url)s
Repository Root: %(reporoot)s
Repository UUID: %(uuid)s
Revision: %(revision)s
Node Kind: directory
Last Changed Author: %(author)s
Last Changed Rev: %(revision)s
Last Changed Date: %(date)s\n''' %
              {'reporoot': reporoot,
               'uuid': open(hge.uuid_file).read(),
               'url': url,
               'author': author,
               'revision': r,
               # TODO I'd like to format this to the user's local TZ if possible
               'date': mutil.datestr(workingctx.date(),
                                     '%Y-%m-%d %H:%M:%S %1%2 (%a, %d %b %Y)')
              })
run_svn_info = util.register_subcommand('info')(run_svn_info)


def print_parent_revision(ui, repo, hg_repo_path, **opts):
    """Display hg hash and svn revision of nearest svn parent.
    """
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    ha = repo.parents()[0]
    o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, ha.node())
    if o_r:
        ha = repo[o_r[-1]].parents()[0]
    if ha.node() != node.nullid:
        r, br = svn_commit_hashes[ha.node()]
        ui.status('Working copy parent revision is %s: r%s on %s\n' %
                  (ha, r, br or 'trunk'))
    else:
        ui.status('Working copy seems to have no parent svn revision.\n')
    return 0
print_parent_revision = util.register_subcommand('parent')(print_parent_revision)


def rebase_commits(ui, repo, hg_repo_path, extrafn=None, sourcerev=None, **opts):
    """Rebases current unpushed revisions onto Subversion head.

    This moves a line of development from making its own head to the top of
    Subversion development, linearizing the changes. In order to make sure you
    rebase on top of the current top of Subversion work, you should probably run
    'hg svn pull' before running this.
    """
    if extrafn is None:
        def extrafn2(ctx, extra):
            """defined here so we can add things easily.
            """
            extra['branch'] = ctx.branch()
        extrafn = extrafn2
    if sourcerev is None:
        sourcerev = repo.parents()[0].node()
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, sourcerev=sourcerev)
    if not o_r:
        ui.status('Nothing to rebase!\n')
        return 0
    if len(repo[sourcerev].children()):
        ui.status('Refusing to rebase non-head commit like a coward\n')
        return 0
    parent_rev = repo[o_r[-1]].parents()[0]
    target_rev = parent_rev
    p_n = parent_rev.node()
    exhausted_choices = False
    while target_rev.children() and not exhausted_choices:
        for c in target_rev.children():
            exhausted_choices = True
            n = c.node()
            if (n in svn_commit_hashes and
                svn_commit_hashes[n][1] == svn_commit_hashes[p_n][1]):
                target_rev = c
                exhausted_choices = False
                break
    if parent_rev == target_rev:
        ui.status('Already up to date!\n')
        return 0
    # TODO this is really hacky, there must be a more direct way
    return rebase.rebase(ui, repo, dest=node.hex(target_rev.node()),
                         base=node.hex(sourcerev),
                         extrafn=extrafn)
rebase_commits = util.register_subcommand('rebase')(rebase_commits)


def show_outgoing_to_svn(ui, repo, hg_repo_path, **opts):
    """Show changesets not yet pushed to SVN.
    """
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, repo.parents()[0].node())
    if not (o_r and len(o_r)):
        ui.status('No outgoing changes found.\n')
        return 0
    displayer = cmdutil.show_changeset(ui, repo, opts, buffered=False)
    for node in reversed(o_r):
        displayer.show(repo[node])
show_outgoing_to_svn = util.register_subcommand('outgoing')(show_outgoing_to_svn)
