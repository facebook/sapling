import errno
import os
import subprocess
import shutil
import stat
import tempfile
import unittest
import urllib

from mercurial import context
from mercurial import hg
from mercurial import node
from mercurial import ui

import fetch_command
import push_cmd

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

class TestBase(unittest.TestCase):
    def setUp(self):
        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        self.wc_path = '%s/testrepo_wc' % self.tmpdir

    def tearDown(self):
        rmtree(self.tmpdir)
        os.chdir(self.oldwd)
        
    # define this as a property so that it reloads anytime we need it
    @property
    def repo(self):
        return hg.repository(ui.ui(), self.wc_path)

    def pushrevisions(self):
        push_cmd.push_revisions_to_subversion(
            ui.ui(), repo=self.repo, hg_repo_path=self.wc_path,
            svn_url=fileurl(self.repo_path))

    def svnls(self, path, rev='HEAD'):
        path = self.repo_path + '/' + path
        path = fileurl(path)
        args = ['svn', 'ls', '-r', rev, '-R', path]
        p = subprocess.Popen(args, 
                             stdout=subprocess.PIPE, 
                             stderr=subprocess.PIPE)
        stdout, stderr = p.communicate()
        if p.returncode:
            raise Exception('svn ls failed on %s: %r' % (path, stderr))
        entries = [e.strip('/') for e in stdout.splitlines()]
        entries.sort()
        return entries

    def commitchanges(self, changes):
        """Commit changes to mercurial directory

        'changes' is a sequence of tuples (source, dest, data). It can look
        like:
        - (source, source, data) to set source content to data
        - (source, dest, None) to set dest content to source one, and mark it as 
        copied from source.
        - (source, dest, data) to set dest content to data, and mark it as copied
        from source.
        - (source, None, None) to remove source.
        """
        repo = self.repo
        parentctx = repo['tip']

        changed, removed = [], []
        for source, dest, newdata in changes:
            if dest is None:
                removed.append(source)
            else:
                changed.append(dest)

        def filectxfn(repo, memctx, path):
            if path in removed:
                raise IOError()
            entry = [e for e in changes if path == e[1]][0]
            source, dest, newdata = entry
            if newdata is None:
                newdata = parentctx[source].data()
            copied = None
            if source != dest:
                copied = source
            return context.memfilectx(path=dest,
                                      data=newdata,
                                      islink=False,
                                      isexec=False,
                                      copied=copied)
        
        ctx = context.memctx(repo,
                             (parentctx.node(), node.nullid),
                             'automated test',
                             changed + removed,
                             filectxfn,
                             'an_author',
                             '2008-10-07 20:59:48 -0500')
        nodeid = repo.commitctx(ctx)
        repo = self.repo
        hg.update(repo, nodeid)
        return nodeid
