import StringIO
import difflib
import errno
import gettext
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
import urllib

_rootdir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, _rootdir)

from mercurial import cmdutil
from mercurial import commands
from mercurial import context
from mercurial import dispatch as dispatchmod
from mercurial import hg
from mercurial import i18n
from mercurial import node
from mercurial import ui
from mercurial import util
from mercurial import extensions

try:
    SkipTest = unittest.SkipTest
except AttributeError:
    try:
        from unittest2 import SkipTest
    except ImportError:
        try:
            from nose import SkipTest
        except ImportError:
            SkipTest = None

from hgsubversion import util

# Documentation for Subprocess.Popen() says:
#   "Note that on Windows, you cannot set close_fds to true and
#   also redirect the standard handles by setting stdin, stdout or
#   stderr."
canCloseFds = 'win32' not in sys.platform

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

    CloseHandle = WINAPI(BOOL, ctypes.windll.kernel32.CloseHandle,
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
          'non_ascii_path_1.svndump': '/b\xC3\xB8b',
          'non_ascii_path_2.svndump': '/b%C3%B8b',
          }

FIXTURES = os.path.join(os.path.abspath(os.path.dirname(__file__)),
                        'fixtures')


def _makeskip(name, message):
    if SkipTest:
        def skip(*args, **kwargs):
            raise SkipTest(message)
        skip.__name__ = name
        return skip

def requiresmodule(mod):
    """Skip a test if the specified module is not None."""
    def decorator(fn):
        if fn is None:
            return
        if mod is not None:
            return fn
        return _makeskip(fn.__name__, 'missing required feature')
    return decorator


def requiresoption(option):
    '''Skip a test if commands.clone does not take the specified option.'''
    def decorator(fn):
        for entry in cmdutil.findcmd('clone', commands.table)[1][1]:
            if entry[1] == option:
                return fn
        # no match found, so skip
        if SkipTest:
            return _makeskip(fn.__name__,
                             'test requires clone to accept %s' % option)
        # no skipping support, so erase decorated method
        return
    if not isinstance(option, str):
        raise TypeError('requiresoption takes a string argument')
    return decorator

def filtermanifest(manifest):
    return [f for f in manifest if f not in util.ignoredfiles]

def fileurl(path):
    path = os.path.abspath(path).replace(os.sep, '/')
    drive, path = os.path.splitdrive(path)
    if drive:
        drive = '/' + drive
    url = 'file://%s%s' % (drive, path)
    return url

def testui(stupid=False, layout='auto', startrev=0):
    u = ui.ui()
    bools = {True: 'true', False: 'false'}
    u.setconfig('ui', 'quiet', bools[True])
    u.setconfig('extensions', 'hgsubversion', '')
    u.setconfig('hgsubversion', 'stupid', bools[stupid])
    u.setconfig('hgsubversion', 'layout', layout)
    u.setconfig('hgsubversion', 'startrev', startrev)
    return u

def dispatch(cmd):
    try:
        req = dispatchmod.request(cmd)
        dispatchmod.dispatch(req)
    except AttributeError, e:
        dispatchmod.dispatch(cmd)

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

def _verify_our_modules():
    '''
    Verify that hgsubversion was imported from the correct location.

    The correct location is any location within the parent directory of the
    directory containing this file.
    '''

    for modname, module in sys.modules.iteritems():
        if not module or not modname.startswith('hgsubversion.'):
            continue

        modloc = module.__file__
        cp = os.path.commonprefix((os.path.abspath(__file__), modloc))
        assert cp.rstrip(os.sep) == _rootdir, (
            'Module location verification failed: hgsubversion was imported '
            'from the wrong path!'
        )

def hgclone(ui, source, dest, update=True):
    if getattr(hg, 'peer', None):
        # Since 1.9 (d976542986d2)
        src, dest = hg.clone(ui, {}, source, dest, update=update)
    else:
        src, dest = hg.clone(ui, source, dest, update=update)
    return src, dest

def svnls(repo_path, path, rev='HEAD'):
    path = repo_path + '/' + path
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

def svnpropget(repo_path, path, prop, rev='HEAD'):
    path = repo_path + '/' + path
    path = util.normalize_url(fileurl(path))
    args = ['svn', 'propget', '-r', str(rev), prop, path]
    p = subprocess.Popen(args,
                         stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT)
    stdout, stderr = p.communicate()
    if p.returncode:
        raise Exception('svn ls failed on %s: %r' % (path, stderr))
    return stdout.strip()

class TestBase(unittest.TestCase):
    def setUp(self):
        _verify_our_modules()

        self.oldenv = dict([(k, os.environ.get(k, None),) for k in
                           ('LANG', 'LC_ALL', 'HGRCPATH',)])
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

        self.repocount = 0
        self.wc_path = '%s/testrepo_wc' % self.tmpdir
        self.svn_wc = None

        # Previously, we had a MockUI class that wrapped ui, and giving access
        # to the stream. The ui.pushbuffer() and ui.popbuffer() can be used
        # instead. Using the regular UI class, with all stderr redirected to
        # stdout ensures that the test setup is much more similar to usage
        # setups.
        self.patch = (ui.ui.write_err, ui.ui.write)
        setattr(ui.ui, self.patch[0].func_name, self.patch[1])

    def _makerepopath(self):
        self.repocount += 1
        return '%s/testrepo-%d' % (self.tmpdir, self.repocount)

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

        _verify_our_modules()

    def ui(self, stupid=False, layout='auto'):
        return testui(stupid, layout)

    def load_svndump(self, fixture_name):
        '''Loads an svnadmin dump into a fresh repo. Return the svn repo
        path.
        '''
        path = self._makerepopath()
        assert not os.path.exists(path)
        subprocess.call(['svnadmin', 'create', path,],
                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        inp = open(os.path.join(FIXTURES, fixture_name))
        proc = subprocess.Popen(['svnadmin', 'load', path,], stdin=inp,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        proc.communicate()
        return path

    def load_and_fetch(self, fixture_name, subdir=None, stupid=False,
                       layout='auto', startrev=0, externals=None,
                       noupdate=True):
        if layout == 'single':
            if subdir is None:
                subdir = 'trunk'
        elif subdir is None:
            subdir = ''
        repo_path = self.load_svndump(fixture_name)
        projectpath = repo_path
        if subdir:
            projectpath += '/' + subdir

        cmd = [
            'clone',
            '--layout=%s' % layout,
            '--startrev=%s' % startrev,
            fileurl(projectpath),
            self.wc_path,
            ]
        if stupid:
            cmd.append('--stupid')
        if noupdate:
            cmd.append('--noupdate')
        if externals:
            cmd[:0] = ['--config', 'hgsubversion.externals=%s' % externals]

        dispatch(cmd)

        return hg.repository(testui(), self.wc_path), repo_path

    def _load_fixture_and_fetch(self, *args, **kwargs):
        repo, repo_path = self.load_and_fetch(*args, **kwargs)
        return repo

    def add_svn_rev(self, repo_path, changes):
        '''changes is a dict of filename -> contents'''
        if self.svn_wc is None:
            self.svn_wc = os.path.join(self.tmpdir, 'testsvn_wc')
            subprocess.call([
                'svn', 'co', '-q', fileurl(repo_path),
                self.svn_wc
            ],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

        for filename, contents in changes.iteritems():
            # filenames are / separated
            filename = filename.replace('/', os.path.sep)
            filename = os.path.join(self.svn_wc, filename)
            open(filename, 'w').write(contents)
            # may be redundant
            subprocess.call(['svn', 'add', '-q', filename],
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        subprocess.call([
            'svn', 'commit', '-q', self.svn_wc, '-m', 'test changes'],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)

    # define this as a property so that it reloads anytime we need it
    @property
    def repo(self):
        return hg.repository(testui(), self.wc_path)

    def pushrevisions(self, stupid=False, expected_extra_back=0):
        before = len(self.repo)
        self.repo.ui.setconfig('hgsubversion', 'stupid', str(stupid))
        res = commands.push(self.repo.ui, self.repo)
        after = len(self.repo)
        self.assertEqual(expected_extra_back, after - before)
        return res

    def svnco(self, repo_path, svnpath, rev, path):
        path = os.path.join(self.wc_path, path)
        subpath = os.path.dirname(path)
        if not os.path.isdir(subpath):
            os.makedirs(subpath)
        svnpath = fileurl(repo_path + '/' + svnpath)
        args = ['svn', 'co', '-r', rev, svnpath, path]
        p = subprocess.Popen(args,
                             stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT)
        stdout, stderr = p.communicate()
        if p.returncode:
            raise Exception('svn co failed on %s: %r' % (svnpath, stderr))

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
                raise IOError(errno.ENOENT,
                              "File \"%s\" no longer exists" % path)
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

    def assertMultiLineEqual(self, first, second, msg=None):
        """Assert that two multi-line strings are equal. (Based on Py3k code.)
        """
        try:
            return super(TestBase, self).assertMultiLineEqual(first, second,
                                                              msg)
        except AttributeError:
            pass

        self.assert_(isinstance(first, str),
                     ('First argument is not a string'))
        self.assert_(isinstance(second, str),
                     ('Second argument is not a string'))

        if first != second:
            diff = ''.join(difflib.unified_diff(first.splitlines(True),
                                                second.splitlines(True),
                                                fromfile='a',
                                                tofile='b'))
            msg = '%s\n%s' % (msg or '', diff)
            raise self.failureException, msg

    def draw(self, repo):
        """Helper function displaying a repository graph, especially
        useful when debugging comprehensive tests.
        """
        # Could be more elegant, but it works with stock hg
        _ui = ui.ui()
        _ui.setconfig('extensions', 'graphlog', '')
        extensions.loadall(_ui)
        graphlog = extensions.find('graphlog')
        templ = """\
changeset: {rev}:{node|short}
branch:    {branches}
tags:      {tags}
summary:   {desc|firstline}
files:     {files}

"""
        graphlog.graphlog(_ui, repo, rev=None, template=templ)
