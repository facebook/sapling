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

'''

from mercurial import commands
from mercurial import hg
from mercurial import util
from mercurial import bundlerepo
from mercurial.i18n import _
import os
from git_handler import GitHandler

# support for `hg clone git://github.com/defunkt/facebox.git`
# also hg clone git+ssh://git@github.com/schacon/simplegit.git
import gitrepo, hgrepo
hg.schemes['git'] = gitrepo
hg.schemes['git+ssh'] = gitrepo

def _local(path):
    return (os.path.isfile(util.drop_scheme('file', path)) and
            bundlerepo or hgrepo)

hg.schemes['file'] = _local

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

commands.norepo += " gclone"
cmdtable = {
  "gimport":
        (gimport, [], _('hg gimport')),
  "gexport":
        (gexport, [], _('hg gexport')),
  "gclear":
      (gclear, [], _('Clears out the Git cached data')),
}
