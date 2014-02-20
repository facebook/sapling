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

from distutils import version
import difflib
import errno
import optparse
import os
import shutil
import subprocess
import signal
import sys
import tempfile
import time
import random
import re
import threading
import killdaemons as killmod
import Queue as queue

processlock = threading.Lock()

# subprocess._cleanup can race with any Popen.wait or Popen.poll on py24
# http://bugs.python.org/issue1731717 for details. We shouldn't be producing
# zombies but it's pretty harmless even if we do.
if sys.version_info < (2, 5):
    subprocess._cleanup = lambda: None

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

# reserved exit code to skip test (used by hghave)
SKIPPED_STATUS = 80
SKIPPED_PREFIX = 'skipped: '
FAILED_PREFIX  = 'hghave check failed: '
PYTHON = sys.executable.replace('\\', '/')
IMPL_PATH = 'PYTHONPATH'
if 'java' in sys.platform:
    IMPL_PATH = 'JYTHONPATH'

requiredtools = [os.path.basename(sys.executable), "diff", "grep", "unzip",
                 "gunzip", "bunzip2", "sed"]
createdfiles = []

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
            f = open(path, "r")
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
            if warn:
                print "warning: no such %s file: %s" % (listtype, filename)
            continue

        for line in f.readlines():
            line = line.split('#', 1)[0].strip()
            if line:
                entries[line] = filename

        f.close()
    return entries

def parseargs():
    parser = optparse.OptionParser("%prog [options] [tests]")

    # keep these sorted
    parser.add_option("--blacklist", action="append",
        help="skip tests listed in the specified blacklist file")
    parser.add_option("--whitelist", action="append",
        help="always run tests listed in the specified whitelist file")
    parser.add_option("-C", "--annotate", action="store_true",
        help="output files annotated with coverage")
    parser.add_option("-c", "--cover", action="store_true",
        help="print a test coverage report")
    parser.add_option("-d", "--debug", action="store_true",
        help="debug mode: write output of test scripts to console"
             " rather than capturing and diff'ing it (disables timeout)")
    parser.add_option("-f", "--first", action="store_true",
        help="exit on the first test failure")
    parser.add_option("-H", "--htmlcov", action="store_true",
        help="create an HTML report of the coverage of the files")
    parser.add_option("--inotify", action="store_true",
        help="enable inotify extension when running tests")
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
    parser.add_option("--tmpdir", type="string",
        help="run tests in the given temporary directory"
             " (implies --keep-tmpdir)")
    parser.add_option("-v", "--verbose", action="store_true",
        help="output verbose messages")
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

    for option, (envvar, default) in defaults.items():
        defaults[option] = type(default)(os.environ.get(envvar, default))
    parser.set_defaults(**defaults)
    (options, args) = parser.parse_args()

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
        testdir = os.path.dirname(os.path.realpath(sys.argv[0]))
        hgbin = os.path.join(os.path.dirname(testdir), 'hg')
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

def parsehghaveoutput(lines):
    '''Parse hghave log lines.
    Return tuple of lists (missing, failed):
      * the missing/unknown features
      * the features for which existence check failed'''
    missing = []
    failed = []
    for line in lines:
        if line.startswith(SKIPPED_PREFIX):
            line = line.splitlines()[0]
            missing.append(line[len(SKIPPED_PREFIX):])
        elif line.startswith(FAILED_PREFIX):
            line = line.splitlines()[0]
            failed.append(line[len(FAILED_PREFIX):])

    return missing, failed

def showdiff(expected, output, ref, err):
    print
    for line in difflib.unified_diff(expected, output, ref, err):
        sys.stdout.write(line)

verbose = False
def vlog(*msg):
    if verbose is not False:
        iolock.acquire()
        if verbose:
            print verbose,
        for m in msg:
            print m,
        print
        sys.stdout.flush()
        iolock.release()

def log(*msg):
    iolock.acquire()
    if verbose:
        print verbose,
    for m in msg:
        print m,
    print
    sys.stdout.flush()
    iolock.release()

def findprogram(program):
    """Search PATH for a executable program"""
    for p in os.environ.get('PATH', os.defpath).split(os.pathsep):
        name = os.path.join(p, program)
        if os.name == 'nt' or os.access(name, os.X_OK):
            return name
    return None

def createhgrc(path, options):
    # create a fresh hgrc
    hgrc = open(path, 'w')
    hgrc.write('[ui]\n')
    hgrc.write('slash = True\n')
    hgrc.write('interactive = False\n')
    hgrc.write('[defaults]\n')
    hgrc.write('backout = -d "0 0"\n')
    hgrc.write('commit = -d "0 0"\n')
    hgrc.write('shelve = --date "0 0"\n')
    hgrc.write('tag = -d "0 0"\n')
    if options.inotify:
        hgrc.write('[extensions]\n')
        hgrc.write('inotify=\n')
        hgrc.write('[inotify]\n')
        hgrc.write('pidfile=daemon.pids')
        hgrc.write('appendpid=True\n')
    if options.extra_config_opt:
        for opt in options.extra_config_opt:
            section, key = opt.split('.', 1)
            assert '=' in key, ('extra config opt %s must '
                                'have an = for assignment' % opt)
            hgrc.write('[%s]\n%s\n' % (section, key))
    hgrc.close()

def createenv(options, testtmp, threadtmp, port):
    env = os.environ.copy()
    env['TESTTMP'] = testtmp
    env['HOME'] = testtmp
    env["HGPORT"] = str(port)
    env["HGPORT1"] = str(port + 1)
    env["HGPORT2"] = str(port + 2)
    env["HGRCPATH"] = os.path.join(threadtmp, '.hgrc')
    env["DAEMON_PIDS"] = os.path.join(threadtmp, 'daemon.pids')
    env["HGEDITOR"] = sys.executable + ' -c "import sys; sys.exit(0)"'
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

def checktools():
    # Before we go any further, check for pre-requisite tools
    # stuff from coreutils (cat, rm, etc) are not tested
    for p in requiredtools:
        if os.name == 'nt' and not p.endswith('.exe'):
            p += '.exe'
        found = findprogram(p)
        if found:
            vlog("# Found prerequisite", p, "at", found)
        else:
            print "WARNING: Did not find prerequisite tool: "+p

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

def cleanup(options):
    if not options.keep_tmpdir:
        vlog("# Cleaning up HGTMP", HGTMP)
        shutil.rmtree(HGTMP, True)
        for f in createdfiles:
            try:
                os.remove(f)
            except OSError:
                pass

def usecorrectpython():
    # some tests run python interpreter. they must use same
    # interpreter we use or bad things will happen.
    pyexename = sys.platform == 'win32' and 'python.exe' or 'python'
    if getattr(os, 'symlink', None):
        vlog("# Making python executable in test path a symlink to '%s'" %
             sys.executable)
        mypython = os.path.join(TMPBINDIR, pyexename)
        try:
            if os.readlink(mypython) == sys.executable:
                return
            os.unlink(mypython)
        except OSError, err:
            if err.errno != errno.ENOENT:
                raise
        if findprogram(pyexename) != sys.executable:
            try:
                os.symlink(sys.executable, mypython)
                createdfiles.append(mypython)
            except OSError, err:
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
        if not findprogram(pyexename):
            print "WARNING: Cannot find %s in search path" % pyexename

def installhg(options):
    vlog("# Performing temporary installation of HG")
    installerrs = os.path.join("tests", "install.err")
    compiler = ''
    if options.compiler:
        compiler = '--compiler ' + options.compiler
    pure = options.pure and "--pure" or ""
    py3 = ''
    if sys.version_info[0] == 3:
        py3 = '--c2to3'

    # Run installer in hg root
    script = os.path.realpath(sys.argv[0])
    hgroot = os.path.dirname(os.path.dirname(script))
    os.chdir(hgroot)
    nohome = '--home=""'
    if os.name == 'nt':
        # The --home="" trick works only on OS where os.sep == '/'
        # because of a distutils convert_path() fast-path. Avoid it at
        # least on Windows for now, deal with .pydistutils.cfg bugs
        # when they happen.
        nohome = ''
    cmd = ('%(exe)s setup.py %(py3)s %(pure)s clean --all'
           ' build %(compiler)s --build-base="%(base)s"'
           ' install --force --prefix="%(prefix)s" --install-lib="%(libdir)s"'
           ' --install-scripts="%(bindir)s" %(nohome)s >%(logfile)s 2>&1'
           % dict(exe=sys.executable, py3=py3, pure=pure, compiler=compiler,
                  base=os.path.join(HGTMP, "build"),
                  prefix=INST, libdir=PYTHONDIR, bindir=BINDIR,
                  nohome=nohome, logfile=installerrs))
    vlog("# Running", cmd)
    if os.system(cmd) == 0:
        if not options.verbose:
            os.remove(installerrs)
    else:
        f = open(installerrs)
        for line in f:
            print line,
        f.close()
        sys.exit(1)
    os.chdir(TESTDIR)

    usecorrectpython()

    if options.py3k_warnings and not options.anycoverage:
        vlog("# Updating hg command to enable Py3k Warnings switch")
        f = open(os.path.join(BINDIR, 'hg'), 'r')
        lines = [line.rstrip() for line in f]
        lines[0] += ' -3'
        f.close()
        f = open(os.path.join(BINDIR, 'hg'), 'w')
        for line in lines:
            f.write(line + '\n')
        f.close()

    hgbat = os.path.join(BINDIR, 'hg.bat')
    if os.path.isfile(hgbat):
        # hg.bat expects to be put in bin/scripts while run-tests.py
        # installation layout put it in bin/ directly. Fix it
        f = open(hgbat, 'rb')
        data = f.read()
        f.close()
        if '"%~dp0..\python" "%~dp0hg" %*' in data:
            data = data.replace('"%~dp0..\python" "%~dp0hg" %*',
                                '"%~dp0python" "%~dp0hg" %*')
            f = open(hgbat, 'wb')
            f.write(data)
            f.close()
        else:
            print 'WARNING: cannot fix hg.bat reference to python.exe'

    if options.anycoverage:
        custom = os.path.join(TESTDIR, 'sitecustomize.py')
        target = os.path.join(PYTHONDIR, 'sitecustomize.py')
        vlog('# Installing coverage trigger to %s' % target)
        shutil.copyfile(custom, target)
        rc = os.path.join(TESTDIR, '.coveragerc')
        vlog('# Installing coverage rc to %s' % rc)
        os.environ['COVERAGE_PROCESS_START'] = rc
        fn = os.path.join(INST, '..', '.coverage')
        os.environ['COVERAGE_FILE'] = fn

def outputtimes(options):
    vlog('# Producing time report')
    times.sort(key=lambda t: (t[1], t[0]), reverse=True)
    cols = '%7.3f   %s'
    print '\n%-7s   %s' % ('Time', 'Test')
    for test, timetaken in times:
        print cols % (timetaken, test)

def outputcoverage(options):

    vlog('# Producing coverage report')
    os.chdir(PYTHONDIR)

    def covrun(*args):
        cmd = 'coverage %s' % ' '.join(args)
        vlog('# Running: %s' % cmd)
        os.system(cmd)

    covrun('-c')
    omit = ','.join(os.path.join(x, '*') for x in [BINDIR, TESTDIR])
    covrun('-i', '-r', '"--omit=%s"' % omit) # report
    if options.htmlcov:
        htmldir = os.path.join(TESTDIR, 'htmlcov')
        covrun('-i', '-b', '"--directory=%s"' % htmldir, '"--omit=%s"' % omit)
    if options.annotate:
        adir = os.path.join(TESTDIR, 'annotated')
        if not os.path.isdir(adir):
            os.mkdir(adir)
        covrun('-i', '-a', '"--directory=%s"' % adir, '"--omit=%s"' % omit)

def pytest(test, wd, options, replacements, env):
    py3kswitch = options.py3k_warnings and ' -3' or ''
    cmd = '%s%s "%s"' % (PYTHON, py3kswitch, test)
    vlog("# Running", cmd)
    if os.name == 'nt':
        replacements.append((r'\r\n', '\n'))
    return run(cmd, wd, options, replacements, env)

needescape = re.compile(r'[\x00-\x08\x0b-\x1f\x7f-\xff]').search
escapesub = re.compile(r'[\x00-\x08\x0b-\x1f\\\x7f-\xff]').sub
escapemap = dict((chr(i), r'\x%02x' % i) for i in range(256))
escapemap.update({'\\': '\\\\', '\r': r'\r'})
def escapef(m):
    return escapemap[m.group(0)]
def stringescape(s):
    return escapesub(escapef, s)

def rematch(el, l):
    try:
        # use \Z to ensure that the regex matches to the end of the string
        if os.name == 'nt':
            return re.match(el + r'\r?\n\Z', l)
        return re.match(el + r'\n\Z', l)
    except re.error:
        # el is an invalid regex
        return False

def globmatch(el, l):
    # The only supported special characters are * and ? plus / which also
    # matches \ on windows. Escaping of these caracters is supported.
    if el + '\n' == l:
        if os.altsep:
            # matching on "/" is not needed for this line
            return '-glob'
        return True
    i, n = 0, len(el)
    res = ''
    while i < n:
        c = el[i]
        i += 1
        if c == '\\' and el[i] in '*?\\/':
            res += el[i - 1:i + 1]
            i += 1
        elif c == '*':
            res += '.*'
        elif c == '?':
            res += '.'
        elif c == '/' and os.altsep:
            res += '[/\\\\]'
        else:
            res += re.escape(c)
    return rematch(res, l)

def linematch(el, l):
    if el == l: # perfect match (fast)
        return True
    if el:
        if el.endswith(" (esc)\n"):
            el = el[:-7].decode('string-escape') + '\n'
        if el == l or os.name == 'nt' and el[:-1] + '\r\n' == l:
            return True
        if el.endswith(" (re)\n"):
            return rematch(el[:-6], l)
        if el.endswith(" (glob)\n"):
            return globmatch(el[:-8], l)
        if os.altsep and l.replace('\\', '/') == el:
            return '+glob'
    return False

def tsttest(test, wd, options, replacements, env):
    # We generate a shell script which outputs unique markers to line
    # up script results with our source. These markers include input
    # line number and the last return code
    salt = "SALT" + str(time.time())
    def addsalt(line, inpython):
        if inpython:
            script.append('%s %d 0\n' % (salt, line))
        else:
            script.append('echo %s %s $?\n' % (salt, line))

    # After we run the shell script, we re-unify the script output
    # with non-active parts of the source, with synchronization by our
    # SALT line number markers. The after table contains the
    # non-active components, ordered by line number
    after = {}
    pos = prepos = -1

    # Expected shellscript output
    expected = {}

    # We keep track of whether or not we're in a Python block so we
    # can generate the surrounding doctest magic
    inpython = False

    # True or False when in a true or false conditional section
    skipping = None

    def hghave(reqs):
        # TODO: do something smarter when all other uses of hghave is gone
        tdir = TESTDIR.replace('\\', '/')
        proc = Popen4('%s -c "%s/hghave %s"' %
                      (options.shell, tdir, ' '.join(reqs)), wd, 0)
        stdout, stderr = proc.communicate()
        ret = proc.wait()
        if wifexited(ret):
            ret = os.WEXITSTATUS(ret)
        if ret == 2:
            print stdout
            sys.exit(1)
        return ret == 0

    f = open(test)
    t = f.readlines()
    f.close()

    script = []
    if options.debug:
        script.append('set -x\n')
    if os.getenv('MSYSTEM'):
        script.append('alias pwd="pwd -W"\n')
    n = 0
    for n, l in enumerate(t):
        if not l.endswith('\n'):
            l += '\n'
        if l.startswith('#if'):
            if skipping is not None:
                after.setdefault(pos, []).append('  !!! nested #if\n')
            skipping = not hghave(l.split()[1:])
            after.setdefault(pos, []).append(l)
        elif l.startswith('#else'):
            if skipping is None:
                after.setdefault(pos, []).append('  !!! missing #if\n')
            skipping = not skipping
            after.setdefault(pos, []).append(l)
        elif l.startswith('#endif'):
            if skipping is None:
                after.setdefault(pos, []).append('  !!! missing #if\n')
            skipping = None
            after.setdefault(pos, []).append(l)
        elif skipping:
            after.setdefault(pos, []).append(l)
        elif l.startswith('  >>> '): # python inlines
            after.setdefault(pos, []).append(l)
            prepos = pos
            pos = n
            if not inpython:
                # we've just entered a Python block, add the header
                inpython = True
                addsalt(prepos, False) # make sure we report the exit code
                script.append('%s -m heredoctest <<EOF\n' % PYTHON)
            addsalt(n, True)
            script.append(l[2:])
        elif l.startswith('  ... '): # python inlines
            after.setdefault(prepos, []).append(l)
            script.append(l[2:])
        elif l.startswith('  $ '): # commands
            if inpython:
                script.append("EOF\n")
                inpython = False
            after.setdefault(pos, []).append(l)
            prepos = pos
            pos = n
            addsalt(n, False)
            cmd = l[4:].split()
            if len(cmd) == 2 and cmd[0] == 'cd':
                l = '  $ cd %s || exit 1\n' % cmd[1]
            script.append(l[4:])
        elif l.startswith('  > '): # continuations
            after.setdefault(prepos, []).append(l)
            script.append(l[4:])
        elif l.startswith('  '): # results
            # queue up a list of expected results
            expected.setdefault(pos, []).append(l[2:])
        else:
            if inpython:
                script.append("EOF\n")
                inpython = False
            # non-command/result - queue up for merged output
            after.setdefault(pos, []).append(l)

    if inpython:
        script.append("EOF\n")
    if skipping is not None:
        after.setdefault(pos, []).append('  !!! missing #endif\n')
    addsalt(n + 1, False)

    # Write out the script and execute it
    name = wd + '.sh'
    f = open(name, 'w')
    for l in script:
        f.write(l)
    f.close()

    cmd = '%s "%s"' % (options.shell, name)
    vlog("# Running", cmd)
    exitcode, output = run(cmd, wd, options, replacements, env)
    # do not merge output if skipped, return hghave message instead
    # similarly, with --debug, output is None
    if exitcode == SKIPPED_STATUS or output is None:
        return exitcode, output

    # Merge the script output back into a unified test

    warnonly = True
    pos = -1
    postout = []
    for l in output:
        lout, lcmd = l, None
        if salt in l:
            lout, lcmd = l.split(salt, 1)

        if lout:
            if not lout.endswith('\n'):
                lout += ' (no-eol)\n'

            # find the expected output at the current position
            el = None
            if pos in expected and expected[pos]:
                el = expected[pos].pop(0)

            r = linematch(el, lout)
            if isinstance(r, str):
                if r == '+glob':
                    lout = el[:-1] + ' (glob)\n'
                    r = 0 # warn only
                elif r == '-glob':
                    lout = ''.join(el.rsplit(' (glob)', 1))
                    r = 0 # warn only
                else:
                    log('\ninfo, unknown linematch result: %r\n' % r)
                    r = False
            if r:
                postout.append("  " + el)
            else:
                if needescape(lout):
                    lout = stringescape(lout.rstrip('\n')) + " (esc)\n"
                postout.append("  " + lout) # let diff deal with it
                if r != 0: # != warn only
                    warnonly = False

        if lcmd:
            # add on last return code
            ret = int(lcmd.split()[1])
            if ret != 0:
                postout.append("  [%s]\n" % ret)
            if pos in after:
                # merge in non-active test bits
                postout += after.pop(pos)
            pos = int(lcmd.split()[0])

    if pos in after:
        postout += after.pop(pos)

    if warnonly and exitcode == 0:
        exitcode = False
    return exitcode, postout

wifexited = getattr(os, "WIFEXITED", lambda x: False)
def run(cmd, wd, options, replacements, env):
    """Run command in a sub-process, capturing the output (stdout and stderr).
    Return a tuple (exitcode, output).  output is None in debug mode."""
    # TODO: Use subprocess.Popen if we're running on Python 2.4
    if options.debug:
        proc = subprocess.Popen(cmd, shell=True, cwd=wd, env=env)
        ret = proc.wait()
        return (ret, None)

    proc = Popen4(cmd, wd, options.timeout, env)
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
    if wifexited(ret):
        ret = os.WEXITSTATUS(ret)

    if proc.timeout:
        ret = 'timeout'

    if ret:
        killdaemons(env['DAEMON_PIDS'])

    if abort:
        raise KeyboardInterrupt()

    for s, r in replacements:
        output = re.sub(s, r, output)
    return ret, output.splitlines(True)

def runone(options, test, count):
    '''returns a result element: (code, test, msg)'''

    def skip(msg):
        if options.verbose:
            log("\nSkipping %s: %s" % (testpath, msg))
        return 's', test, msg

    def fail(msg, ret):
        warned = ret is False
        if not options.nodiff:
            log("\n%s: %s %s" % (warned and 'Warning' or 'ERROR', test, msg))
        if (not ret and options.interactive
            and os.path.exists(testpath + ".err")):
            iolock.acquire()
            print "Accept this change? [n] ",
            answer = sys.stdin.readline().strip()
            iolock.release()
            if answer.lower() in "y yes".split():
                if test.endswith(".t"):
                    rename(testpath + ".err", testpath)
                else:
                    rename(testpath + ".err", testpath + ".out")
                return '.', test, ''
        return warned and '~' or '!', test, msg

    def success():
        return '.', test, ''

    def ignore(msg):
        return 'i', test, msg

    def describe(ret):
        if ret < 0:
            return 'killed by signal %d' % -ret
        return 'returned error code %d' % ret

    testpath = os.path.join(TESTDIR, test)
    err = os.path.join(TESTDIR, test + ".err")
    lctest = test.lower()

    if not os.path.exists(testpath):
            return skip("doesn't exist")

    if not (options.whitelisted and test in options.whitelisted):
        if options.blacklist and test in options.blacklist:
            return skip("blacklisted")

        if options.retest and not os.path.exists(test + ".err"):
            return ignore("not retesting")

        if options.keywords:
            fp = open(test)
            t = fp.read().lower() + test.lower()
            fp.close()
            for k in options.keywords.lower().split():
                if k in t:
                    break
                else:
                    return ignore("doesn't match keyword")

    if not lctest.startswith("test-"):
        return skip("not a test file")
    for ext, func, out in testtypes:
        if lctest.endswith(ext):
            runner = func
            ref = os.path.join(TESTDIR, test + out)
            break
    else:
        return skip("unknown test type")

    vlog("# Test", test)

    if os.path.exists(err):
        os.remove(err)       # Remove any previous output files

    # Make a tmp subdirectory to work in
    threadtmp = os.path.join(HGTMP, "child%d" % count)
    testtmp = os.path.join(threadtmp, os.path.basename(test))
    os.mkdir(threadtmp)
    os.mkdir(testtmp)

    port = options.port + count * 3
    replacements = [
        (r':%s\b' % port, ':$HGPORT'),
        (r':%s\b' % (port + 1), ':$HGPORT1'),
        (r':%s\b' % (port + 2), ':$HGPORT2'),
        ]
    if os.name == 'nt':
        replacements.append(
            (''.join(c.isalpha() and '[%s%s]' % (c.lower(), c.upper()) or
                     c in '/\\' and r'[/\\]' or
                     c.isdigit() and c or
                     '\\' + c
                     for c in testtmp), '$TESTTMP'))
    else:
        replacements.append((re.escape(testtmp), '$TESTTMP'))

    env = createenv(options, testtmp, threadtmp, port)
    createhgrc(env['HGRCPATH'], options)

    starttime = time.time()
    try:
        ret, out = runner(testpath, testtmp, options, replacements, env)
    except KeyboardInterrupt:
        endtime = time.time()
        log('INTERRUPTED: %s (after %d seconds)' % (test, endtime - starttime))
        raise
    endtime = time.time()
    times.append((test, endtime - starttime))
    vlog("# Ret was:", ret)

    killdaemons(env['DAEMON_PIDS'])

    skipped = (ret == SKIPPED_STATUS)

    # If we're not in --debug mode and reference output file exists,
    # check test output against it.
    if options.debug:
        refout = None                   # to match "out is None"
    elif os.path.exists(ref):
        f = open(ref, "r")
        refout = f.read().splitlines(True)
        f.close()
    else:
        refout = []

    if (ret != 0 or out != refout) and not skipped and not options.debug:
        # Save errors to a file for diagnosis
        f = open(err, "wb")
        for line in out:
            f.write(line)
        f.close()

    if skipped:
        if out is None:                 # debug mode: nothing to parse
            missing = ['unknown']
            failed = None
        else:
            missing, failed = parsehghaveoutput(out)
        if not missing:
            missing = ['irrelevant']
        if failed:
            result = fail("hghave failed checking for %s" % failed[-1], ret)
            skipped = False
        else:
            result = skip(missing[-1])
    elif ret == 'timeout':
        result = fail("timed out", ret)
    elif out != refout:
        if not options.nodiff:
            iolock.acquire()
            if options.view:
                os.system("%s %s %s" % (options.view, ref, err))
            else:
                showdiff(refout, out, ref, err)
            iolock.release()
        if ret:
            result = fail("output changed and " + describe(ret), ret)
        else:
            result = fail("output changed", ret)
    elif ret:
        result = fail(describe(ret), ret)
    else:
        result = success()

    if not options.verbose:
        iolock.acquire()
        sys.stdout.write(result[0])
        sys.stdout.flush()
        iolock.release()

    if not options.keep_tmpdir:
        shutil.rmtree(threadtmp, True)
    return result

_hgpath = None

def _gethgpath():
    """Return the path to the mercurial package that is actually found by
    the current Python interpreter."""
    global _hgpath
    if _hgpath is not None:
        return _hgpath

    cmd = '%s -c "import mercurial; print (mercurial.__path__[0])"'
    pipe = os.popen(cmd % PYTHON)
    try:
        _hgpath = pipe.read().strip()
    finally:
        pipe.close()
    return _hgpath

def _checkhglib(verb):
    """Ensure that the 'mercurial' package imported by python is
    the one we expect it to be.  If not, print a warning to stderr."""
    expecthg = os.path.join(PYTHONDIR, 'mercurial')
    actualhg = _gethgpath()
    if os.path.abspath(actualhg) != os.path.abspath(expecthg):
        sys.stderr.write('warning: %s with unexpected mercurial lib: %s\n'
                         '         (expected %s)\n'
                         % (verb, actualhg, expecthg))

results = {'.':[], '!':[], '~': [], 's':[], 'i':[]}
times = []
iolock = threading.Lock()
abort = False

def scheduletests(options, tests):
    jobs = options.jobs
    done = queue.Queue()
    running = 0
    count = 0
    global abort

    def job(test, count):
        try:
            done.put(runone(options, test, count))
        except KeyboardInterrupt:
            pass
        except: # re-raises
            done.put(('!', test, 'run-test raised an error, see traceback'))
            raise

    try:
        while tests or running:
            if not done.empty() or running == jobs or not tests:
                try:
                    code, test, msg = done.get(True, 1)
                    results[code].append((test, msg))
                    if options.first and code not in '.si':
                        break
                except queue.Empty:
                    continue
                running -= 1
            if tests and not running == jobs:
                test = tests.pop(0)
                if options.loop:
                    tests.append(test)
                t = threading.Thread(target=job, name=test, args=(test, count))
                t.start()
                running += 1
                count += 1
    except KeyboardInterrupt:
        abort = True

def runtests(options, tests):
    try:
        if INST:
            installhg(options)
            _checkhglib("Testing")
        else:
            usecorrectpython()

        if options.restart:
            orig = list(tests)
            while tests:
                if os.path.exists(tests[0] + ".err"):
                    break
                tests.pop(0)
            if not tests:
                print "running all tests"
                tests = orig

        scheduletests(options, tests)

        failed = len(results['!'])
        warned = len(results['~'])
        tested = len(results['.']) + failed + warned
        skipped = len(results['s'])
        ignored = len(results['i'])

        print
        if not options.noskips:
            for s in results['s']:
                print "Skipped %s: %s" % s
        for s in results['~']:
            print "Warned %s: %s" % s
        for s in results['!']:
            print "Failed %s: %s" % s
        _checkhglib("Tested")
        print "# Ran %d tests, %d skipped, %d warned, %d failed." % (
            tested, skipped + ignored, warned, failed)
        if results['!']:
            print 'python hash seed:', os.environ['PYTHONHASHSEED']
        if options.time:
            outputtimes(options)

        if options.anycoverage:
            outputcoverage(options)
    except KeyboardInterrupt:
        failed = True
        print "\ninterrupted!"

    if failed:
        return 1
    if warned:
        return 80

testtypes = [('.py', pytest, '.out'),
             ('.t', tsttest, '')]

def main():
    (options, args) = parseargs()
    os.umask(022)

    checktools()

    if len(args) == 0:
        args = [t for t in os.listdir(".")
                if t.startswith("test-")
                and (t.endswith(".py") or t.endswith(".t"))]

    tests = args

    if options.random:
        random.shuffle(tests)
    else:
        # keywords for slow tests
        slow = 'svn gendoc check-code-hg'.split()
        def sortkey(f):
            # run largest tests first, as they tend to take the longest
            try:
                val = -os.stat(f).st_size
            except OSError, e:
                if e.errno != errno.ENOENT:
                    raise
                return -1e9 # file does not exist, tell early
            for kw in slow:
                if kw in f:
                    val *= 10
            return val
        tests.sort(key=sortkey)

    if 'PYTHONHASHSEED' not in os.environ:
        # use a random python hash seed all the time
        # we do the randomness ourself to know what seed is used
        os.environ['PYTHONHASHSEED'] = str(random.getrandbits(32))

    global TESTDIR, HGTMP, INST, BINDIR, TMPBINDIR, PYTHONDIR, COVERAGE_FILE
    TESTDIR = os.environ["TESTDIR"] = os.getcwd()
    if options.tmpdir:
        options.keep_tmpdir = True
        tmpdir = options.tmpdir
        if os.path.exists(tmpdir):
            # Meaning of tmpdir has changed since 1.3: we used to create
            # HGTMP inside tmpdir; now HGTMP is tmpdir.  So fail if
            # tmpdir already exists.
            sys.exit("error: temp dir %r already exists" % tmpdir)

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
            d = os.getenv('TMP')
        tmpdir = tempfile.mkdtemp('', 'hgtests.', d)
    HGTMP = os.environ['HGTMP'] = os.path.realpath(tmpdir)

    if options.with_hg:
        INST = None
        BINDIR = os.path.dirname(os.path.realpath(options.with_hg))
        TMPBINDIR = os.path.join(HGTMP, 'install', 'bin')
        os.makedirs(TMPBINDIR)

        # This looks redundant with how Python initializes sys.path from
        # the location of the script being executed.  Needed because the
        # "hg" specified by --with-hg is not the only Python script
        # executed in the test suite that needs to import 'mercurial'
        # ... which means it's not really redundant at all.
        PYTHONDIR = BINDIR
    else:
        INST = os.path.join(HGTMP, "install")
        BINDIR = os.environ["BINDIR"] = os.path.join(INST, "bin")
        TMPBINDIR = BINDIR
        PYTHONDIR = os.path.join(INST, "lib", "python")

    os.environ["BINDIR"] = BINDIR
    os.environ["PYTHON"] = PYTHON

    path = [BINDIR] + os.environ["PATH"].split(os.pathsep)
    if TMPBINDIR != BINDIR:
        path = [TMPBINDIR] + path
    os.environ["PATH"] = os.pathsep.join(path)

    # Include TESTDIR in PYTHONPATH so that out-of-tree extensions
    # can run .../tests/run-tests.py test-foo where test-foo
    # adds an extension to HGRC. Also include run-test.py directory to import
    # modules like heredoctest.
    pypath = [PYTHONDIR, TESTDIR, os.path.abspath(os.path.dirname(__file__))]
    # We have to augment PYTHONPATH, rather than simply replacing
    # it, in case external libraries are only available via current
    # PYTHONPATH.  (In particular, the Subversion bindings on OS X
    # are in /opt/subversion.)
    oldpypath = os.environ.get(IMPL_PATH)
    if oldpypath:
        pypath.append(oldpypath)
    os.environ[IMPL_PATH] = os.pathsep.join(pypath)

    COVERAGE_FILE = os.path.join(TESTDIR, ".coverage")

    vlog("# Using TESTDIR", TESTDIR)
    vlog("# Using HGTMP", HGTMP)
    vlog("# Using PATH", os.environ["PATH"])
    vlog("# Using", IMPL_PATH, os.environ[IMPL_PATH])

    try:
        sys.exit(runtests(options, tests) or 0)
    finally:
        time.sleep(.1)
        cleanup(options)

if __name__ == '__main__':
    main()
