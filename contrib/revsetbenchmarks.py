#!/usr/bin/env python

# Measure the performance of a list of revsets against multiple revisions
# defined by parameter. Checkout one by one and run perfrevset with every
# revset in the list to benchmark its performance.
#
# - First argument is a revset of mercurial own repo to runs against.
# - Second argument is the file from which the revset array will be taken
#   If second argument is omitted read it from standard input
#
# You should run this from the root of your mercurial repository.
#
# This script also does one run of the current version of mercurial installed
# to compare performance.

import sys
import os
from subprocess import check_call, Popen, CalledProcessError, STDOUT, PIPE
# cannot use argparse, python 2.7 only
from optparse import OptionParser

def check_output(*args, **kwargs):
    kwargs.setdefault('stderr', PIPE)
    kwargs.setdefault('stdout', PIPE)
    proc = Popen(*args, **kwargs)
    output, error = proc.communicate()
    if proc.returncode != 0:
        raise CalledProcessError(proc.returncode, ' '.join(args[0]))
    return output

def update(rev):
    """update the repo to a revision"""
    try:
        check_call(['hg', 'update', '--quiet', '--check', str(rev)])
    except CalledProcessError, exc:
        print >> sys.stderr, 'update to revision %s failed, aborting' % rev
        sys.exit(exc.returncode)


def hg(cmd, repo=None):
    """run a mercurial command

    <cmd> is the list of command + argument,
    <repo> is an optional repository path to run this command in."""
    fullcmd = ['./hg']
    if repo is not None:
        fullcmd += ['-R', repo]
    fullcmd += ['--config',
                'extensions.perf=' + os.path.join(contribdir, 'perf.py')]
    fullcmd += cmd
    return check_output(fullcmd, stderr=STDOUT)

def perf(revset, target=None):
    """run benchmark for this very revset"""
    try:
        output = hg(['perfrevset', revset], repo=target)
        output = output.lstrip('!') # remove useless ! in this context
        return output.strip()
    except CalledProcessError, exc:
        print >> sys.stderr, 'abort: cannot run revset benchmark: %s' % exc.cmd
        if exc.output is None:
            print >> sys.stderr, '(no ouput)'
        else:
            print >> sys.stderr, exc.output
        sys.exit(exc.returncode)

def printrevision(rev):
    """print data about a revision"""
    sys.stdout.write("Revision: ")
    sys.stdout.flush()
    check_call(['hg', 'log', '--rev', str(rev), '--template',
               '{desc|firstline}\n'])

def getrevs(spec):
    """get the list of rev matched by a revset"""
    try:
        out = check_output(['hg', 'log', '--template={rev}\n', '--rev', spec])
    except CalledProcessError, exc:
        print >> sys.stderr, "abort, can't get revision from %s" % spec
        sys.exit(exc.returncode)
    return [r for r in out.split() if r]


parser = OptionParser(usage="usage: %prog [options] <revs>")
parser.add_option("-f", "--file",
                  help="read revset from FILE (stdin if omitted)",
                  metavar="FILE")
parser.add_option("-R", "--repo",
                  help="run benchmark on REPO", metavar="REPO")

(options, args) = parser.parse_args()

if len(sys.argv) < 2:
    parser.print_help()
    sys.exit(255)

# the directory where both this script and the perf.py extension live.
contribdir = os.path.dirname(__file__)

target_rev = args[0]

revsetsfile = sys.stdin
if options.file:
    revsetsfile = open(options.file)

revsets = [l.strip() for l in revsetsfile if not l.startswith('#')]

print "Revsets to benchmark"
print "----------------------------"

for idx, rset in enumerate(revsets):
    print "%i) %s" % (idx, rset)

print "----------------------------"
print


revs = getrevs(target_rev)

results = []
for r in revs:
    print "----------------------------"
    printrevision(r)
    print "----------------------------"
    update(r)
    res = []
    results.append(res)
    for idx, rset in enumerate(revsets):
        data = perf(rset, target=options.repo)
        res.append(data)
        print "%i)" % idx, data
        sys.stdout.flush()
    print "----------------------------"


print """

Result by revset
================
"""

print 'Revision:', revs
for idx, rev in enumerate(revs):
    sys.stdout.write('%i) ' % idx)
    sys.stdout.flush()
    printrevision(rev)

print
print

for ridx, rset in enumerate(revsets):

    print "revset #%i: %s" % (ridx, rset)
    for idx, data in enumerate(results):
        print '%i) %s' % (idx, data[ridx])
    print
