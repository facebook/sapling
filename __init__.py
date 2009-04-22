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
from mercurial import util, repair, merge, cmdutil, commands, hg, url
from mercurial import extensions, ancestor
from mercurial.commands import templateopts
from mercurial.node import nullrev, nullid, short
from mercurial.i18n import _
import os, errno, sys
import subprocess
import dulwich

def gclone(ui, git_url, hg_repo_path=None):
    ## TODO : add git_url as the default remote path
    if not hg_repo_path:
        hg_repo_path = hg.defaultdest(git_url)
        if hg_repo_path.endswith('.git'):
            hg_repo_path = hg_repo_path[:-4]
        hg_repo_path += '-hg'
    dest_repo = hg.repository(ui, hg_repo_path, create=True)

    # make the git data directory
    git_hg_path = os.path.join(hg_repo_path, '.hg', 'git')
    os.mkdir(git_hg_path)
    dulwich.repo.Repo.init_bare(git_hg_path)
    
    # fetch the initial git data
    git_fetch(dest_repo, git_url)
    
    # checkout the tip
    # hg.update(ui, dest_repo)

def gpush(ui, repo):
    dest_repo.ui.status(_("pushing to git url\n"))
    
def gpull(ui, repo):
    dest_repo.ui.status(_("pulling from git url\n"))
    

def git_fetch(dest_repo, git_url):
    dest_repo.ui.status(_("fetching from git url\n"))
    git_fetch_pack(dest_repo, git_url)
    
def git_fetch_pack(dest_repo, git_url):
    from dulwich.repo import Repo
    from dulwich.client import SimpleFetchGraphWalker
    client, path = get_transport_and_path(git_url)
    git_dir = os.path.join(dest_repo.path, 'git')
    r = Repo(git_dir)
    graphwalker = SimpleFetchGraphWalker(r.heads().values(), r.get_parents)
    f, commit = r.object_store.add_pack()
    try:
        client.fetch_pack(path, r.object_store.determine_wants_all, graphwalker, f.write, sys.stdout.write)
        f.close()
        commit()
    except:
        f.close()
    raise

def get_transport_and_path(uri):
    from dulwich.client import TCPGitClient, SSHGitClient, SubprocessGitClient
    for handler, transport in (("git://", TCPGitClient), ("git+ssh://", SSHGitClient)):
        if uri.startswith(handler):
            host, path = uri[len(handler):].split("/", 1)
            return transport(host), "/"+path
    # if its not git or git+ssh, try a local url..
    return SubprocessGitClient(), uri
        
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