# git.py - git server bridge
#
# Copyright 2008 Scott Chacon <schacon at gmail dot com>
#   also some code (and help) borrowed from durin42
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

'''push and pull from a Git server

This extension lets you communicate (push and pull) with a Git server.
This way you can use Git hosting for your project or collaborate with a
project that is in Git.  A bridger of worlds, this plugin be.

Try hg clone git:// or hg clone git+ssh://
'''

import os

from mercurial import commands, extensions, hg, util
from mercurial.i18n import _

import gitrepo, hgrepo
from git_handler import GitHandler

# support for `hg clone git://github.com/defunkt/facebox.git`
# also hg clone git+ssh://git@github.com/schacon/simplegit.git
hg.schemes['git'] = gitrepo
hg.schemes['git+ssh'] = gitrepo

# support for `hg clone localgitrepo`
_oldlocal = hg.schemes['file']

def _local(path):
    p = util.drop_scheme('file', path)
    if (os.path.exists(os.path.join(p, '.git')) and
        not os.path.exists(os.path.join(p, '.hg'))):
        return gitrepo
    # detect a bare repository
    if (os.path.exists(os.path.join(p, 'HEAD')) and
        os.path.exists(os.path.join(p, 'objects')) and
        os.path.exists(os.path.join(p, 'refs')) and
        not os.path.exists(os.path.join(p, '.hg'))):
        return gitrepo
    return _oldlocal(path)

hg.schemes['file'] = _local

hgdefaultdest = hg.defaultdest
def defaultdest(source):
    for scheme in ('git', 'git+ssh'):
        if source.startswith('%s://' % scheme) and source.endswith('.git'):
            source = source[:-4]
            break
    return hgdefaultdest(source)
hg.defaultdest = defaultdest

def reposetup(ui, repo):
    klass = hgrepo.generate_repo_subclass(repo.__class__)
    repo.__class__ = klass

def gimport(ui, repo, remote_name=None):
    git = GitHandler(repo, ui)
    git.import_commits(remote_name)

def gexport(ui, repo):
    git = GitHandler(repo, ui)
    git.export_commits()

def gclear(ui, repo):
    repo.ui.status(_("clearing out the git cache data\n"))
    git = GitHandler(repo, ui)
    git.clear()

cmdtable = {
  "gimport":
        (gimport, [], _('hg gimport')),
  "gexport":
        (gexport, [], _('hg gexport')),
  "gclear":
      (gclear, [], _('Clears out the Git cached data')),
}
