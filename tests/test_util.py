import StringIO
import difflib
import errno
import gettext
import os
import shutil
import stat
import subprocess
import sys
import tarfile
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
from mercurial import scmutil
from mercurial import ui
from mercurial import util
from mercurial import extensions

from hgsubversion import compathacks

try:
    from mercurial import obsolete
    obsolete._enabled
except ImportError:
    obsolete = None

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
from hgsubversion import svnwrap

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
          'subdir_is_file_prefix.svndump': '/flaf',
          'renames_with_prefix.svndump': '/prefix',
          }
# map defining the layouts of the fixtures we can use with custom layout
# these are really popular layouts, so I gave them names
trunk_only = {
    'default': 'trunk',
    }
trunk_dev_branch = {
    'default': 'trunk',
    'dev_branch': 'branches/dev_branch',
    }
custom = {
    'addspecial.svndump': {
        'default': 'trunk',
        'foo': 'branches/foo',
        },
    'binaryfiles.svndump': trunk_only,
    'branch_create_with_dir_delete.svndump': trunk_dev_branch,
    'branch_delete_parent_dir.svndump': trunk_dev_branch,
    'branchmap.svndump': {
        'default': 'trunk',
        'badname': 'branches/badname',
        'feature': 'branches/feature',
        },
    'branch_prop_edit.svndump': trunk_dev_branch,
    'branch_rename_to_trunk.svndump': {
        'default': 'trunk',
        'dev_branch': 'branches/dev_branch',
        'old_trunk': 'branches/old_trunk',
        },
    'copies.svndump': trunk_only,
    'copyafterclose.svndump': {
        'default': 'trunk',
        'test': 'branches/test'
        },
    'copybeforeclose.svndump': {
        'default': 'trunk',
        'test': 'branches/test'
        },
    'delentries.svndump': trunk_only,
    'delete_restore_trunk.svndump': trunk_only,
    'empty_dir_in_trunk_not_repo_root.svndump': trunk_only,
    'executebit.svndump': trunk_only,
    'filecase.svndump': trunk_only,
    'file_not_in_trunk_root.svndump': trunk_only,
    'project_name_with_space.svndump': trunk_dev_branch,
    'pushrenames.svndump': trunk_only,
    'rename_branch_parent_dir.svndump': trunk_dev_branch,
    'renamedproject.svndump': {
        'default': 'trunk',
        'branch': 'branches/branch',
        },
    'renames.svndump': {
        'default': 'trunk',
        'branch1': 'branches/branch1',
        },
    'renames_with_prefix.svndump': {
        'default': 'trunk',
        'branch1': 'branches/branch1',
        },
    'replace_branch_with_branch.svndump': {
        'default': 'trunk',
        'branch1': 'branches/branch1',
        'branch2': 'branches/branch2',
        },
    'replace_trunk_with_branch.svndump': {
        'default': 'trunk',
        'test': 'branches/test',
        },
    'revert.svndump': trunk_only,
    'siblingbranchfix.svndump': {
        'default': 'trunk',
        'wrongbranch': 'branches/wrongbranch',
        },
    'simple_branch.svndump': {
        'default': 'trunk',
        'the_branch': 'branches/the_branch',
        },
    'spaces-in-path.svndump': trunk_dev_branch,
    'symlinks.svndump': trunk_only,
    'truncatedhistory.svndump': trunk_only,
    'unorderedbranch.svndump': {
        'default': 'trunk',
        'branch': 'branches/branch',
        },
    'unrelatedbranch.svndump': {
        'default': 'trunk',
        'branch1': 'branches/branch1',
        'branch2': 'branches/branch2',
        },
}

FIXTURES = os.path.join(os.path.abspath(os.path.dirname(__file__)),
                        'fixtures')

def getlocalpeer(repo):
    localrepo = getattr(repo, 'local', lambda: repo)()
    if isinstance(localrepo, bool):
        localrepo = repo
    return localrepo

def repolen(repo):
    """Naively calculate the amount of available revisions in a repository.

    this is usually equal to len(repo) -- except in the face of
    obsolete revisions.
    """
    # kind of nasty way of calculating the length, but fortunately,
    # our test repositories tend to be rather small
    return len([r for r in repo])

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

def requiresreplay(method):
    '''Skip a test in stupid mode.'''
    def test(self, *args, **kwargs):
        if self.stupid:
            if SkipTest:
                raise SkipTest("test requires replay mode")
        else:
            return method(self, *args, **kwargs)

    test.__name__ = method.__name__
    return test

def filtermanifest(manifest):
    return [f for f in manifest if f not in util.ignoredfiles]

def fileurl(path):
    path = os.path.abspath(path).replace(os.sep, '/')
    drive, path = os.path.splitdrive(path)
    if drive:
        # In svn 1.7, the swig svn wrapper returns local svn URLs
        # with an uppercase drive letter, try to match that to
        # simplify svn info tests.
        drive = '/' + drive.upper()
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
    cmd = getattr(dispatchmod, 'request', lambda x: x)(cmd)
    return dispatchmod.dispatch(cmd)

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

def hgclone(ui, source, dest, update=True, rev=None):
    if getattr(hg, 'peer', None):
        # Since 1.9 (d976542986d2)
        src, dest = hg.clone(ui, {}, source, dest, update=update, rev=rev)
    else:
        src, dest = hg.clone(ui, source, dest, update=update, rev=rev)
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


def _obsolete_wrap(cls, name):
    origfunc = getattr(cls, name)

    if not name.startswith('test_') or not origfunc:
        return

    if not obsolete:
        wrapper = _makeskip(name, 'obsolete not available')
    else:
        def wrapper(self, *args, **opts):
            self.assertFalse(obsolete._enabled, 'obsolete was already active')

            obsolete._enabled = True

            try:
                    origfunc(self, *args, **opts)
                    self.assertTrue(obsolete._enabled, 'obsolete remains active')
            finally:
                obsolete._enabled = False

    if not wrapper:
        return

    wrapper.__name__ = name + ' obsolete'
    wrapper.__module__ = origfunc.__module__

    if origfunc.__doc__:
        firstline = origfunc.__doc__.strip().splitlines()[0]
        wrapper.__doc__ = firstline + ' (obsolete)'

    assert getattr(cls, wrapper.__name__, None) is None

    setattr(cls, wrapper.__name__, wrapper)


def _stupid_wrap(cls, name):
    origfunc = getattr(cls, name)

    if not name.startswith('test_') or not origfunc:
        return

    def wrapper(self, *args, **opts):
        self.assertFalse(self.stupid, 'stupid mode was already active')

        self.stupid = True

        try:
            origfunc(self, *args, **opts)
        finally:
            self.stupid = False

    wrapper.__name__ = name + ' stupid'
    wrapper.__module__ = origfunc.__module__

    if origfunc.__doc__:
        firstline = origfunc.__doc__.strip().splitlines()[0]
        wrapper.__doc__ = firstline + ' (stupid)'

    assert getattr(cls, wrapper.__name__, None) is None

    setattr(cls, wrapper.__name__, wrapper)

class TestMeta(type):
    def __init__(cls, *args, **opts):
        if cls.obsolete_mode_tests:
            for origname in dir(cls):
                _obsolete_wrap(cls, origname)

        if cls.stupid_mode_tests:
            for origname in dir(cls):
                _stupid_wrap(cls, origname)

        return super(TestMeta, cls).__init__(*args, **opts)

class TestBase(unittest.TestCase):
    __metaclass__ = TestMeta

    obsolete_mode_tests = False
    stupid_mode_tests = False

    stupid = False

    def setUp(self):
        _verify_our_modules()
        if 'hgsubversion' in sys.modules:
            sys.modules['hgext_hgsubversion'] = sys.modules['hgsubversion']

        # the Python 2.7 default of 640 is obnoxiously low
        self.maxDiff = 4096

        self.oldenv = dict([(k, os.environ.get(k, None),) for k in
                           ('LANG', 'LC_ALL', 'HGRCPATH',)])
        self.oldt = i18n.t
        os.environ['LANG'] = os.environ['LC_ALL'] = 'C'
        i18n.t = gettext.translation('hg', i18n.localedir, fallback=True)

        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp(
            'svnwrap_test', dir=os.environ.get('HGSUBVERSION_TEST_TEMP', None))
        os.chdir(self.tmpdir)
        self.hgrc = os.path.join(self.tmpdir, '.hgrc')
        os.environ['HGRCPATH'] = self.hgrc
        scmutil._rcpath = None
        rc = open(self.hgrc, 'w')
        rc.write('[ui]\nusername=test-user\n')
        for l in '[extensions]', 'hgsubversion=':
            print >> rc, l

        self.repocount = 0
        self.wc_path = '%s/testrepo_wc' % self.tmpdir
        self.svn_wc = None

        self.config_dir = self.tmpdir
        svnwrap.common._svn_config_dir = self.config_dir
        self.setup_svn_config('')

        # Previously, we had a MockUI class that wrapped ui, and giving access
        # to the stream. The ui.pushbuffer() and ui.popbuffer() can be used
        # instead. Using the regular UI class, with all stderr redirected to
        # stdout ensures that the test setup is much more similar to usage
        # setups.
        self.patch = (ui.ui.write_err, ui.ui.write)
        setattr(ui.ui, self.patch[0].func_name, self.patch[1])

    def setup_svn_config(self, config):
        c = open(self.config_dir + '/config', 'w')
        try:
            c.write(config)
        finally:
            c.close()

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

    def ui(self, layout='auto'):
        return testui(self.stupid, layout)

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

    def load_repo_tarball(self, fixture_name):
        '''Extracts a tarball of an svn repo and returns the svn repo path.'''
        path = self._makerepopath()
        assert not os.path.exists(path)
        os.mkdir(path)
        tarball = tarfile.open(os.path.join(FIXTURES, fixture_name))
        # This is probably somewhat fragile, but I'm not sure how to
        # do better in particular, I think it assumes that the tar
        # entries are in the right order and that directories appear
        # before their contents.  This is a valid assummption for sane
        # tarballs, from what I can tell.  In particular, for a simple
        # tarball of a svn repo with paths relative to the repo root,
        # it seems to work
        for entry in tarball:
            tarball.extract(entry, path)
        return path

    def fetch(self, repo_path, subdir=None, layout='auto',
            startrev=0, externals=None, noupdate=True, dest=None, rev=None,
            config=None):
        if layout == 'single':
            if subdir is None:
                subdir = 'trunk'
        elif subdir is None:
            subdir = ''
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
        if self.stupid:
            cmd.append('--stupid')
        if noupdate:
            cmd.append('--noupdate')
        if rev is not None:
            cmd.append('--rev=%s' % rev)
        config = dict(config or {})
        if externals:
            config['hgsubversion.externals'] = str(externals)
        for k,v in reversed(sorted(config.iteritems())):
            cmd[:0] = ['--config', '%s=%s' % (k, v)]

        r = dispatch(cmd)
        assert not r, 'fetch of %s failed' % projectpath

        return hg.repository(testui(), self.wc_path)

    def load_and_fetch(self, fixture_name, *args, **opts):
        if fixture_name.endswith('.svndump'):
            repo_path = self.load_svndump(fixture_name)
        elif fixture_name.endswith('tar.gz'):
            repo_path = self.load_repo_tarball(fixture_name)
        else:
            assert False, 'Unknown fixture type'

        return self.fetch(repo_path, *args, **opts), repo_path

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

    def pushrevisions(self, expected_extra_back=0):
        before = repolen(self.repo)
        self.repo.ui.setconfig('hgsubversion', 'stupid', str(self.stupid))
        res = commands.push(self.repo.ui, self.repo)
        after = repolen(self.repo)
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
            return compathacks.makememfilectx(repo,
                                              path=dest,
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

    def getgraph(self, repo):
        """Helper function displaying a repository graph, especially
        useful when debugging comprehensive tests.
        """
        # Could be more elegant, but it works with stock hg
        _ui = ui.ui()
        _ui.setconfig('extensions', 'graphlog', '')
        extensions.loadall(_ui)
        graphlog = extensions.find('graphlog')
        templ = """\
changeset: {rev}:{node|short} (r{svnrev})
branch:    {branches}
tags:      {tags}
summary:   {desc|firstline}
files:     {files}

"""
        _ui.pushbuffer()
        graphlog.graphlog(_ui, repo, rev=None, template=templ)
        return _ui.popbuffer()

    def draw(self, repo):
        sys.stdout.write(self.getgraph(repo))
