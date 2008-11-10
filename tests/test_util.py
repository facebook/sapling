import errno
import os
import subprocess
import shutil
import stat
import urllib

from mercurial import ui
from mercurial import hg

import fetch_command

FIXTURES = os.path.join(os.path.abspath(os.path.dirname(__file__)),
                        'fixtures')

def fileurl(path):    
    path = os.path.abspath(path)
    drive, path = os.path.splitdrive(path)
    path = urllib.pathname2url(path)
    if drive:
        drive = '/' + drive
    url = 'file://%s%s' % (drive, path)
    return url

def load_svndump_fixture(path, fixture_name):
    '''Loads an svnadmin dump into a fresh repo at path, which should not
    already exist.
    '''
    subprocess.call(['svnadmin', 'create', path,])
    proc = subprocess.Popen(['svnadmin', 'load', path,], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    inp = open(os.path.join(FIXTURES, fixture_name))
    proc.stdin.write(inp.read())
    proc.stdin.flush()
    proc.communicate()

def load_fixture_and_fetch(fixture_name, repo_path, wc_path, stupid=False):
    load_svndump_fixture(repo_path, fixture_name)
    fetch_command.fetch_revisions(ui.ui(),
                                  svn_url=fileurl(repo_path),
                                  hg_repo_path=wc_path,
                                  stupid=stupid)
    repo = hg.repository(ui.ui(), wc_path)
    return repo

def rmtree(path):
    # Read-only files cannot be removed under Windows
    for root, dirs, files in os.walk(path):
        for f in files:
            f = os.path.join(root, f)
            try:
                s = os.stat(f)
            except OSError, e:
                if e.errno == errno.ENOENT:
                    continue
                raise
            if (s.st_mode & stat.S_IWRITE) == 0:
                os.chmod(f, s.st_mode | stat.S_IWRITE)
    shutil.rmtree(path)
