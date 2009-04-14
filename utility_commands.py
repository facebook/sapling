import os

from mercurial import node
from mercurial import util as hgutil
from hgext import rebase as hgrebase

import svnwrap
import cmdutil
import util
import hg_delta_editor

def url(ui, repo, hg_repo_path, **opts):
    """show the location (URL) of the Subversion repository
    """
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    ui.status(hge.url, '\n')


def genignore(ui, repo, hg_repo_path, force=False, **opts):
    """generate .hgignore from svn:ignore properties.
    """
    ignpath = os.path.join(hg_repo_path, '.hgignore')
    if not force and os.path.exists(ignpath):
        raise hgutil.Abort('not overwriting existing .hgignore, try --force?')
    ignorefile = open(ignpath, 'w')
    ignorefile.write('.hgignore\nsyntax:glob\n')
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    parent = cmdutil.parentrev(ui, repo, hge, svn_commit_hashes)
    r, br = svn_commit_hashes[parent.node()]
    if br == None:
        branchpath = 'trunk'
    else:
        branchpath = 'branches/%s' % br
    url = hge.url
    if url[-1] == '/':
        url = url[:-1]
    user = opts.get('username', hgutil.getuser())
    passwd = opts.get('passwd', '')
    svn = svnwrap.SubversionRepo(url, user, passwd)
    dirs = [''] + [d[0] for d in svn.list_files(branchpath, r) if d[1] == 'd']
    for dir in dirs:
        props = svn.list_props('%s/%s/' % (branchpath,dir), r)
        if 'svn:ignore' in props:
            lines = props['svn:ignore'].strip().split('\n')
            for prop in lines:
                if dir:
                    ignorefile.write('%s/%s\n' % (dir, prop))
                else:
                    ignorefile.write('%s\n' % prop)


def info(ui, repo, hg_repo_path, **opts):
    """show Subversion details similar to `svn info'
    """
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    parent = cmdutil.parentrev(ui, repo, hge, svn_commit_hashes)
    pn = parent.node()
    if pn not in svn_commit_hashes:
        ui.status('Not a child of an svn revision.\n')
        return 0
    r, br = svn_commit_hashes[pn]
    subdir = parent.extra()['convert_revision'][40:].split('@')[0]
    if br == None:
        branchpath = '/trunk'
    elif br.startswith('../'):
        branchpath = '/%s' % br[3:]
        subdir = subdir.replace('branches/../', '')
    else:
        branchpath = '/branches/%s' % br
    url = hge.url
    if url[-1] == '/':
        url = url[:-1]
    url = '%s%s' % (url, branchpath)
    author = hge.svnauthorforauthor(parent.user())
    # cleverly figure out repo root w/o actually contacting the server
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
               'date': hgutil.datestr(parent.date(),
                                      '%Y-%m-%d %H:%M:%S %1%2 (%a, %d %b %Y)')
              })


def rebase(ui, repo, extrafn=None, sourcerev=None, **opts):
    """rebase current unpushed revisions onto the Subversion head

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
    hge = hg_delta_editor.HgChangeReceiver(repo=repo)
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
    return hgrebase.rebase(ui, repo, dest=node.hex(target_rev.node()),
                         base=node.hex(sourcerev),
                         extrafn=extrafn)


def listauthors(ui, args, authors=None, **opts):
    """list all authors in a Subversion repository
    """
    if not len(args):
        ui.status('No repository specified.\n')
        return
    svn = svnwrap.SubversionRepo(util.normalize_url(args[0]))
    author_set = set()
    for rev in svn.revisions():
        author_set.add(str(rev.author)) # So None becomes 'None'
    if authors:
        authorfile = open(authors, 'w')
        authorfile.write('%s=\n' % '=\n'.join(sorted(author_set)))
        authorfile.close()
    else:
        ui.status('%s\n' % '\n'.join(sorted(author_set)))


def version(ui, **opts):
    """Show current version of hg and hgsubversion.
    """
    ui.status('hg: %s\n' % hgutil.version())
    ui.status('svn bindings: %s\n' % svnwrap.version())
    ui.status('hgsubversion: %s\n' % util.version(ui))


nourl = ['version', 'listauthors']
table = {
    'url': url,
    'genignore': genignore,
    'info': info,
    'listauthors': listauthors,
    'version': version,
    'rebase': rebase,
}
