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

#
# Stage One - use Git commands to do the import / pushes, all in one big uggo file
#
# Stage Two - implement the Git packfile generation and server communication
#             in native Python, so we don't need Git locally and don't need
#             to keep all the git repo data around.  We should just need a SHA
#             mapping - since everything is append only in both systems it should
#             be pretty simple to do. 
#

# just importing every damn thing because i don't know python that well
# and I have no idea what I actually need
from mercurial import util, repair, merge, cmdutil, commands, error, hg, url
from mercurial import extensions, ancestor
from mercurial.commands import templateopts
from mercurial.node import nullrev, nullid, short
from mercurial.i18n import _
import os, errno
import subprocess

def gclone(ui, git_url, hg_repo_path=None):
    ## TODO : add git_url as the default remote path
    if not hg_repo_path:
        hg_repo_path = hg.defaultdest(git_url)
        if hg_repo_path.endswith('.git'):
            hg_repo_path = hg_repo_path[:-4]
        hg_repo_path += '-hg'
    subprocess.call(['hg', 'init', hg_repo_path])
    clone_git(git_url, git_path(hg_repo_path))
    import_git_heads(hg_repo_path)

def gpull(ui, repo, source='default', **opts):
    """fetch from a git repo
    """
    lock = wlock = None
    try:
        lock = repo.lock()
        wlock = repo.wlock()
        ui.write("fetching from the remote\n")
        git_fetch()
        import_git_heads()
        # do the pull
    finally:
        del lock, wlock

def gpush(ui, repo, dest='default', **opts):
    """push to a git repo
    """
    lock = wlock = None
    try:
        lock = repo.lock()
        wlock = repo.wlock()
        ui.write("pushing to the remote\n")
        # do the push
    finally:
        del lock, wlock

def git_path(hg_path=None):
    return os.path.join(hg_path, '.hg', 'git-remote')

def clone_git(git_url, hg_path=None):
    git_initialize(git_path(hg_path), git_url)
    git_fetch(git_path(hg_path))
    
def git_initialize(git_path, git_url):
    # TODO: implement this in pure python - should be strait-forward
    subprocess.call(['git', '--bare', 'init', git_path])
    oldwd = os.getcwd()
    os.chdir(git_path)
    subprocess.call(['git', 'remote', 'add', 'origin', git_url])
    os.chdir(oldwd)
    
def git_fetch(git_path, remote='origin'):
    # TODO: implement this in pure python
    #       - we'll have to handle ssh and git
    oldwd = os.getcwd()
    os.chdir(git_path)
    subprocess.call(['git', 'fetch', remote])
    os.chdir(oldwd)
  
def git_push():
    # find all the local changesets that aren't mapped
    # create git commit object shas and map them
    # stick those objects in a packfile and push them up (over ssh)
    
def import_git_heads(hg_path=None):
    # go through each branch
      # add all commits we don't have locally
      # write a SHA<->SHA mapping table
      # update the local branches to match
    return subprocess.call(['hg', 'convert', git_path(hg_path), hg_path])
  
        
commands.norepo += " gclone"
cmdtable = {
  "gclone":
      (gclone,
       [ #('A', 'authors', '', 'username mapping filename'),
       ],
       'Clone a git repository into an hg repository.',
       ),
  "gpush":
        (gpush,
         [('m', 'merge', None, _('merge automatically'))],
         _('hg gpush remote')),
  "gpull":
        (gpull, [], _('hg gpull [--merge] remote')),
}