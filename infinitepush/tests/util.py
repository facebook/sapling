import os
import string
import random
from .. import indexapi
from .. import store
import time
import getpass
from mercurial import hg, ui
import subprocess

def getrandomid():
    return ''.join(random.choice("abcdef" + string.digits)
                   for _ in range(32))

def getfilebundlestore(tmpdir):
    repopath = tmpdir.mkdir("repo")
    storepath = tmpdir.mkdir("store")
    repo = getrepo(repopath)
    repo.ui.setconfig("scratchbranch", "storepath", storepath.dirname)
    return store.filebundlestore(repo.ui, repo)

def getrepo(tmpdir):
    os.chdir(tmpdir.dirname)
    os.system("hg init")
    return hg.repository(ui.ui(), tmpdir.dirname)

def getfileindexandrepo(tmpdir):
    repo = getrepo(tmpdir)
    fileindex = indexapi.fileindexapi(repo)
    return fileindex, repo
