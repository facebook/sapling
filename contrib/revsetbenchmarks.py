#!/usr/bin/env python

# Measure the performance of a list of revsets against multiple revisions
# defined by parameter. Checkout one by one and run perfrevset with every
# revset in the list to benchmark its performance.
#
# You should run this from the root of your mercurial repository.
#
# call with --help for details

import sys
import os
import re
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
        return parseoutput(output)
    except CalledProcessError, exc:
        print >> sys.stderr, 'abort: cannot run revset benchmark: %s' % exc.cmd
        if exc.output is None:
            print >> sys.stderr, '(no ouput)'
        else:
            print >> sys.stderr, exc.output
        sys.exit(exc.returncode)

outputre = re.compile(r'! wall (\d+.\d+) comb (\d+.\d+) user (\d+.\d+) '
                      'sys (\d+.\d+) \(best of (\d+)\)')

def parseoutput(output):
    """parse a textual output into a dict

    We cannot just use json because we want to compare with old
    versions of Mercurial that may not support json output.
    """
    match = outputre.search(output)
    if not match:
        print >> sys.stderr, 'abort: invalid output:'
        print >> sys.stderr, output
        sys.exit(1)
    return {'comb': float(match.group(2)),
            'count': int(match.group(5)),
            'sys': float(match.group(3)),
            'user': float(match.group(4)),
            'wall': float(match.group(1)),
            }

def printrevision(rev):
    """print data about a revision"""
    sys.stdout.write("Revision: ")
    sys.stdout.flush()
    check_call(['hg', 'log', '--rev', str(rev), '--template',
               '{desc|firstline}\n'])

def idxwidth(nbidx):
    """return the max width of number used for index

    This is similar to log10(nbidx), but we use custom code here
    because we start with zero and we'd rather not deal with all the
    extra rounding business that log10 would imply.
    """
    nbidx -= 1 # starts at 0
    idxwidth = 0
    while nbidx:
        idxwidth += 1
        nbidx //= 10
    if not idxwidth:
        idxwidth = 1
    return idxwidth

def printresult(idx, data, maxidx):
    """print a line of result to stdout"""
    mask = '%%0%ii) %%s' % idxwidth(maxidx)
    out = ['%10.6f' % data['wall'],
           '%10.6f' % data['comb'],
           '%10.6f' % data['user'],
           '%10.6f' % data['sys'],
           '%6d'    % data['count'],
          ]
    print mask % (idx, ' '.join(out))

def printheader(maxidx):
    header = [' ' * (idxwidth(maxidx) + 1),
              '  %-8s' % 'wall',
              '  %-8s' % 'comb',
              '  %-8s' % 'user',
              '  %-8s' % 'sys',
              '%6s' % 'count',
             ]
    print ' '.join(header)

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

if not args:
    parser.print_help()
    sys.exit(255)

# the directory where both this script and the perf.py extension live.
contribdir = os.path.dirname(__file__)

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

revs = []
for a in args:
    revs.extend(getrevs(a))

results = []
for r in revs:
    print "----------------------------"
    printrevision(r)
    print "----------------------------"
    update(r)
    res = []
    results.append(res)
    printheader(len(revsets))
    for idx, rset in enumerate(revsets):
        data = perf(rset, target=options.repo)
        res.append(data)
        printresult(idx, data, len(revsets))
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
    printheader(len(results))
    for idx, data in enumerate(results):
        printresult(idx, data[ridx], len(results))
    print
