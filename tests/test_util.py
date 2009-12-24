import StringIO
import errno
import gettext
import imp
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
import urllib

import __init__

from mercurial import commands
from mercurial import context
from mercurial import hg
from mercurial import i18n
from mercurial import node
from mercurial import ui

from hgsubversion import util

# Documentation for Subprocess.Popen() says:
#   "Note that on Windows, you cannot set close_fds to true and
#   also redirect the standard handles by setting stdin, stdout or
#   stderr."
canCloseFds='win32' not in sys.platform

if not 'win32' in sys.platform:
    def kill_process(popen_obj):
        os.kill(popen_obj.pid, 9)
else:
    import ctypes
    from ctypes.wintypes import BOOL, DWORD, HANDLE, UINT

    def win_status_check(result, func, args):
        if result == 0:
            raise ctypes.WinError()
        return args

    def WINAPI(returns, func, *params):
        assert len(params) % 2 == 0

        func.argtypes = tuple(params[0::2])
        func.resvalue = returns
        func.errcheck = win_status_check

        return func

    # dwDesiredAccess
    PROCESS_TERMINATE = 0x0001

    OpenProcess = WINAPI(HANDLE, ctypes.windll.kernel32.OpenProcess,
        DWORD, 'dwDesiredAccess',
        BOOL, 'bInheritHandle',
        DWORD, 'dwProcessId',
    )

    CloseHandle =  WINAPI(BOOL, ctypes.windll.kernel32.CloseHandle,
        HANDLE, 'hObject'
    )

    TerminateProcess = WINAPI(BOOL, ctypes.windll.kernel32.TerminateProcess,
        HANDLE, 'hProcess',
        UINT, 'uExitCode'
    )

    def kill_process(popen_obj):
        phnd = OpenProcess(PROCESS_TERMINATE, False, popen_obj.pid)
        TerminateProcess(phnd, 1)
        CloseHandle(phnd)

# Fixtures that need to be pulled at a subdirectory of the repo path
subdir = {'truncatedhistory.svndump': '/project2',
          'fetch_missing_files_subdir.svndump': '/foo',
          'empty_dir_in_trunk_not_repo_root.svndump': '/project',
          'project_root_not_repo_root.svndump': '/dummyproj',
          'project_name_with_space.svndump': '/project name',
          }

FIXTURES = os.path.join(os.path.abspath(os.path.dirname(__file__)),
                        'fixtures')

def filtermanifest(manifest):
    return filter(lambda x: x not in ('.hgtags', '.hgsvnexternals', ),
                  manifest)

def fileurl(path):
    path = os.path.abspath(path)
    drive, path = os.path.splitdrive(path)
    if drive:
        drive = '/' + drive
    url = 'file://%s%s' % (drive, path)
    return url

def load_svndump_fixture(path, fixture_name):
    '''Loads an svnadmin dump into a fresh repo at path, which should not
    already exist.
    '''
    if os.path.exists(path): rmtree(path)
    subprocess.call(['svnadmin', 'create', path,],
                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    inp = open(os.path.join(FIXTURES, fixture_name))
    proc = subprocess.Popen(['svnadmin', 'load', path,], stdin=inp,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    proc.communicate()

def load_fixture_and_fetch(fixture_name, repo_path, wc_path, stupid=False, subdir='',
                           noupdate=True, layout='auto'):
    load_svndump_fixture(repo_path, fixture_name)
    if subdir:
        repo_path += '/' + subdir

    confvars = locals()
    def conf():
        for var in ('stupid', 'layout'):
            _ui = ui.ui()
            _ui.setconfig('hgsubversion', var, confvars[var])
        return _ui
    _ui = conf()
    commands.clone(_ui, fileurl(repo_path), wc_path, noupdate=noupdate)
    _ui = conf()
    return hg.repository(_ui, wc_path)

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
        self.oldenv = dict([(k, os.environ.get(k, None), ) for k in
                           ('LANG', 'LC_ALL', 'HGRCPATH', )])
        self.oldt = i18n.t
        os.environ['LANG'] = os.environ['LC_ALL'] = 'C'
        i18n.t = gettext.translation('hg', i18n.localedir, fallback=True)

        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp(
            'svnwrap_test', dir=os.environ.get('HGSUBVERSION_TEST_TEMP', None))
        self.hgrc = os.path.join(self.tmpdir, '.hgrc')
        os.environ['HGRCPATH'] = self.hgrc
        rc = open(self.hgrc, 'w')
        for l in '[extensions]', 'hgsubversion=':
            print >> rc, l

        self.repo_path = '%s/testrepo' % self.tmpdir
        self.wc_path = '%s/testrepo_wc' % self.tmpdir

        # Previously, we had a MockUI class that wrapped ui, and giving access
        # to the stream. The ui.pushbuffer() and ui.popbuffer() can be used
        # instead. Using the regular UI class, with all stderr redirected to
        # stdout ensures that the test setup is much more similar to usage
        # setups.
        self.patch = (ui.ui.write_err, ui.ui.write)
        setattr(ui.ui, self.patch[0].func_name, self.patch[1])

    def tearDown(self):
        for var, val in self.oldenv.iteritems():
            if val is None:
                del os.environ[var]
            else:
                os.environ[var] = val
        i18n.t = self.oldt
        rmtree(self.tmpdir)
        os.chdir(self.oldwd)
        setattr(ui.ui, self.patch[0].func_name, self.patch[0])

    def _load_fixture_and_fetch(self, fixture_name, subdir=None, stupid=False, layout='auto'):
        if layout == 'single':
            if subdir is None:
                subdir = 'trunk'
        elif subdir is None:
            subdir = ''
        return load_fixture_and_fetch(fixture_name, self.repo_path,
                                      self.wc_path, subdir=subdir,
                                      stupid=stupid, layout=layout)

    # define this as a property so that it reloads anytime we need it
    @property
    def repo(self):
        return hg.repository(ui.ui(), self.wc_path)

    def pushrevisions(self, stupid=False, expected_extra_back=0):
        before = len(self.repo)
        self.repo.ui.setconfig('hgsubversion', 'stupid', str(stupid))
        commands.push(self.repo.ui, self.repo)
        after = len(self.repo)
        self.assertEqual(expected_extra_back, after - before)

    def svnls(self, path, rev='HEAD'):
        path = self.repo_path + '/' + path
        path = util.normalize_url(fileurl(path))
        args = ['svn', 'ls', '-r', rev, '-R', path]
        p = subprocess.Popen(args,
                             stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT)
        stdout, stderr = p.communicate()
        if p.returncode:
            raise Exception('svn ls failed on %s: %r' % (path, stderr))
        entries = [e.strip('/') for e in stdout.splitlines()]
        entries.sort()
        return entries

    def commitchanges(self, changes, parent='tip', message='automated test'):
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
        parentctx = repo[parent]

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
                             message,
                             changed + removed,
                             filectxfn,
                             'an_author',
                             '2008-10-07 20:59:48 -0500')
        nodeid = repo.commitctx(ctx)
        repo = self.repo
        hg.clean(repo, nodeid)
        return nodeid

    def assertchanges(self, changes, ctx):
        """Assert that all 'changes' (as in defined in commitchanged())
        went into ctx.
        """
        for source, dest, data in changes:
            if dest is None:
                self.assertTrue(source not in ctx)
                continue
            self.assertTrue(dest in ctx)
            if data is None:
                data = ctx.parents()[0][source].data()
            self.assertEqual(ctx[dest].data(), data)
            if dest != source:
                copy = ctx[dest].renamed()
                self.assertEqual(copy[0], source)
