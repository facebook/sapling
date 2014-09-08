from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals

import getpass
import json
import os
import subprocess

from mercurial import commands
from mercurial.extensions import wrapcommand

# TODO: Delete bookmarks on server when they are deleted
# client-side

# This needs to be changed
TOOL_PATH = ('/data/users/mjberger/fbcode/_build/dbg/tools/fb_scm/' +
             'fb_share_bookmarks/hg_bookmark_manager.lpar')

def push_bookmark(project, bookmark, dirPath):
    """
    Push the local user's bookmark entry to the database using the
    bookshelf tool
    """
    name = generate_name(project, getpass.getuser(), bookmark)
    cmd = [TOOL_PATH, 'push', '--name', name, '--path', dirPath]
    p = subprocess.Popen(cmd,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    out, err = p.communicate()
    if p.returncode != 0:
        log = os.path.join(dirPath, '.bookshelf_log')
        try:
            os.remove(log)
        except:
            pass
        with open(log, 'w+') as f:
            f.write("STDOUT:\n")
            f.write(out)
            f.write("\n\nSTDERR:\n")
            f.write(err)
        return False
    return True

def pull_bookmark(project, user, bookmark):
    """
    Pull information about a remote bookmark using the bookshelf tool
    """
    name = generate_name(project, user, bookmark)
    cmd = [TOOL_PATH, 'pull', '--name', name]
    p = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE)
    out, err = p.communicate()
    if p.returncode != 0:
        return None
    return json.loads(out)

def get_project_name(dirPath):
    """
    Return the local project's name. If there is no .projectid file,
    we return the name of the directory.
    """
    filePath = os.path.join(dirPath, '.projectid')
    if not os.path.isfile(filePath):
        return dirPath.split('/')[-1]

    with open(filePath) as f:
        name = f.readlines()
        return name[0].strip()

def get_remote_bookmark(ui, repo, info):
    """
    Pull the appropriate bookmark using hg from a dev server
    based on the supplied info from the bookshelf tool.
    """
    user = getpass.getuser()
    server = info['server']
    path = info['root']
    # Hg doesn't like unicode so we cast this format as a str
    # because we've imported unicode_literals
    source = str('ssh://{0}@{1}/{2}'.format(user, server, path))
    code = commands.pull(ui, repo, source,
        bookmark=[info['bookmark']], update=True)
    return code == 0

def commithook(ui, repo, **kwargs):
    """
    The commit hook for hg. Every time a user commits, we send
    the bookmark's information to the central server.
    """
    project = get_project_name(repo.root)
    ctx = repo['.']
    for bookmark in ctx.bookmarks():
        # Having a slash in the name will mess up our rudimentary
        # naming scheme.
        if '/' in bookmark:
            continue
        if push_bookmark(project, bookmark, repo.root):
            # We seem to be running python2.6 :-/
            ui.write("Wrote '%s' to bookshelf\n" % bookmark)
        else:
            ui.write("Failed to write '%s' to bookshelf\n" % bookmark)
            log = os.path.join(repo.root, '.bookshelf_log')
            ui.write("Wrote tool's output to: %s\n" % log)

def checkout(orig, ui, repo, *pats, **opts):
    """
    The checkout wrapper for hg. Every time a user checks out a bookmark,
    we check to see if the desired bookmark is a remote bookmark. We
    consider a remote bookmark to be any bookmark with a '/' in its name.
    If we can find the corresponding bookmark in the database, we attempt
    to pull the remote bookmark.
    """
    bookmark = pats[0]
    # Local bookmarks take precedence over remote bookmarks
    if '/' not in bookmark or bookmark in repo._bookmarks:
        return orig(ui, repo, *pats, **opts)

    project = get_project_name(repo.root)
    user, bookmark = bookmark.split('/')
    info = pull_bookmark(project, user, bookmark)
    if info is None:
        return orig(ui, repo, *pats, **opts)

    get_remote_bookmark(ui, repo, info)

def generate_name(project, user, bookmark):
    return "{0}/{1}/{2]".format(project, user, bookmark)

wrapcommand(commands.table, 'checkout', checkout,)

# This doesn't seem to do anything but leaving it here because the
# hg extension guide says to put this
def uisetup(ui):
    ui.setconfig('hooks', 'commit.bookshelf', commithook)

def reposetup(ui, repo):
    ui.setconfig("hooks", "commit.bookshelf", commithook)
