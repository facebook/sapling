#!/usr/bin/env python
import json
import multiprocessing
import os
import re
import subprocess
import sys

"""run a subset of tests that related to the current change

Optionally write result using JSON format. The JSON format can be parsed
by MercurialTestEngine.php
"""

reporoot = os.path.abspath(os.path.dirname(os.path.dirname(__file__)))

def info(message):
    """print message to stderr"""
    sys.stderr.write(message)

def getrunner():
    """return the path of run-tests.py. best-effort"""
    runner = os.environ.get('MERCURIALRUNTEST', 'run-tests.py')
    if os.path.exists(runner):
        return runner
    # Search some common places for run-tests.py
    for prefix in ['..', os.path.expanduser('~')]:
        for hgrepo in ['hg', 'hg-crew', 'hg-committed']:
            path = os.path.abspath(os.path.join(prefix, hgrepo,
                                                'tests', 'run-tests.py'))
            if os.path.exists(path):
                return path
    return runner

def checkoutput(*args, **kwds):
    """like subprocess.checked_output, but raise RuntimeError and return
    stderr as a second value.
    """
    proc = subprocess.Popen(*args, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, **kwds)
    out, err = proc.communicate()
    retcode = proc.poll()
    if retcode:
        raise RuntimeError('%r exits with %d' % (args, retcode))
    return out, err

def changedfiles(rev='wdir() + .'):
    """return a list of paths (relative to repo root) that rev touches.
    by default, check the working directory and its parent.
    """
    cmd = ['hg', 'log', '-T', '{join(files,"\\0")}', '-r', rev]
    out, err = checkoutput(cmd, cwd=reporoot)
    return out.split('\0')

def words(path):
    """strip extension and split it to words.
    for example, 'a/b-c.txt' -> ['a', 'b', 'c']
    """
    return re.split('[^\w]+', os.path.splitext(path)[0])

def interestingtests():
    """return a list of interesting test filenames"""
    tests = [p for p in os.listdir(os.path.join(reporoot, 'tests'))
             if p.startswith('test-') and p[-2:] in ['py', '.t']]
    # Convert ['test-foo-bar.t', 'test-baz.t'] to [{'foo', 'bar'}, {'baz'}]
    testwords = [set(words(t)[1:]) for t in tests]
    # Include test-check*, except for test-check-code-hg.t used by arc lint.
    result = set([t for t in tests
                  if (t.startswith('test-check')
                      and t != 'test-check-code-hg.t')])
    # A test is interesting if there is a common word in both the path of the
    # changed source file and the name of the test file. For example:
    # - test-githelp.t is interesting if githelp.py is changed
    # - test-remotefilelog-sparse.t is interesting if sparse.py is changed
    # - test-remotefilelog-foo.t is interesting if remotefilelog/* is changed
    for path in changedfiles():
        if path.startswith('tests/test-'):
            # for a test file, do not enable other tests but only itself
            result.add(os.path.basename(path))
            continue
        result.update(t for t, s in zip(tests, testwords)
                      if any(c in s for c in words(path)))
    return result

def reporequires():
    """return a list of string, which are the requirements of the hg repo"""
    requirespath = os.path.join(reporoot, '.hg', 'requires')
    if os.path.exists(requirespath):
        return [s.rstrip() for s in open(requirespath, 'r')]
    return []

def runtests(tests=[], jsonpath=None):
    """run given tests, optionally write the result to the given json path"""
    cpucount = multiprocessing.cpu_count()
    cmd = [getrunner(), '-j%d' % cpucount, '-l']
    requires = reporequires()
    if 'lz4revlog' in requires:
        cmd += ['--extra-config-opt=extensions.lz4revlog=']
    if jsonpath:
        cmd += ['--json']
    cmd += tests
    shellcmd = subprocess.list2cmdline(cmd)
    os.chdir(os.path.join(reporoot, 'tests'))
    exitcode = os.system(shellcmd)
    # move report.json to jsonpath
    if jsonpath:
        reportpath = os.path.join(reporoot, 'tests', 'report.json')
        report = open(reportpath).read()
        with open(jsonpath, 'w') as f:
            # strip the "testreport =" header which makes the JSON illegal
            f.write(re.sub('^testreport =', '', report))
        os.unlink(reportpath)
    return exitcode

if __name__ == '__main__':
    jsonpath = (sys.argv + [None, None])[1]
    tests = interestingtests()
    if tests:
        info('%d test%s to run: %s\n'
             % (len(tests), ('' if len(tests) == 1 else 's'), ' '.join(tests)))
        sys.exit(runtests(tests, jsonpath))
    else:
        info('no tests to run\n')
        # Write out an empty results file
        with open(jsonpath, 'w') as fp:
            json.dump({}, fp)
