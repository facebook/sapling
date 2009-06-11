import os

from mercurial import util as hgutil

import svnmeta
import svnwrap
import cmdutil
import util

def genignore(ui, repo, hg_repo_path, force=False, **opts):
    """generate .hgignore from svn:ignore properties.
    """
    ignpath = os.path.join(hg_repo_path, '.hgignore')
    if not force and os.path.exists(ignpath):
        raise hgutil.Abort('not overwriting existing .hgignore, try --force?')
    ignorefile = open(ignpath, 'w')
    ignorefile.write('.hgignore\nsyntax:glob\n')
    url = util.normalize_url(repo.ui.config('paths', 'default'))
    user, passwd = util.getuserpass(opts)
    svn = svnwrap.SubversionRepo(url, user, passwd)
    meta = svnmeta.SVNMeta(repo, svn.uuid)
    hashes = meta.revmap.hashes()
    parent = cmdutil.parentrev(ui, repo, meta, hashes)
    r, br = hashes[parent.node()]
    if br == None:
        branchpath = 'trunk'
    else:
        branchpath = 'branches/%s' % br
    if url[-1] == '/':
        url = url[:-1]
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
    url = util.normalize_url(repo.ui.config('paths', 'default'))
    user, passwd = util.getuserpass(opts)
    svn = svnwrap.SubversionRepo(url, user, passwd)
    meta = svnmeta.SVNMeta(repo, svn.uuid)
    hashes = meta.revmap.hashes()
    parent = cmdutil.parentrev(ui, repo, meta, hashes)
    pn = parent.node()
    if pn not in hashes:
        ui.status('Not a child of an svn revision.\n')
        return 0
    r, br = hashes[pn]
    subdir = parent.extra()['convert_revision'][40:].split('@')[0]
    if br == None:
        branchpath = '/trunk'
    elif br.startswith('../'):
        branchpath = '/%s' % br[3:]
        subdir = subdir.replace('branches/../', '')
    else:
        branchpath = '/branches/%s' % br
    url = util.normalize_url(repo.ui.config('paths', 'default'))
    if url[-1] == '/':
        url = url[:-1]
    url = '%s%s' % (url, branchpath)
    author = meta.authors.reverselookup(parent.user())
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
               'uuid': meta.uuid,
               'url': url,
               'author': author,
               'revision': r,
               # TODO I'd like to format this to the user's local TZ if possible
               'date': hgutil.datestr(parent.date(),
                                      '%Y-%m-%d %H:%M:%S %1%2 (%a, %d %b %Y)')
              })


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

table = {
    'genignore': genignore,
    'info': info,
    'listauthors': listauthors,
    'version': version,
}
