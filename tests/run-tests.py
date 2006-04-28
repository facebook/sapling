#!/usr/bin/env python
#
# run-tests.py - Run a set of tests on Mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, sys, shutil, re
import tempfile
import difflib
import popen2
from optparse import OptionParser

required_tools = ["python", "diff", "grep", "unzip", "gunzip", "bunzip2", "sed"]

parser = OptionParser("%prog [options] [tests]")
parser.add_option("-v", "--verbose", action="store_true",
    help="output verbose messages")
parser.add_option("-c", "--cover", action="store_true",
    help="print a test coverage report")
parser.add_option("-s", "--cover_stdlib", action="store_true",
    help="print a test coverage report inc. standard libraries")
parser.add_option("-C", "--annotate", action="store_true",
    help="output files annotated with coverage")
(options, args) = parser.parse_args()
verbose = options.verbose
coverage = options.cover or options.cover_stdlib or options.annotate

def vlog(*msg):
    if verbose:
        for m in msg:
            print m,
        print

def show_diff(expected, output):
    for line in difflib.unified_diff(expected, output,
            "Expected output", "Test output", lineterm=''):
        print line

def find_program(program):
    """Search PATH for a executable program"""
    for p in os.environ.get('PATH', os.defpath).split(os.pathsep):
        name = os.path.join(p, program)
        if os.access(name, os.X_OK):
            return name
    return None

def check_required_tools():
    # Before we go any further, check for pre-requisite tools
    # stuff from coreutils (cat, rm, etc) are not tested
    for p in required_tools:
        if os.name == 'nt':
            p += '.exe'
        found = find_program(p)
        if found:
            vlog("# Found prerequisite", p, "at", found)
        else:
            print "WARNING: Did not find prerequisite tool: "+p

def cleanup_exit():
    if verbose:
        print "# Cleaning up HGTMP", HGTMP
    shutil.rmtree(HGTMP, True)

def install_hg():
    vlog("# Performing temporary installation of HG")
    installerrs = os.path.join("tests", "install.err")

    os.chdir("..") # Get back to hg root
    cmd = '%s setup.py install --home="%s" --install-lib="%s" >%s 2>&1' % \
        (sys.executable, INST, PYTHONDIR, installerrs)
    vlog("# Running", cmd)
    if os.system(cmd) == 0:
        if not verbose:
            os.remove(installerrs)
    else:
        f = open(installerrs)
        for line in f:
            print line,
        f.close()
        sys.exit(1)
    os.chdir(TESTDIR)

    os.environ["PATH"] = "%s%s%s" % (BINDIR, os.pathsep, os.environ["PATH"])
    os.environ["PYTHONPATH"] = PYTHONDIR

    if coverage:
        vlog("# Installing coverage wrapper")
        os.environ['COVERAGE_FILE'] = COVERAGE_FILE
        if os.path.exists(COVERAGE_FILE):
            os.unlink(COVERAGE_FILE)
        # Create a wrapper script to invoke hg via coverage.py
        os.rename(os.path.join(BINDIR, "hg"), os.path.join(BINDIR, "_hg.py")) 
        f = open(os.path.join(BINDIR, 'hg'), 'w')
        f.write('#!' + sys.executable + '\n')
        f.write('import sys, os; os.execv(sys.executable, [sys.executable, '+ \
            '"%s", "-x", "%s"] + sys.argv[1:])\n' % (
            os.path.join(TESTDIR, 'coverage.py'),
            os.path.join(BINDIR, '_hg.py')))
        f.close()
        os.chmod(os.path.join(BINDIR, 'hg'), 0700)

def output_coverage():
    if coverage:
        vlog("# Producing coverage report")
        omit = [BINDIR, TESTDIR, PYTHONDIR]
        if not options.cover_stdlib:
            # Exclude as system paths (ignoring empty strings seen on win)
            omit += [x for x in sys.path if x != '']  
        omit = ','.join(omit)
        os.chdir(PYTHONDIR)
        cmd = '"%s" "%s" -r "--omit=%s"' % (
            sys.executable, os.path.join(TESTDIR, 'coverage.py'), omit)
        vlog("# Running: "+cmd)
        os.system(cmd)
        if options.annotate:
            adir = os.path.join(TESTDIR, 'annotated')
            if not os.path.isdir(adir):
                os.mkdir(adir)
            cmd = '"%s" "%s" -a "--directory=%s" "--omit=%s"' % (
                sys.executable, os.path.join(TESTDIR, 'coverage.py'),
                adir, omit)
            vlog("# Running: "+cmd)
            os.system(cmd)

def run(cmd, split_lines=True):
    """Run command in a sub-process, capturing the output (stdout and stderr).
    Return the exist code, and output."""
    # TODO: Use subprocess.Popen if we're running on Python 2.4
    if os.name == 'nt':
        tochild, fromchild = os.popen4(cmd)
        tochild.close()
        output = fromchild.read()
        ret = fromchild.close()
        if ret == None:
            ret = 0
    else:
        proc = popen2.Popen4(cmd)
        proc.tochild.close()
        output = proc.fromchild.read()
        ret = proc.wait()
    if split_lines:
        output = output.splitlines()
    return ret, output

def run_one(test):
    vlog("# Test", test)
    if not verbose:
        sys.stdout.write('.')
        sys.stdout.flush()

    err = os.path.join(TESTDIR, test+".err")
    ref = os.path.join(TESTDIR, test+".out")

    if os.path.exists(err):
        os.remove(err)       # Remove any previous output files

    # Make a tmp subdirectory to work in
    tmpd = os.path.join(HGTMP, test)
    os.mkdir(tmpd)
    os.chdir(tmpd)

    if test.endswith(".py"):
        cmd = '%s "%s"' % (sys.executable, os.path.join(TESTDIR, test))
    else:
        cmd = '"%s"' % (os.path.join(TESTDIR, test))

    # To reliably get the error code from batch files on WinXP,
    # the "cmd /c call" prefix is needed. Grrr
    if os.name == 'nt' and test.endswith(".bat"):
        cmd = 'cmd /c call "%s"' % (os.path.join(TESTDIR, test))

    vlog("# Running", cmd)
    ret, out = run(cmd)
    vlog("# Ret was:", ret)

    if ret == 0:
        # If reference output file exists, check test output against it
        if os.path.exists(ref):
            f = open(ref, "r")
            ref_out = f.read().splitlines()
            f.close()
            if out != ref_out:
                ret = 1
                print "\nERROR: %s output changed" % (test)
                show_diff(ref_out, out)
    else:
        print "\nERROR: %s failed with error code %d" % (test, ret)

    if ret != 0: # Save errors to a file for diagnosis
        f = open(err, "w")
        for line in out:
            f.write(line)
            f.write("\n")
        f.close()

    os.chdir(TESTDIR)
    shutil.rmtree(tmpd, True)
    return ret == 0


os.umask(022)

check_required_tools()

# Reset some environment variables to well-known values so that
# the tests produce repeatable output.
os.environ['LANG'] = os.environ['LC_ALL'] = 'C'
os.environ['TZ'] = 'GMT'

os.environ["HGEDITOR"] = sys.executable + ' -c "import sys; sys.exit(0)"'
os.environ["HGMERGE"]  = sys.executable + ' -c "import sys; sys.exit(0)"'
os.environ["HGUSER"]   = "test"
os.environ["HGRCPATH"] = ""

TESTDIR = os.environ["TESTDIR"] = os.getcwd()
HGTMP   = os.environ["HGTMP"]   = tempfile.mkdtemp("", "hgtests.")
vlog("# Using TESTDIR", TESTDIR)
vlog("# Using HGTMP", HGTMP)

INST = os.path.join(HGTMP, "install")
BINDIR = os.path.join(INST, "bin")
PYTHONDIR = os.path.join(INST, "lib", "python")
COVERAGE_FILE = os.path.join(TESTDIR, ".coverage")

try:
    install_hg()

    tests = 0
    failed = 0

    if len(args) == 0:
        args = os.listdir(".")
    for test in args:
        if test.startswith("test-"):
            if '~' in test or re.search(r'\.(out|err)$', test):
                continue
            if not run_one(test):
                failed += 1
            tests += 1

    print "\n# Ran %d tests, %d failed." % (tests, failed)
    output_coverage()
finally:
    cleanup_exit()

if failed:
    sys.exit(1)
