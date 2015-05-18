#!/usr/bin/env python
#
# run-tests.py - Run a set of tests on Mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Modifying this script is tricky because it has many modes:
#   - serial (default) vs parallel (-jN, N > 1)
#   - no coverage (default) vs coverage (-c, -C, -s)
#   - temp install (default) vs specific hg script (--with-hg, --local)
#   - tests are a mix of shell scripts and Python scripts
#
# If you change this script, it is recommended that you ensure you
# haven't broken it by running it in various modes with a representative
# sample of test scripts.  For example:
#
#  1) serial, no coverage, temp install:
#      ./run-tests.py test-s*
#  2) serial, no coverage, local hg:
#      ./run-tests.py --local test-s*
#  3) serial, coverage, temp install:
#      ./run-tests.py -c test-s*
#  4) serial, coverage, local hg:
#      ./run-tests.py -c --local test-s*      # unsupported
#  5) parallel, no coverage, temp install:
#      ./run-tests.py -j2 test-s*
#  6) parallel, no coverage, local hg:
#      ./run-tests.py -j2 --local test-s*
#  7) parallel, coverage, temp install:
#      ./run-tests.py -j2 -c test-s*          # currently broken
#  8) parallel, coverage, local install:
#      ./run-tests.py -j2 -c --local test-s*  # unsupported (and broken)
#  9) parallel, custom tmp dir:
#      ./run-tests.py -j2 --tmpdir /tmp/myhgtests
#
# (You could use any subset of the tests: test-s* happens to match
# enough that it's worth doing parallel runs, few enough that it
# completes fairly quickly, includes both shell and Python scripts, and
# includes some scripts that run daemon processes.)

from __future__ import print_function

from distutils import version
import difflib
import errno
import optparse
import os
import shutil
import subprocess
import signal
import socket
import sys
import tempfile
import time
import random
import re
import threading
import killdaemons as killmod
try:
    import Queue as queue
except ImportError:
    import queue
from xml.dom import minidom
import unittest

osenvironb = getattr(os, 'environb', os.environ)

try:
    import json
except ImportError:
    try:
        import simplejson as json
    except ImportError:
        json = None

processlock = threading.Lock()

if sys.version_info > (3, 0, 0):
    xrange = range # we use xrange in one place, and we'd rather not use range

# subprocess._cleanup can race with any Popen.wait or Popen.poll on py24
# http://bugs.python.org/issue1731717 for details. We shouldn't be producing
# zombies but it's pretty harmless even if we do.
if sys.version_info < (2, 5):
    subprocess._cleanup = lambda: None

def checkportisavailable(port):
    """return true if a port seems free to bind on localhost"""
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.bind(('localhost', port))
        s.close()
        return True
    except socket.error as exc:
        if not exc.errno == errno.EADDRINUSE:
            raise
        return False

closefds = os.name == 'posix'
def Popen4(cmd, wd, timeout, env=None):
    processlock.acquire()
    p = subprocess.Popen(cmd, shell=True, bufsize=-1, cwd=wd, env=env,
                         close_fds=closefds,
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT)
    processlock.release()

    p.fromchild = p.stdout
    p.tochild = p.stdin
    p.childerr = p.stderr

    p.timeout = False
    if timeout:
        def t():
            start = time.time()
            while time.time() - start < timeout and p.returncode is None:
                time.sleep(.1)
            p.timeout = True
            if p.returncode is None:
                terminate(p)
        threading.Thread(target=t).start()

    return p

PYTHON = sys.executable.replace('\\', '/').encode('utf-8')
IMPL_PATH = b'PYTHONPATH'
if 'java' in sys.platform:
    IMPL_PATH = b'JYTHONPATH'

defaults = {
    'jobs': ('HGTEST_JOBS', 1),
    'timeout': ('HGTEST_TIMEOUT', 180),
    'port': ('HGTEST_PORT', 20059),
    'shell': ('HGTEST_SHELL', 'sh'),
}

def parselistfiles(files, listtype, warn=True):
    entries = dict()
    for filename in files:
        try:
            path = os.path.expanduser(os.path.expandvars(filename))
            f = open(path, "rb")
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            if warn:
                print("warning: no such %s file: %s" % (listtype, filename))
            continue

        for line in f.readlines():
            line = line.split(b'#', 1)[0].strip()
            if line:
                entries[line] = filename

        f.close()
    return entries

def getparser():
    """Obtain the OptionParser used by the CLI."""
    parser = optparse.OptionParser("%prog [options] [tests]")

    # keep these sorted
    parser.add_option("--blacklist", action="append",
        help="skip tests listed in the specified blacklist file")
    parser.add_option("--whitelist", action="append",
        help="always run tests listed in the specified whitelist file")
    parser.add_option("--changed", type="string",
        help="run tests that are changed in parent rev or working directory")
    parser.add_option("-C", "--annotate", action="store_true",
        help="output files annotated with coverage")
    parser.add_option("-c", "--cover", action="store_true",
        help="print a test coverage report")
    parser.add_option("-d", "--debug", action="store_true",
        help="debug mode: write output of test scripts to console"
             " rather than capturing and diffing it (disables timeout)")
    parser.add_option("-f", "--first", action="store_true",
        help="exit on the first test failure")
    parser.add_option("-H", "--htmlcov", action="store_true",
        help="create an HTML report of the coverage of the files")
    parser.add_option("-i", "--interactive", action="store_true",
        help="prompt to accept changed output")
    parser.add_option("-j", "--jobs", type="int",
        help="number of jobs to run in parallel"
             " (default: $%s or %d)" % defaults['jobs'])
    parser.add_option("--keep-tmpdir", action="store_true",
        help="keep temporary directory after running tests")
    parser.add_option("-k", "--keywords",
        help="run tests matching keywords")
    parser.add_option("-l", "--local", action="store_true",
        help="shortcut for --with-hg=<testdir>/../hg")
    parser.add_option("--loop", action="store_true",
        help="loop tests repeatedly")
    parser.add_option("--runs-per-test", type="int", dest="runs_per_test",
        help="run each test N times (default=1)", default=1)
    parser.add_option("-n", "--nodiff", action="store_true",
        help="skip showing test changes")
    parser.add_option("-p", "--port", type="int",
        help="port on which servers should listen"
             " (default: $%s or %d)" % defaults['port'])
    parser.add_option("--compiler", type="string",
        help="compiler to build with")
    parser.add_option("--pure", action="store_true",
        help="use pure Python code instead of C extensions")
    parser.add_option("-R", "--restart", action="store_true",
        help="restart at last error")
    parser.add_option("-r", "--retest", action="store_true",
        help="retest failed tests")
    parser.add_option("-S", "--noskips", action="store_true",
        help="don't report skip tests verbosely")
    parser.add_option("--shell", type="string",
        help="shell to use (default: $%s or %s)" % defaults['shell'])
    parser.add_option("-t", "--timeout", type="int",
        help="kill errant tests after TIMEOUT seconds"
             " (default: $%s or %d)" % defaults['timeout'])
    parser.add_option("--time", action="store_true",
        help="time how long each test takes")
    parser.add_option("--json", action="store_true",
                      help="store test result data in 'report.json' file")
    parser.add_option("--tmpdir", type="string",
        help="run tests in the given temporary directory"
             " (implies --keep-tmpdir)")
    parser.add_option("-v", "--verbose", action="store_true",
        help="output verbose messages")
    parser.add_option("--xunit", type="string",
                      help="record xunit results at specified path")
    parser.add_option("--view", type="string",
        help="external diff viewer")
    parser.add_option("--with-hg", type="string",
        metavar="HG",
        help="test using specified hg script rather than a "
             "temporary installation")
    parser.add_option("-3", "--py3k-warnings", action="store_true",
        help="enable Py3k warnings on Python 2.6+")
    parser.add_option('--extra-config-opt', action="append",
                      help='set the given config opt in the test hgrc')
    parser.add_option('--random', action="store_true",
                      help='run tests in random order')
    parser.add_option('--profile-runner', action='store_true',
                      help='run statprof on run-tests')

    for option, (envvar, default) in defaults.items():
        defaults[option] = type(default)(os.environ.get(envvar, default))
    parser.set_defaults(**defaults)

    return parser

def parseargs(args, parser):
    """Parse arguments with our OptionParser and validate results."""
    (options, args) = parser.parse_args(args)

    # jython is always pure
    if 'java' in sys.platform or '__pypy__' in sys.modules:
        options.pure = True

    if options.with_hg:
        options.with_hg = os.path.expanduser(options.with_hg)
        if not (os.path.isfile(options.with_hg) and
                os.access(options.with_hg, os.X_OK)):
            parser.error('--with-hg must specify an executable hg script')
        if not os.path.basename(options.with_hg) == 'hg':
            sys.stderr.write('warning: --with-hg should specify an hg script\n')
    if options.local:
        testdir = os.path.dirname(os.path.realpath(sys.argv[0]).encode('utf-8'))
        hgbin = os.path.join(os.path.dirname(testdir), b'hg')
        if os.name != 'nt' and not os.access(hgbin, os.X_OK):
            parser.error('--local specified, but %r not found or not executable'
                         % hgbin)
        options.with_hg = hgbin

    options.anycoverage = options.cover or options.annotate or options.htmlcov
    if options.anycoverage:
        try:
            import coverage
            covver = version.StrictVersion(coverage.__version__).version
            if covver < (3, 3):
                parser.error('coverage options require coverage 3.3 or later')
        except ImportError:
            parser.error('coverage options now require the coverage package')

    if options.anycoverage and options.local:
        # this needs some path mangling somewhere, I guess
        parser.error("sorry, coverage options do not work when --local "
                     "is specified")

    if options.anycoverage and options.with_hg:
        parser.error("sorry, coverage options do not work when --with-hg "
                     "is specified")

    global verbose
    if options.verbose:
        verbose = ''

    if options.tmpdir:
        options.tmpdir = os.path.expanduser(options.tmpdir)

    if options.jobs < 1:
        parser.error('--jobs must be positive')
    if options.interactive and options.debug:
        parser.error("-i/--interactive and -d/--debug are incompatible")
    if options.debug:
        if options.timeout != defaults['timeout']:
            sys.stderr.write(
                'warning: --timeout option ignored with --debug\n')
        options.timeout = 0
    if options.py3k_warnings:
        if sys.version_info[:2] < (2, 6) or sys.version_info[:2] >= (3, 0):
            parser.error('--py3k-warnings can only be used on Python 2.6+')
    if options.blacklist:
        options.blacklist = parselistfiles(options.blacklist, 'blacklist')
    if options.whitelist:
        options.whitelisted = parselistfiles(options.whitelist, 'whitelist')
    else:
        options.whitelisted = {}

    return (options, args)

def rename(src, dst):
    """Like os.rename(), trade atomicity and opened files friendliness
    for existing destination support.
    """
    shutil.copy(src, dst)
    os.remove(src)

_unified_diff = difflib.unified_diff
if sys.version_info[0] > 2:
    import functools
    _unified_diff = functools.partial(difflib.diff_bytes, difflib.unified_diff)

def getdiff(expected, output, ref, err):
    servefail = False
    lines = []
    for line in _unified_diff(expected, output, ref, err):
        if line.startswith(b'+++') or line.startswith(b'---'):
            line = line.replace(b'\\', b'/')
            if line.endswith(b' \n'):
                line = line[:-2] + b'\n'
        lines.append(line)
        if not servefail and line.startswith(
                             b'+  abort: child process failed to start'):
            servefail = True

    return servefail, lines

verbose = False
def vlog(*msg):
    """Log only when in verbose mode."""
    if verbose is False:
        return

    return log(*msg)

# Bytes that break XML even in a CDATA block: control characters 0-31
# sans \t, \n and \r
CDATA_EVIL = re.compile(br"[\000-\010\013\014\016-\037]")

def cdatasafe(data):
    """Make a string safe to include in a CDATA block.

    Certain control characters are illegal in a CDATA block, and
    there's no way to include a ]]> in a CDATA either. This function
    replaces illegal bytes with ? and adds a space between the ]] so
    that it won't break the CDATA block.
    """
    return CDATA_EVIL.sub(b'?', data).replace(b']]>', b'] ]>')

def log(*msg):
    """Log something to stdout.

    Arguments are strings to print.
    """
    with iolock:
        if verbose:
            print(verbose, end=' ')
        for m in msg:
            print(m, end=' ')
        print()
        sys.stdout.flush()

def terminate(proc):
    """Terminate subprocess (with fallback for Python versions < 2.6)"""
    vlog('# Terminating process %d' % proc.pid)
    try:
        getattr(proc, 'terminate', lambda : os.kill(proc.pid, signal.SIGTERM))()
    except OSError:
        pass

def killdaemons(pidfile):
    return killmod.killdaemons(pidfile, tryhard=False, remove=True,
                               logfn=vlog)

class Test(unittest.TestCase):
    """Encapsulates a single, runnable test.

    While this class conforms to the unittest.TestCase API, it differs in that
    instances need to be instantiated manually. (Typically, unittest.TestCase
    classes are instantiated automatically by scanning modules.)
    """

    # Status code reserved for skipped tests (used by hghave).
    SKIPPED_STATUS = 80

    def __init__(self, path, tmpdir, keeptmpdir=False,
                 debug=False,
                 timeout=defaults['timeout'],
                 startport=defaults['port'], extraconfigopts=None,
                 py3kwarnings=False, shell=None):
        """Create a test from parameters.

        path is the full path to the file defining the test.

        tmpdir is the main temporary directory to use for this test.

        keeptmpdir determines whether to keep the test's temporary directory
        after execution. It defaults to removal (False).

        debug mode will make the test execute verbosely, with unfiltered
        output.

        timeout controls the maximum run time of the test. It is ignored when
        debug is True.

        startport controls the starting port number to use for this test. Each
        test will reserve 3 port numbers for execution. It is the caller's
        responsibility to allocate a non-overlapping port range to Test
        instances.

        extraconfigopts is an iterable of extra hgrc config options. Values
        must have the form "key=value" (something understood by hgrc). Values
        of the form "foo.key=value" will result in "[foo] key=value".

        py3kwarnings enables Py3k warnings.

        shell is the shell to execute tests in.
        """
        self.path = path
        self.bname = os.path.basename(path)
        self.name = self.bname.decode('utf-8')
        self._testdir = os.path.dirname(path)
        self.errpath = os.path.join(self._testdir, b'%s.err' % self.bname)

        self._threadtmp = tmpdir
        self._keeptmpdir = keeptmpdir
        self._debug = debug
        self._timeout = timeout
        self._startport = startport
        self._extraconfigopts = extraconfigopts or []
        self._py3kwarnings = py3kwarnings
        self._shell = shell.encode('utf-8')

        self._aborted = False
        self._daemonpids = []
        self._finished = None
        self._ret = None
        self._out = None
        self._skipped = None
        self._testtmp = None

        # If we're not in --debug mode and reference output file exists,
        # check test output against it.
        if debug:
            self._refout = None # to match "out is None"
        elif os.path.exists(self.refpath):
            f = open(self.refpath, 'rb')
            self._refout = f.read().splitlines(True)
            f.close()
        else:
            self._refout = []

    # needed to get base class __repr__ running
    @property
    def _testMethodName(self):
        return self.name

    def __str__(self):
        return self.name

    def shortDescription(self):
        return self.name

    def setUp(self):
        """Tasks to perform before run()."""
        self._finished = False
        self._ret = None
        self._out = None
        self._skipped = None

        try:
            os.mkdir(self._threadtmp)
        except OSError as e:
            if e.errno != errno.EEXIST:
                raise

        self._testtmp = os.path.join(self._threadtmp,
                                     os.path.basename(self.path))
        os.mkdir(self._testtmp)

        # Remove any previous output files.
        if os.path.exists(self.errpath):
            try:
                os.remove(self.errpath)
            except OSError as e:
                # We might have raced another test to clean up a .err
                # file, so ignore ENOENT when removing a previous .err
                # file.
                if e.errno != errno.ENOENT:
                    raise

    def run(self, result):
        """Run this test and report results against a TestResult instance."""
        # This function is extremely similar to unittest.TestCase.run(). Once
        # we require Python 2.7 (or at least its version of unittest), this
        # function can largely go away.
        self._result = result
        result.startTest(self)
        try:
            try:
                self.setUp()
            except (KeyboardInterrupt, SystemExit):
                self._aborted = True
                raise
            except Exception:
                result.addError(self, sys.exc_info())
                return

            success = False
            try:
                self.runTest()
            except KeyboardInterrupt:
                self._aborted = True
                raise
            except SkipTest as e:
                result.addSkip(self, str(e))
                # The base class will have already counted this as a
                # test we "ran", but we want to exclude skipped tests
                # from those we count towards those run.
                result.testsRun -= 1
            except IgnoreTest as e:
                result.addIgnore(self, str(e))
                # As with skips, ignores also should be excluded from
                # the number of tests executed.
                result.testsRun -= 1
            except WarnTest as e:
                result.addWarn(self, str(e))
            except self.failureException as e:
                # This differs from unittest in that we don't capture
                # the stack trace. This is for historical reasons and
                # this decision could be revisited in the future,
                # especially for PythonTest instances.
                if result.addFailure(self, str(e)):
                    success = True
            except Exception:
                result.addError(self, sys.exc_info())
            else:
                success = True

            try:
                self.tearDown()
            except (KeyboardInterrupt, SystemExit):
                self._aborted = True
                raise
            except Exception:
                result.addError(self, sys.exc_info())
                success = False

            if success:
                result.addSuccess(self)
        finally:
            result.stopTest(self, interrupted=self._aborted)

    def runTest(self):
        """Run this test instance.

        This will return a tuple describing the result of the test.
        """
        env = self._getenv()
        self._daemonpids.append(env['DAEMON_PIDS'])
        self._createhgrc(env['HGRCPATH'])

        vlog('# Test', self.name)

        ret, out = self._run(env)
        self._finished = True
        self._ret = ret
        self._out = out

        def describe(ret):
            if ret < 0:
                return 'killed by signal: %d' % -ret
            return 'returned error code %d' % ret

        self._skipped = False

        if ret == self.SKIPPED_STATUS:
            if out is None: # Debug mode, nothing to parse.
                missing = ['unknown']
                failed = None
            else:
                missing, failed = TTest.parsehghaveoutput(out)

            if not missing:
                missing = ['skipped']

            if failed:
                self.fail('hg have failed checking for %s' % failed[-1])
            else:
                self._skipped = True
                raise SkipTest(missing[-1])
        elif ret == 'timeout':
            self.fail('timed out')
        elif ret is False:
            raise WarnTest('no result code from test')
        elif out != self._refout:
            # Diff generation may rely on written .err file.
            if (ret != 0 or out != self._refout) and not self._skipped \
                and not self._debug:
                f = open(self.errpath, 'wb')
                for line in out:
                    f.write(line)
                f.close()

            # The result object handles diff calculation for us.
            if self._result.addOutputMismatch(self, ret, out, self._refout):
                # change was accepted, skip failing
                return

            if ret:
                msg = 'output changed and ' + describe(ret)
            else:
                msg = 'output changed'

            self.fail(msg)
        elif ret:
            self.fail(describe(ret))

    def tearDown(self):
        """Tasks to perform after run()."""
        for entry in self._daemonpids:
            killdaemons(entry)
        self._daemonpids = []

        if not self._keeptmpdir:
            shutil.rmtree(self._testtmp, True)
            shutil.rmtree(self._threadtmp, True)

        if (self._ret != 0 or self._out != self._refout) and not self._skipped \
            and not self._debug and self._out:
            f = open(self.errpath, 'wb')
            for line in self._out:
                f.write(line)
            f.close()

        vlog("# Ret was:", self._ret, '(%s)' % self.name)

    def _run(self, env):
        # This should be implemented in child classes to run tests.
        raise SkipTest('unknown test type')

    def abort(self):
        """Terminate execution of this test."""
        self._aborted = True

    def _getreplacements(self):
        """Obtain a mapping of text replacements to apply to test output.

        Test output needs to be normalized so it can be compared to expected
        output. This function defines how some of that normalization will
        occur.
        """
        r = [
            (br':%d\b' % self._startport, b':$HGPORT'),
            (br':%d\b' % (self._startport + 1), b':$HGPORT1'),
            (br':%d\b' % (self._startport + 2), b':$HGPORT2'),
            (br'(?m)^(saved backup bundle to .*\.hg)( \(glob\))?$',
             br'\1 (glob)'),
            ]

        if os.name == 'nt':
            r.append(
                (b''.join(c.isalpha() and b'[%s%s]' % (c.lower(), c.upper()) or
                    c in b'/\\' and br'[/\\]' or c.isdigit() and c or b'\\' + c
                    for c in self._testtmp), b'$TESTTMP'))
        else:
            r.append((re.escape(self._testtmp), b'$TESTTMP'))

        return r

    def _getenv(self):
        """Obtain environment variables to use during test execution."""
        env = os.environ.copy()
        env['TESTTMP'] = self._testtmp
        env['HOME'] = self._testtmp
        env["HGPORT"] = str(self._startport)
        env["HGPORT1"] = str(self._startport + 1)
        env["HGPORT2"] = str(self._startport + 2)
        env["HGRCPATH"] = os.path.join(self._threadtmp, b'.hgrc')
        env["DAEMON_PIDS"] = os.path.join(self._threadtmp, b'daemon.pids')
        env["HGEDITOR"] = ('"' + sys.executable + '"'
                           + ' -c "import sys; sys.exit(0)"')
        env["HGMERGE"] = "internal:merge"
        env["HGUSER"]   = "test"
        env["HGENCODING"] = "ascii"
        env["HGENCODINGMODE"] = "strict"

        # Reset some environment variables to well-known values so that
        # the tests produce repeatable output.
        env['LANG'] = env['LC_ALL'] = env['LANGUAGE'] = 'C'
        env['TZ'] = 'GMT'
        env["EMAIL"] = "Foo Bar <foo.bar@example.com>"
        env['COLUMNS'] = '80'
        env['TERM'] = 'xterm'

        for k in ('HG HGPROF CDPATH GREP_OPTIONS http_proxy no_proxy ' +
                  'NO_PROXY').split():
            if k in env:
                del env[k]

        # unset env related to hooks
        for k in env.keys():
            if k.startswith('HG_'):
                del env[k]

        return env

    def _createhgrc(self, path):
        """Create an hgrc file for this test."""
        hgrc = open(path, 'wb')
        hgrc.write(b'[ui]\n')
        hgrc.write(b'slash = True\n')
        hgrc.write(b'interactive = False\n')
        hgrc.write(b'mergemarkers = detailed\n')
        hgrc.write(b'promptecho = True\n')
        hgrc.write(b'[defaults]\n')
        hgrc.write(b'backout = -d "0 0"\n')
        hgrc.write(b'commit = -d "0 0"\n')
        hgrc.write(b'shelve = --date "0 0"\n')
        hgrc.write(b'tag = -d "0 0"\n')
        hgrc.write(b'[devel]\n')
        hgrc.write(b'all = true\n')
        hgrc.write(b'[largefiles]\n')
        hgrc.write(b'usercache = %s\n' %
                   (os.path.join(self._testtmp, b'.cache/largefiles')))

        for opt in self._extraconfigopts:
            section, key = opt.split('.', 1)
            assert '=' in key, ('extra config opt %s must '
                                'have an = for assignment' % opt)
            hgrc.write(b'[%s]\n%s\n' % (section, key))
        hgrc.close()

    def fail(self, msg):
        # unittest differentiates between errored and failed.
        # Failed is denoted by AssertionError (by default at least).
        raise AssertionError(msg)

    def _runcommand(self, cmd, env, normalizenewlines=False):
        """Run command in a sub-process, capturing the output (stdout and
        stderr).

        Return a tuple (exitcode, output). output is None in debug mode.
        """
        if self._debug:
            proc = subprocess.Popen(cmd, shell=True, cwd=self._testtmp,
                                    env=env)
            ret = proc.wait()
            return (ret, None)

        proc = Popen4(cmd, self._testtmp, self._timeout, env)
        def cleanup():
            terminate(proc)
            ret = proc.wait()
            if ret == 0:
                ret = signal.SIGTERM << 8
            killdaemons(env['DAEMON_PIDS'])
            return ret

        output = ''
        proc.tochild.close()

        try:
            output = proc.fromchild.read()
        except KeyboardInterrupt:
            vlog('# Handling keyboard interrupt')
            cleanup()
            raise

        ret = proc.wait()
        if os.WIFEXITED(ret):
            ret = os.WEXITSTATUS(ret)

        if proc.timeout:
            ret = 'timeout'

        if ret:
            killdaemons(env['DAEMON_PIDS'])

        for s, r in self._getreplacements():
            output = re.sub(s, r, output)

        if normalizenewlines:
            output = output.replace('\r\n', '\n')

        return ret, output.splitlines(True)

class PythonTest(Test):
    """A Python-based test."""

    @property
    def refpath(self):
        return os.path.join(self._testdir, b'%s.out' % self.bname)

    def _run(self, env):
        py3kswitch = self._py3kwarnings and b' -3' or b''
        cmd = b'%s%s "%s"' % (PYTHON, py3kswitch, self.path)
        vlog("# Running", cmd)
        normalizenewlines = os.name == 'nt'
        result = self._runcommand(cmd, env,
                                  normalizenewlines=normalizenewlines)
        if self._aborted:
            raise KeyboardInterrupt()

        return result

# This script may want to drop globs from lines matching these patterns on
# Windows, but check-code.py wants a glob on these lines unconditionally.  Don't
# warn if that is the case for anything matching these lines.
checkcodeglobpats = [
    re.compile(br'^pushing to \$TESTTMP/.*[^)]$'),
    re.compile(br'^moving \S+/.*[^)]$'),
    re.compile(br'^pulling from \$TESTTMP/.*[^)]$')
]

bchr = chr
if sys.version_info[0] == 3:
    bchr = lambda x: bytes([x])

class TTest(Test):
    """A "t test" is a test backed by a .t file."""

    SKIPPED_PREFIX = 'skipped: '
    FAILED_PREFIX = 'hghave check failed: '
    NEEDESCAPE = re.compile(br'[\x00-\x08\x0b-\x1f\x7f-\xff]').search

    ESCAPESUB = re.compile(br'[\x00-\x08\x0b-\x1f\\\x7f-\xff]').sub
    ESCAPEMAP = dict((bchr(i), br'\x%02x' % i) for i in range(256))
    ESCAPEMAP.update({b'\\': b'\\\\', b'\r': br'\r'})

    @property
    def refpath(self):
        return os.path.join(self._testdir, self.bname)

    def _run(self, env):
        f = open(self.path, 'rb')
        lines = f.readlines()
        f.close()

        salt, script, after, expected = self._parsetest(lines)

        # Write out the generated script.
        fname = b'%s.sh' % self._testtmp
        f = open(fname, 'wb')
        for l in script:
            f.write(l)
        f.close()

        cmd = b'%s "%s"' % (self._shell, fname)
        vlog("# Running", cmd)

        exitcode, output = self._runcommand(cmd, env)

        if self._aborted:
            raise KeyboardInterrupt()

        # Do not merge output if skipped. Return hghave message instead.
        # Similarly, with --debug, output is None.
        if exitcode == self.SKIPPED_STATUS or output is None:
            return exitcode, output

        return self._processoutput(exitcode, output, salt, after, expected)

    def _hghave(self, reqs):
        # TODO do something smarter when all other uses of hghave are gone.
        tdir = self._testdir.replace(b'\\', b'/')
        proc = Popen4(b'%s -c "%s/hghave %s"' %
                      (self._shell, tdir, b' '.join(reqs)),
                      self._testtmp, 0, self._getenv())
        stdout, stderr = proc.communicate()
        ret = proc.wait()
        if os.WIFEXITED(ret):
            ret = os.WEXITSTATUS(ret)
        if ret == 2:
            print(stdout)
            sys.exit(1)

        return ret == 0

    def _parsetest(self, lines):
        # We generate a shell script which outputs unique markers to line
        # up script results with our source. These markers include input
        # line number and the last return code.
        salt = b"SALT%d" % time.time()
        def addsalt(line, inpython):
            if inpython:
                script.append(b'%s %d 0\n' % (salt, line))
            else:
                script.append(b'echo %s %d $?\n' % (salt, line))

        script = []

        # After we run the shell script, we re-unify the script output
        # with non-active parts of the source, with synchronization by our
        # SALT line number markers. The after table contains the non-active
        # components, ordered by line number.
        after = {}

        # Expected shell script output.
        expected = {}

        pos = prepos = -1

        # True or False when in a true or false conditional section
        skipping = None

        # We keep track of whether or not we're in a Python block so we
        # can generate the surrounding doctest magic.
        inpython = False

        if self._debug:
            script.append(b'set -x\n')
        if os.getenv('MSYSTEM'):
            script.append(b'alias pwd="pwd -W"\n')

        for n, l in enumerate(lines):
            if not l.endswith(b'\n'):
                l += b'\n'
            if l.startswith(b'#require'):
                lsplit = l.split()
                if len(lsplit) < 2 or lsplit[0] != b'#require':
                    after.setdefault(pos, []).append('  !!! invalid #require\n')
                if not self._hghave(lsplit[1:]):
                    script = [b"exit 80\n"]
                    break
                after.setdefault(pos, []).append(l)
            elif l.startswith(b'#if'):
                lsplit = l.split()
                if len(lsplit) < 2 or lsplit[0] != b'#if':
                    after.setdefault(pos, []).append('  !!! invalid #if\n')
                if skipping is not None:
                    after.setdefault(pos, []).append('  !!! nested #if\n')
                skipping = not self._hghave(lsplit[1:])
                after.setdefault(pos, []).append(l)
            elif l.startswith(b'#else'):
                if skipping is None:
                    after.setdefault(pos, []).append('  !!! missing #if\n')
                skipping = not skipping
                after.setdefault(pos, []).append(l)
            elif l.startswith(b'#endif'):
                if skipping is None:
                    after.setdefault(pos, []).append('  !!! missing #if\n')
                skipping = None
                after.setdefault(pos, []).append(l)
            elif skipping:
                after.setdefault(pos, []).append(l)
            elif l.startswith(b'  >>> '): # python inlines
                after.setdefault(pos, []).append(l)
                prepos = pos
                pos = n
                if not inpython:
                    # We've just entered a Python block. Add the header.
                    inpython = True
                    addsalt(prepos, False) # Make sure we report the exit code.
                    script.append(b'%s -m heredoctest <<EOF\n' % PYTHON)
                addsalt(n, True)
                script.append(l[2:])
            elif l.startswith(b'  ... '): # python inlines
                after.setdefault(prepos, []).append(l)
                script.append(l[2:])
            elif l.startswith(b'  $ '): # commands
                if inpython:
                    script.append(b'EOF\n')
                    inpython = False
                after.setdefault(pos, []).append(l)
                prepos = pos
                pos = n
                addsalt(n, False)
                cmd = l[4:].split()
                if len(cmd) == 2 and cmd[0] == b'cd':
                    l = b'  $ cd %s || exit 1\n' % cmd[1]
                script.append(l[4:])
            elif l.startswith(b'  > '): # continuations
                after.setdefault(prepos, []).append(l)
                script.append(l[4:])
            elif l.startswith(b'  '): # results
                # Queue up a list of expected results.
                expected.setdefault(pos, []).append(l[2:])
            else:
                if inpython:
                    script.append(b'EOF\n')
                    inpython = False
                # Non-command/result. Queue up for merged output.
                after.setdefault(pos, []).append(l)

        if inpython:
            script.append(b'EOF\n')
        if skipping is not None:
            after.setdefault(pos, []).append('  !!! missing #endif\n')
        addsalt(n + 1, False)

        return salt, script, after, expected

    def _processoutput(self, exitcode, output, salt, after, expected):
        # Merge the script output back into a unified test.
        warnonly = 1 # 1: not yet; 2: yes; 3: for sure not
        if exitcode != 0:
            warnonly = 3

        pos = -1
        postout = []
        for l in output:
            lout, lcmd = l, None
            if salt in l:
                lout, lcmd = l.split(salt, 1)

            if lout:
                if not lout.endswith(b'\n'):
                    lout += b' (no-eol)\n'

                # Find the expected output at the current position.
                el = None
                if expected.get(pos, None):
                    el = expected[pos].pop(0)

                r = TTest.linematch(el, lout)
                if isinstance(r, str):
                    if r == '+glob':
                        lout = el[:-1] + ' (glob)\n'
                        r = '' # Warn only this line.
                    elif r == '-glob':
                        lout = ''.join(el.rsplit(' (glob)', 1))
                        r = '' # Warn only this line.
                    else:
                        log('\ninfo, unknown linematch result: %r\n' % r)
                        r = False
                if r:
                    postout.append(b'  ' + el)
                else:
                    if self.NEEDESCAPE(lout):
                        lout = TTest._stringescape(b'%s (esc)\n' %
                                                   lout.rstrip(b'\n'))
                    postout.append(b'  ' + lout) # Let diff deal with it.
                    if r != '': # If line failed.
                        warnonly = 3 # for sure not
                    elif warnonly == 1: # Is "not yet" and line is warn only.
                        warnonly = 2 # Yes do warn.

            if lcmd:
                # Add on last return code.
                ret = int(lcmd.split()[1])
                if ret != 0:
                    postout.append(b'  [%d]\n' % ret)
                if pos in after:
                    # Merge in non-active test bits.
                    postout += after.pop(pos)
                pos = int(lcmd.split()[0])

        if pos in after:
            postout += after.pop(pos)

        if warnonly == 2:
            exitcode = False # Set exitcode to warned.

        return exitcode, postout

    @staticmethod
    def rematch(el, l):
        try:
            # use \Z to ensure that the regex matches to the end of the string
            if os.name == 'nt':
                return re.match(el + br'\r?\n\Z', l)
            return re.match(el + br'\n\Z', l)
        except re.error:
            # el is an invalid regex
            return False

    @staticmethod
    def globmatch(el, l):
        # The only supported special characters are * and ? plus / which also
        # matches \ on windows. Escaping of these characters is supported.
        if el + b'\n' == l:
            if os.altsep:
                # matching on "/" is not needed for this line
                for pat in checkcodeglobpats:
                    if pat.match(el):
                        return True
                return b'-glob'
            return True
        i, n = 0, len(el)
        res = b''
        while i < n:
            c = el[i:i + 1]
            i += 1
            if c == b'\\' and i < n and el[i:i + 1] in b'*?\\/':
                res += el[i - 1:i + 1]
                i += 1
            elif c == b'*':
                res += b'.*'
            elif c == b'?':
                res += b'.'
            elif c == b'/' and os.altsep:
                res += b'[/\\\\]'
            else:
                res += re.escape(c)
        return TTest.rematch(res, l)

    @staticmethod
    def linematch(el, l):
        if el == l: # perfect match (fast)
            return True
        if el:
            if el.endswith(b" (esc)\n"):
                if sys.version_info[0] == 3:
                    el = el[:-7].decode('unicode_escape') + '\n'
                    el = el.encode('utf-8')
                else:
                    el = el[:-7].decode('string-escape') + '\n'
            if el == l or os.name == 'nt' and el[:-1] + b'\r\n' == l:
                return True
            if el.endswith(b" (re)\n"):
                return TTest.rematch(el[:-6], l)
            if el.endswith(b" (glob)\n"):
                # ignore '(glob)' added to l by 'replacements'
                if l.endswith(b" (glob)\n"):
                    l = l[:-8] + b"\n"
                return TTest.globmatch(el[:-8], l)
            if os.altsep and l.replace(b'\\', b'/') == el:
                return b'+glob'
        return False

    @staticmethod
    def parsehghaveoutput(lines):
        '''Parse hghave log lines.

        Return tuple of lists (missing, failed):
          * the missing/unknown features
          * the features for which existence check failed'''
        missing = []
        failed = []
        for line in lines:
            if line.startswith(TTest.SKIPPED_PREFIX):
                line = line.splitlines()[0]
                missing.append(line[len(TTest.SKIPPED_PREFIX):])
            elif line.startswith(TTest.FAILED_PREFIX):
                line = line.splitlines()[0]
                failed.append(line[len(TTest.FAILED_PREFIX):])

        return missing, failed

    @staticmethod
    def _escapef(m):
        return TTest.ESCAPEMAP[m.group(0)]

    @staticmethod
    def _stringescape(s):
        return TTest.ESCAPESUB(TTest._escapef, s)

iolock = threading.RLock()

class SkipTest(Exception):
    """Raised to indicate that a test is to be skipped."""

class IgnoreTest(Exception):
    """Raised to indicate that a test is to be ignored."""

class WarnTest(Exception):
    """Raised to indicate that a test warned."""

class TestResult(unittest._TextTestResult):
    """Holds results when executing via unittest."""
    # Don't worry too much about accessing the non-public _TextTestResult.
    # It is relatively common in Python testing tools.
    def __init__(self, options, *args, **kwargs):
        super(TestResult, self).__init__(*args, **kwargs)

        self._options = options

        # unittest.TestResult didn't have skipped until 2.7. We need to
        # polyfill it.
        self.skipped = []

        # We have a custom "ignored" result that isn't present in any Python
        # unittest implementation. It is very similar to skipped. It may make
        # sense to map it into skip some day.
        self.ignored = []

        # We have a custom "warned" result that isn't present in any Python
        # unittest implementation. It is very similar to failed. It may make
        # sense to map it into fail some day.
        self.warned = []

        self.times = []
        self._firststarttime =  None
        # Data stored for the benefit of generating xunit reports.
        self.successes = []
        self.faildata = {}

    def addFailure(self, test, reason):
        self.failures.append((test, reason))

        if self._options.first:
            self.stop()
        else:
            with iolock:
                if not self._options.nodiff:
                    self.stream.write('\nERROR: %s output changed\n' % test)

                self.stream.write('!')
                self.stream.flush()

    def addSuccess(self, test):
        with iolock:
            super(TestResult, self).addSuccess(test)
        self.successes.append(test)

    def addError(self, test, err):
        super(TestResult, self).addError(test, err)
        if self._options.first:
            self.stop()

    # Polyfill.
    def addSkip(self, test, reason):
        self.skipped.append((test, reason))
        with iolock:
            if self.showAll:
                self.stream.writeln('skipped %s' % reason)
            else:
                self.stream.write('s')
                self.stream.flush()

    def addIgnore(self, test, reason):
        self.ignored.append((test, reason))
        with iolock:
            if self.showAll:
                self.stream.writeln('ignored %s' % reason)
            else:
                if reason not in ('not retesting', "doesn't match keyword"):
                    self.stream.write('i')
                else:
                    self.testsRun += 1
                self.stream.flush()

    def addWarn(self, test, reason):
        self.warned.append((test, reason))

        if self._options.first:
            self.stop()

        with iolock:
            if self.showAll:
                self.stream.writeln('warned %s' % reason)
            else:
                self.stream.write('~')
                self.stream.flush()

    def addOutputMismatch(self, test, ret, got, expected):
        """Record a mismatch in test output for a particular test."""
        if self.shouldStop:
            # don't print, some other test case already failed and
            # printed, we're just stale and probably failed due to our
            # temp dir getting cleaned up.
            return

        accepted = False
        failed = False
        lines = []

        with iolock:
            if self._options.nodiff:
                pass
            elif self._options.view:
                v = self._options.view
                if sys.version_info[0] == 3:
                    v = v.encode('utf-8')
                os.system(b"%s %s %s" %
                          (v, test.refpath, test.errpath))
            else:
                servefail, lines = getdiff(expected, got,
                                           test.refpath, test.errpath)
                if servefail:
                    self.addFailure(
                        test,
                        'server failed to start (HGPORT=%s)' % test._startport)
                else:
                    self.stream.write('\n')
                    for line in lines:
                        if sys.version_info[0] > 2:
                            self.stream.flush()
                            self.stream.buffer.write(line)
                            self.stream.buffer.flush()
                        else:
                            self.stream.write(line)
                            self.stream.flush()

            # handle interactive prompt without releasing iolock
            if self._options.interactive:
                self.stream.write('Accept this change? [n] ')
                answer = sys.stdin.readline().strip()
                if answer.lower() in ('y', 'yes'):
                    if test.name.endswith('.t'):
                        rename(test.errpath, test.path)
                    else:
                        rename(test.errpath, '%s.out' % test.path)
                    accepted = True
            if not accepted and not failed:
                self.faildata[test.name] = b''.join(lines)

        return accepted

    def startTest(self, test):
        super(TestResult, self).startTest(test)

        # os.times module computes the user time and system time spent by
        # child's processes along with real elapsed time taken by a process.
        # This module has one limitation. It can only work for Linux user
        # and not for Windows.
        test.started = os.times()
        if self._firststarttime is None: # thread racy but irrelevant
            self._firststarttime = test.started[4]

    def stopTest(self, test, interrupted=False):
        super(TestResult, self).stopTest(test)

        test.stopped = os.times()

        starttime = test.started
        endtime = test.stopped
        origin = self._firststarttime
        self.times.append((test.name,
                           endtime[2] - starttime[2], # user space CPU time
                           endtime[3] - starttime[3], # sys  space CPU time
                           endtime[4] - starttime[4], # real time
                           starttime[4] - origin, # start date in run context
                           endtime[4] - origin, # end date in run context
                           ))

        if interrupted:
            with iolock:
                self.stream.writeln('INTERRUPTED: %s (after %d seconds)' % (
                    test.name, self.times[-1][3]))

class TestSuite(unittest.TestSuite):
    """Custom unittest TestSuite that knows how to execute Mercurial tests."""

    def __init__(self, testdir, jobs=1, whitelist=None, blacklist=None,
                 retest=False, keywords=None, loop=False, runs_per_test=1,
                 loadtest=None,
                 *args, **kwargs):
        """Create a new instance that can run tests with a configuration.

        testdir specifies the directory where tests are executed from. This
        is typically the ``tests`` directory from Mercurial's source
        repository.

        jobs specifies the number of jobs to run concurrently. Each test
        executes on its own thread. Tests actually spawn new processes, so
        state mutation should not be an issue.

        whitelist and blacklist denote tests that have been whitelisted and
        blacklisted, respectively. These arguments don't belong in TestSuite.
        Instead, whitelist and blacklist should be handled by the thing that
        populates the TestSuite with tests. They are present to preserve
        backwards compatible behavior which reports skipped tests as part
        of the results.

        retest denotes whether to retest failed tests. This arguably belongs
        outside of TestSuite.

        keywords denotes key words that will be used to filter which tests
        to execute. This arguably belongs outside of TestSuite.

        loop denotes whether to loop over tests forever.
        """
        super(TestSuite, self).__init__(*args, **kwargs)

        self._jobs = jobs
        self._whitelist = whitelist
        self._blacklist = blacklist
        self._retest = retest
        self._keywords = keywords
        self._loop = loop
        self._runs_per_test = runs_per_test
        self._loadtest = loadtest

    def run(self, result):
        # We have a number of filters that need to be applied. We do this
        # here instead of inside Test because it makes the running logic for
        # Test simpler.
        tests = []
        num_tests = [0]
        for test in self._tests:
            def get():
                num_tests[0] += 1
                if getattr(test, 'should_reload', False):
                    return self._loadtest(test.bname, num_tests[0])
                return test
            if not os.path.exists(test.path):
                result.addSkip(test, "Doesn't exist")
                continue

            if not (self._whitelist and test.name in self._whitelist):
                if self._blacklist and test.bname in self._blacklist:
                    result.addSkip(test, 'blacklisted')
                    continue

                if self._retest and not os.path.exists(test.errpath):
                    result.addIgnore(test, 'not retesting')
                    continue

                if self._keywords:
                    f = open(test.path, 'rb')
                    t = f.read().lower() + test.bname.lower()
                    f.close()
                    ignored = False
                    for k in self._keywords.lower().split():
                        if k not in t:
                            result.addIgnore(test, "doesn't match keyword")
                            ignored = True
                            break

                    if ignored:
                        continue
            for _ in xrange(self._runs_per_test):
                tests.append(get())

        runtests = list(tests)
        done = queue.Queue()
        running = 0

        def job(test, result):
            try:
                test(result)
                done.put(None)
            except KeyboardInterrupt:
                pass
            except: # re-raises
                done.put(('!', test, 'run-test raised an error, see traceback'))
                raise

        stoppedearly = False

        try:
            while tests or running:
                if not done.empty() or running == self._jobs or not tests:
                    try:
                        done.get(True, 1)
                        running -= 1
                        if result and result.shouldStop:
                            stoppedearly = True
                            break
                    except queue.Empty:
                        continue
                if tests and not running == self._jobs:
                    test = tests.pop(0)
                    if self._loop:
                        if getattr(test, 'should_reload', False):
                            num_tests[0] += 1
                            tests.append(
                                self._loadtest(test.name, num_tests[0]))
                        else:
                            tests.append(test)
                    t = threading.Thread(target=job, name=test.name,
                                         args=(test, result))
                    t.start()
                    running += 1

            # If we stop early we still need to wait on started tests to
            # finish. Otherwise, there is a race between the test completing
            # and the test's cleanup code running. This could result in the
            # test reporting incorrect.
            if stoppedearly:
                while running:
                    try:
                        done.get(True, 1)
                        running -= 1
                    except queue.Empty:
                        continue
        except KeyboardInterrupt:
            for test in runtests:
                test.abort()

        return result

class TextTestRunner(unittest.TextTestRunner):
    """Custom unittest test runner that uses appropriate settings."""

    def __init__(self, runner, *args, **kwargs):
        super(TextTestRunner, self).__init__(*args, **kwargs)

        self._runner = runner

    def run(self, test):
        result = TestResult(self._runner.options, self.stream,
                            self.descriptions, self.verbosity)

        test(result)

        failed = len(result.failures)
        warned = len(result.warned)
        skipped = len(result.skipped)
        ignored = len(result.ignored)

        with iolock:
            self.stream.writeln('')

            if not self._runner.options.noskips:
                for test, msg in result.skipped:
                    self.stream.writeln('Skipped %s: %s' % (test.name, msg))
            for test, msg in result.warned:
                self.stream.writeln('Warned %s: %s' % (test.name, msg))
            for test, msg in result.failures:
                self.stream.writeln('Failed %s: %s' % (test.name, msg))
            for test, msg in result.errors:
                self.stream.writeln('Errored %s: %s' % (test.name, msg))

            if self._runner.options.xunit:
                xuf = open(self._runner.options.xunit, 'wb')
                try:
                    timesd = dict((t[0], t[3]) for t in result.times)
                    doc = minidom.Document()
                    s = doc.createElement('testsuite')
                    s.setAttribute('name', 'run-tests')
                    s.setAttribute('tests', str(result.testsRun))
                    s.setAttribute('errors', "0") # TODO
                    s.setAttribute('failures', str(failed))
                    s.setAttribute('skipped', str(skipped + ignored))
                    doc.appendChild(s)
                    for tc in result.successes:
                        t = doc.createElement('testcase')
                        t.setAttribute('name', tc.name)
                        t.setAttribute('time', '%.3f' % timesd[tc.name])
                        s.appendChild(t)
                    for tc, err in sorted(result.faildata.items()):
                        t = doc.createElement('testcase')
                        t.setAttribute('name', tc)
                        t.setAttribute('time', '%.3f' % timesd[tc])
                        # createCDATASection expects a unicode or it will
                        # convert using default conversion rules, which will
                        # fail if string isn't ASCII.
                        err = cdatasafe(err).decode('utf-8', 'replace')
                        cd = doc.createCDATASection(err)
                        t.appendChild(cd)
                        s.appendChild(t)
                    xuf.write(doc.toprettyxml(indent='  ', encoding='utf-8'))
                finally:
                    xuf.close()

            if self._runner.options.json:
                if json is None:
                    raise ImportError("json module not installed")
                jsonpath = os.path.join(self._runner._testdir, 'report.json')
                fp = open(jsonpath, 'w')
                try:
                    timesd = {}
                    for tdata in result.times:
                        test = tdata[0]
                        timesd[test] = tdata[1:]

                    outcome = {}
                    groups = [('success', ((tc, None)
                               for tc in result.successes)),
                              ('failure', result.failures),
                              ('skip', result.skipped)]
                    for res, testcases in groups:
                        for tc, __ in testcases:
                            tres = {'result': res,
                                    'time': ('%0.3f' % timesd[tc.name][2]),
                                    'cuser': ('%0.3f' % timesd[tc.name][0]),
                                    'csys': ('%0.3f' % timesd[tc.name][1]),
                                    'start': ('%0.3f' % timesd[tc.name][3]),
                                    'end': ('%0.3f' % timesd[tc.name][4])}
                            outcome[tc.name] = tres
                    jsonout = json.dumps(outcome, sort_keys=True, indent=4)
                    fp.writelines(("testreport =", jsonout))
                finally:
                    fp.close()

            self._runner._checkhglib('Tested')

            self.stream.writeln(
                '# Ran %d tests, %d skipped, %d warned, %d failed.'
                % (result.testsRun,
                   skipped + ignored, warned, failed))
            if failed:
                self.stream.writeln('python hash seed: %s' %
                    os.environ['PYTHONHASHSEED'])
            if self._runner.options.time:
                self.printtimes(result.times)

        return result

    def printtimes(self, times):
        # iolock held by run
        self.stream.writeln('# Producing time report')
        times.sort(key=lambda t: (t[3]))
        cols = '%7.3f %7.3f %7.3f %7.3f %7.3f   %s'
        self.stream.writeln('%-7s %-7s %-7s %-7s %-7s   %s' %
                            ('start', 'end', 'cuser', 'csys', 'real', 'Test'))
        for tdata in times:
            test = tdata[0]
            cuser, csys, real, start, end = tdata[1:6]
            self.stream.writeln(cols % (start, end, cuser, csys, real, test))

class TestRunner(object):
    """Holds context for executing tests.

    Tests rely on a lot of state. This object holds it for them.
    """

    # Programs required to run tests.
    REQUIREDTOOLS = [
        os.path.basename(sys.executable).encode('utf-8'),
        b'diff',
        b'grep',
        b'unzip',
        b'gunzip',
        b'bunzip2',
        b'sed',
    ]

    # Maps file extensions to test class.
    TESTTYPES = [
        (b'.py', PythonTest),
        (b'.t', TTest),
    ]

    def __init__(self):
        self.options = None
        self._hgroot = None
        self._testdir = None
        self._hgtmp = None
        self._installdir = None
        self._bindir = None
        self._tmpbinddir = None
        self._pythondir = None
        self._coveragefile = None
        self._createdfiles = []
        self._hgpath = None
        self._portoffset = 0
        self._ports = {}

    def run(self, args, parser=None):
        """Run the test suite."""
        oldmask = os.umask(0o22)
        try:
            parser = parser or getparser()
            options, args = parseargs(args, parser)
            args = [a.encode('utf-8') for a in args]
            self.options = options

            self._checktools()
            tests = self.findtests(args)
            if options.profile_runner:
                import statprof
                statprof.start()
            result = self._run(tests)
            if options.profile_runner:
                statprof.stop()
                statprof.display()
            return result

        finally:
            os.umask(oldmask)

    def _run(self, tests):
        if self.options.random:
            random.shuffle(tests)
        else:
            # keywords for slow tests
            slow = {b'svn': 10,
                    b'gendoc': 10,
                    b'check-code-hg': 100,
                   }
            def sortkey(f):
                # run largest tests first, as they tend to take the longest
                try:
                    val = -os.stat(f).st_size
                except OSError as e:
                    if e.errno != errno.ENOENT:
                        raise
                    return -1e9 # file does not exist, tell early
                for kw, mul in slow.iteritems():
                    if kw in f:
                        val *= mul
                return val
            tests.sort(key=sortkey)

        self._testdir = osenvironb[b'TESTDIR'] = getattr(
            os, 'getcwdb', os.getcwd)()

        if 'PYTHONHASHSEED' not in os.environ:
            # use a random python hash seed all the time
            # we do the randomness ourself to know what seed is used
            os.environ['PYTHONHASHSEED'] = str(random.getrandbits(32))

        if self.options.tmpdir:
            self.options.keep_tmpdir = True
            tmpdir = self.options.tmpdir.encode('utf-8')
            if os.path.exists(tmpdir):
                # Meaning of tmpdir has changed since 1.3: we used to create
                # HGTMP inside tmpdir; now HGTMP is tmpdir.  So fail if
                # tmpdir already exists.
                print("error: temp dir %r already exists" % tmpdir)
                return 1

                # Automatically removing tmpdir sounds convenient, but could
                # really annoy anyone in the habit of using "--tmpdir=/tmp"
                # or "--tmpdir=$HOME".
                #vlog("# Removing temp dir", tmpdir)
                #shutil.rmtree(tmpdir)
            os.makedirs(tmpdir)
        else:
            d = None
            if os.name == 'nt':
                # without this, we get the default temp dir location, but
                # in all lowercase, which causes troubles with paths (issue3490)
                d = osenvironb.get(b'TMP', None)
            # FILE BUG: mkdtemp works only on unicode in Python 3
            tmpdir = tempfile.mkdtemp('', 'hgtests.',
                                      d and d.decode('utf-8')).encode('utf-8')

        self._hgtmp = osenvironb[b'HGTMP'] = (
            os.path.realpath(tmpdir))

        if self.options.with_hg:
            self._installdir = None
            whg = self.options.with_hg
            # If --with-hg is not specified, we have bytes already,
            # but if it was specified in python3 we get a str, so we
            # have to encode it back into a bytes.
            if sys.version_info[0] == 3:
                if not isinstance(whg, bytes):
                    whg = whg.encode('utf-8')
            self._bindir = os.path.dirname(os.path.realpath(whg))
            assert isinstance(self._bindir, bytes)
            self._tmpbindir = os.path.join(self._hgtmp, b'install', b'bin')
            os.makedirs(self._tmpbindir)

            # This looks redundant with how Python initializes sys.path from
            # the location of the script being executed.  Needed because the
            # "hg" specified by --with-hg is not the only Python script
            # executed in the test suite that needs to import 'mercurial'
            # ... which means it's not really redundant at all.
            self._pythondir = self._bindir
        else:
            self._installdir = os.path.join(self._hgtmp, b"install")
            self._bindir = osenvironb[b"BINDIR"] = \
                os.path.join(self._installdir, b"bin")
            self._tmpbindir = self._bindir
            self._pythondir = os.path.join(self._installdir, b"lib", b"python")

        osenvironb[b"BINDIR"] = self._bindir
        osenvironb[b"PYTHON"] = PYTHON

        fileb = __file__.encode('utf-8')
        runtestdir = os.path.abspath(os.path.dirname(fileb))
        if sys.version_info[0] == 3:
            sepb = os.pathsep.encode('utf-8')
        else:
            sepb = os.pathsep
        path = [self._bindir, runtestdir] + osenvironb[b"PATH"].split(sepb)
        if os.path.islink(__file__):
            # test helper will likely be at the end of the symlink
            realfile = os.path.realpath(fileb)
            realdir = os.path.abspath(os.path.dirname(realfile))
            path.insert(2, realdir)
        if self._tmpbindir != self._bindir:
            path = [self._tmpbindir] + path
        osenvironb[b"PATH"] = sepb.join(path)

        # Include TESTDIR in PYTHONPATH so that out-of-tree extensions
        # can run .../tests/run-tests.py test-foo where test-foo
        # adds an extension to HGRC. Also include run-test.py directory to
        # import modules like heredoctest.
        pypath = [self._pythondir, self._testdir, runtestdir]
        # We have to augment PYTHONPATH, rather than simply replacing
        # it, in case external libraries are only available via current
        # PYTHONPATH.  (In particular, the Subversion bindings on OS X
        # are in /opt/subversion.)
        oldpypath = osenvironb.get(IMPL_PATH)
        if oldpypath:
            pypath.append(oldpypath)
        osenvironb[IMPL_PATH] = sepb.join(pypath)

        if self.options.pure:
            os.environ["HGTEST_RUN_TESTS_PURE"] = "--pure"

        self._coveragefile = os.path.join(self._testdir, b'.coverage')

        vlog("# Using TESTDIR", self._testdir)
        vlog("# Using HGTMP", self._hgtmp)
        vlog("# Using PATH", os.environ["PATH"])
        vlog("# Using", IMPL_PATH, osenvironb[IMPL_PATH])

        try:
            return self._runtests(tests) or 0
        finally:
            time.sleep(.1)
            self._cleanup()

    def findtests(self, args):
        """Finds possible test files from arguments.

        If you wish to inject custom tests into the test harness, this would
        be a good function to monkeypatch or override in a derived class.
        """
        if not args:
            if self.options.changed:
                proc = Popen4('hg st --rev "%s" -man0 .' %
                              self.options.changed, None, 0)
                stdout, stderr = proc.communicate()
                args = stdout.strip(b'\0').split(b'\0')
            else:
                args = os.listdir(b'.')

        return [t for t in args
                if os.path.basename(t).startswith(b'test-')
                    and (t.endswith(b'.py') or t.endswith(b'.t'))]

    def _runtests(self, tests):
        try:
            if self._installdir:
                self._installhg()
                self._checkhglib("Testing")
            else:
                self._usecorrectpython()

            if self.options.restart:
                orig = list(tests)
                while tests:
                    if os.path.exists(tests[0] + ".err"):
                        break
                    tests.pop(0)
                if not tests:
                    print("running all tests")
                    tests = orig

            tests = [self._gettest(t, i) for i, t in enumerate(tests)]

            failed = False
            warned = False
            kws = self.options.keywords
            if kws is not None and sys.version_info[0] == 3:
                kws = kws.encode('utf-8')

            suite = TestSuite(self._testdir,
                              jobs=self.options.jobs,
                              whitelist=self.options.whitelisted,
                              blacklist=self.options.blacklist,
                              retest=self.options.retest,
                              keywords=kws,
                              loop=self.options.loop,
                              runs_per_test=self.options.runs_per_test,
                              tests=tests, loadtest=self._gettest)
            verbosity = 1
            if self.options.verbose:
                verbosity = 2
            runner = TextTestRunner(self, verbosity=verbosity)
            result = runner.run(suite)

            if result.failures:
                failed = True
            if result.warned:
                warned = True

            if self.options.anycoverage:
                self._outputcoverage()
        except KeyboardInterrupt:
            failed = True
            print("\ninterrupted!")

        if failed:
            return 1
        if warned:
            return 80

    def _getport(self, count):
        port = self._ports.get(count) # do we have a cached entry?
        if port is None:
            port = self.options.port + self._portoffset
            portneeded = 3
            # above 100 tries we just give up and let test reports failure
            for tries in xrange(100):
                allfree = True
                for idx in xrange(portneeded):
                    if not checkportisavailable(port + idx):
                        allfree = False
                        break
                self._portoffset += portneeded
                if allfree:
                    break
            self._ports[count] = port
        return port

    def _gettest(self, test, count):
        """Obtain a Test by looking at its filename.

        Returns a Test instance. The Test may not be runnable if it doesn't
        map to a known type.
        """
        lctest = test.lower()
        testcls = Test

        for ext, cls in self.TESTTYPES:
            if lctest.endswith(ext):
                testcls = cls
                break

        refpath = os.path.join(self._testdir, test)
        tmpdir = os.path.join(self._hgtmp, b'child%d' % count)

        t = testcls(refpath, tmpdir,
                    keeptmpdir=self.options.keep_tmpdir,
                    debug=self.options.debug,
                    timeout=self.options.timeout,
                    startport=self._getport(count),
                    extraconfigopts=self.options.extra_config_opt,
                    py3kwarnings=self.options.py3k_warnings,
                    shell=self.options.shell)
        t.should_reload = True
        return t

    def _cleanup(self):
        """Clean up state from this test invocation."""

        if self.options.keep_tmpdir:
            return

        vlog("# Cleaning up HGTMP", self._hgtmp)
        shutil.rmtree(self._hgtmp, True)
        for f in self._createdfiles:
            try:
                os.remove(f)
            except OSError:
                pass

    def _usecorrectpython(self):
        """Configure the environment to use the appropriate Python in tests."""
        # Tests must use the same interpreter as us or bad things will happen.
        pyexename = sys.platform == 'win32' and b'python.exe' or b'python'
        if getattr(os, 'symlink', None):
            vlog("# Making python executable in test path a symlink to '%s'" %
                 sys.executable)
            mypython = os.path.join(self._tmpbindir, pyexename)
            try:
                if os.readlink(mypython) == sys.executable:
                    return
                os.unlink(mypython)
            except OSError as err:
                if err.errno != errno.ENOENT:
                    raise
            if self._findprogram(pyexename) != sys.executable:
                try:
                    os.symlink(sys.executable, mypython)
                    self._createdfiles.append(mypython)
                except OSError as err:
                    # child processes may race, which is harmless
                    if err.errno != errno.EEXIST:
                        raise
        else:
            exedir, exename = os.path.split(sys.executable)
            vlog("# Modifying search path to find %s as %s in '%s'" %
                 (exename, pyexename, exedir))
            path = os.environ['PATH'].split(os.pathsep)
            while exedir in path:
                path.remove(exedir)
            os.environ['PATH'] = os.pathsep.join([exedir] + path)
            if not self._findprogram(pyexename):
                print("WARNING: Cannot find %s in search path" % pyexename)

    def _installhg(self):
        """Install hg into the test environment.

        This will also configure hg with the appropriate testing settings.
        """
        vlog("# Performing temporary installation of HG")
        installerrs = os.path.join(b"tests", b"install.err")
        compiler = ''
        if self.options.compiler:
            compiler = '--compiler ' + self.options.compiler
        if self.options.pure:
            pure = b"--pure"
        else:
            pure = b""
        py3 = ''

        # Run installer in hg root
        script = os.path.realpath(sys.argv[0])
        exe = sys.executable
        if sys.version_info[0] == 3:
            py3 = b'--c2to3'
            compiler = compiler.encode('utf-8')
            script = script.encode('utf-8')
            exe = exe.encode('utf-8')
        hgroot = os.path.dirname(os.path.dirname(script))
        self._hgroot = hgroot
        os.chdir(hgroot)
        nohome = b'--home=""'
        if os.name == 'nt':
            # The --home="" trick works only on OS where os.sep == '/'
            # because of a distutils convert_path() fast-path. Avoid it at
            # least on Windows for now, deal with .pydistutils.cfg bugs
            # when they happen.
            nohome = b''
        cmd = (b'%(exe)s setup.py %(py3)s %(pure)s clean --all'
               b' build %(compiler)s --build-base="%(base)s"'
               b' install --force --prefix="%(prefix)s"'
               b' --install-lib="%(libdir)s"'
               b' --install-scripts="%(bindir)s" %(nohome)s >%(logfile)s 2>&1'
               % {b'exe': exe, b'py3': py3, b'pure': pure,
                  b'compiler': compiler,
                  b'base': os.path.join(self._hgtmp, b"build"),
                  b'prefix': self._installdir, b'libdir': self._pythondir,
                  b'bindir': self._bindir,
                  b'nohome': nohome, b'logfile': installerrs})

        # setuptools requires install directories to exist.
        def makedirs(p):
            try:
                os.makedirs(p)
            except OSError as e:
                if e.errno != errno.EEXIST:
                    raise
        makedirs(self._pythondir)
        makedirs(self._bindir)

        vlog("# Running", cmd)
        if os.system(cmd) == 0:
            if not self.options.verbose:
                os.remove(installerrs)
        else:
            f = open(installerrs, 'rb')
            for line in f:
                if sys.version_info[0] > 2:
                    sys.stdout.buffer.write(line)
                else:
                    sys.stdout.write(line)
            f.close()
            sys.exit(1)
        os.chdir(self._testdir)

        self._usecorrectpython()

        if self.options.py3k_warnings and not self.options.anycoverage:
            vlog("# Updating hg command to enable Py3k Warnings switch")
            f = open(os.path.join(self._bindir, 'hg'), 'rb')
            lines = [line.rstrip() for line in f]
            lines[0] += ' -3'
            f.close()
            f = open(os.path.join(self._bindir, 'hg'), 'wb')
            for line in lines:
                f.write(line + '\n')
            f.close()

        hgbat = os.path.join(self._bindir, b'hg.bat')
        if os.path.isfile(hgbat):
            # hg.bat expects to be put in bin/scripts while run-tests.py
            # installation layout put it in bin/ directly. Fix it
            f = open(hgbat, 'rb')
            data = f.read()
            f.close()
            if b'"%~dp0..\python" "%~dp0hg" %*' in data:
                data = data.replace(b'"%~dp0..\python" "%~dp0hg" %*',
                                    b'"%~dp0python" "%~dp0hg" %*')
                f = open(hgbat, 'wb')
                f.write(data)
                f.close()
            else:
                print('WARNING: cannot fix hg.bat reference to python.exe')

        if self.options.anycoverage:
            custom = os.path.join(self._testdir, 'sitecustomize.py')
            target = os.path.join(self._pythondir, 'sitecustomize.py')
            vlog('# Installing coverage trigger to %s' % target)
            shutil.copyfile(custom, target)
            rc = os.path.join(self._testdir, '.coveragerc')
            vlog('# Installing coverage rc to %s' % rc)
            os.environ['COVERAGE_PROCESS_START'] = rc
            covdir = os.path.join(self._installdir, '..', 'coverage')
            try:
                os.mkdir(covdir)
            except OSError as e:
                if e.errno != errno.EEXIST:
                    raise

            os.environ['COVERAGE_DIR'] = covdir

    def _checkhglib(self, verb):
        """Ensure that the 'mercurial' package imported by python is
        the one we expect it to be.  If not, print a warning to stderr."""
        if ((self._bindir == self._pythondir) and
            (self._bindir != self._tmpbindir)):
            # The pythondir has been inferred from --with-hg flag.
            # We cannot expect anything sensible here.
            return
        expecthg = os.path.join(self._pythondir, b'mercurial')
        actualhg = self._gethgpath()
        if os.path.abspath(actualhg) != os.path.abspath(expecthg):
            sys.stderr.write('warning: %s with unexpected mercurial lib: %s\n'
                             '         (expected %s)\n'
                             % (verb, actualhg, expecthg))
    def _gethgpath(self):
        """Return the path to the mercurial package that is actually found by
        the current Python interpreter."""
        if self._hgpath is not None:
            return self._hgpath

        cmd = b'%s -c "import mercurial; print (mercurial.__path__[0])"'
        cmd = cmd % PYTHON
        if sys.version_info[0] > 2:
            cmd = cmd.decode('utf-8')
        pipe = os.popen(cmd)
        try:
            self._hgpath = pipe.read().strip()
            if sys.version_info[0] == 3:
                self._hgpath = self._hgpath.encode('utf-8')
        finally:
            pipe.close()

        return self._hgpath

    def _outputcoverage(self):
        """Produce code coverage output."""
        from coverage import coverage

        vlog('# Producing coverage report')
        # chdir is the easiest way to get short, relative paths in the
        # output.
        os.chdir(self._hgroot)
        covdir = os.path.join(self._installdir, '..', 'coverage')
        cov = coverage(data_file=os.path.join(covdir, 'cov'))

        # Map install directory paths back to source directory.
        cov.config.paths['srcdir'] = ['.', self._pythondir]

        cov.combine()

        omit = [os.path.join(x, '*') for x in [self._bindir, self._testdir]]
        cov.report(ignore_errors=True, omit=omit)

        if self.options.htmlcov:
            htmldir = os.path.join(self._testdir, 'htmlcov')
            cov.html_report(directory=htmldir, omit=omit)
        if self.options.annotate:
            adir = os.path.join(self._testdir, 'annotated')
            if not os.path.isdir(adir):
                os.mkdir(adir)
            cov.annotate(directory=adir, omit=omit)

    def _findprogram(self, program):
        """Search PATH for a executable program"""
        if sys.version_info[0] > 2:
            dpb = os.defpath.encode('utf-8')
            sepb = os.pathsep.encode('utf-8')
        else:
            dpb = os.defpath
            sepb = os.pathsep
        for p in osenvironb.get(b'PATH', dpb).split(sepb):
            name = os.path.join(p, program)
            if os.name == 'nt' or os.access(name, os.X_OK):
                return name
        return None

    def _checktools(self):
        """Ensure tools required to run tests are present."""
        for p in self.REQUIREDTOOLS:
            if os.name == 'nt' and not p.endswith('.exe'):
                p += '.exe'
            found = self._findprogram(p)
            if found:
                vlog("# Found prerequisite", p, "at", found)
            else:
                print("WARNING: Did not find prerequisite tool: %s " % p)

if __name__ == '__main__':
    runner = TestRunner()

    try:
        import msvcrt
        msvcrt.setmode(sys.stdin.fileno(), os.O_BINARY)
        msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
        msvcrt.setmode(sys.stderr.fileno(), os.O_BINARY)
    except ImportError:
        pass

    sys.exit(runner.run(sys.argv[1:]))
