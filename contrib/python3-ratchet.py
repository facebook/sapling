# Copyright 2012 Facebook
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""Find tests that newly pass under Python 3.

The approach is simple: we maintain a whitelist of Python 3 passing
tests in the repository, and periodically run all the /other/ tests
and look for new passes. Any newly passing tests get automatically
added to the whitelist.

You probably want to run it like this:

  $ cd tests
  $ python3 ../contrib/python3-ratchet.py \
  >   --working-tests=../contrib/python3-whitelist
"""
from __future__ import print_function
from __future__ import absolute_import

import argparse
import json
import os
import subprocess
import sys

_hgenv = dict(os.environ)
_hgenv.update({
    'HGPLAIN': '1',
    })

_HG_FIRST_CHANGE = '9117c6561b0bd7792fa13b50d28239d51b78e51f'

def _runhg(*args):
    return subprocess.check_output(args, env=_hgenv)

def _is_hg_repo(path):
    return _runhg('hg', 'log', '-R', path,
                  '-r0', '--template={node}').strip() == _HG_FIRST_CHANGE

def _py3default():
    if sys.version_info[0] >= 3:
        return sys.executable
    return 'python3'

def main(argv=()):
    p = argparse.ArgumentParser()
    p.add_argument('--working-tests',
                   help='List of tests that already work in Python 3.')
    p.add_argument('--commit-to-repo',
                   help='If set, commit newly fixed tests to the given repo')
    p.add_argument('-j', default=os.sysconf(r'SC_NPROCESSORS_ONLN'), type=int,
                   help='Number of parallel tests to run.')
    p.add_argument('--python3', default=_py3default(),
                   help='python3 interpreter to use for test run')
    p.add_argument('--commit-user',
                   default='python3-ratchet@mercurial-scm.org',
                   help='Username to specify when committing to a repo.')
    opts = p.parse_args(argv)
    if opts.commit_to_repo:
        if not _is_hg_repo(opts.commit_to_repo):
            print('abort: specified repository is not the hg repository')
            sys.exit(1)
    if not opts.working_tests or not os.path.isfile(opts.working_tests):
        print('abort: --working-tests must exist and be a file (got %r)' %
              opts.working_tests)
        sys.exit(1)
    elif opts.commit_to_repo:
        root = _runhg('hg', 'root').strip()
        if not opts.working_tests.startswith(root):
            print('abort: if --commit-to-repo is given, '
                  '--working-tests must be from that repo')
            sys.exit(1)
    try:
        subprocess.check_call([opts.python3, '-c',
                               'import sys ; '
                               'assert ((3, 5) <= sys.version_info < (3, 6) '
                               'or sys.version_info >= (3, 6, 2))'])
    except subprocess.CalledProcessError:
        print('warning: Python 3.6.0 and 3.6.1 have '
              'a bug which breaks Mercurial')
        print('(see https://bugs.python.org/issue29714 for details)')
        # TODO(augie): uncomment exit when Python 3.6.2 is available
        # sys.exit(1)

    rt = subprocess.Popen([opts.python3, 'run-tests.py', '-j', str(opts.j),
                           '--blacklist', opts.working_tests, '--json'])
    rt.wait()
    with open('report.json') as f:
        data = f.read()
    report = json.loads(data.split('=', 1)[1])
    newpass = set()
    for test, result in report.items():
        if result['result'] != 'success':
            continue
        # A new passing test! Huzzah!
        newpass.add(test)
    if newpass:
        # We already validated the repo, so we can just dive right in
        # and commit.
        if opts.commit_to_repo:
            print(len(newpass), 'new passing tests on Python 3!')
            with open(opts.working_tests) as f:
                oldpass = {l for l in f.read().splitlines() if l}
            with open(opts.working_tests, 'w') as f:
                for p in sorted(oldpass | newpass):
                    f.write('%s\n' % p)
            _runhg('hg', 'commit', '-R', opts.commit_to_repo,
                   '--user', opts.commit_user,
                   '--message', 'python3: expand list of passing tests')
        else:
            print('Newly passing tests:', '\n'.join(sorted(newpass)))
            sys.exit(2)

if __name__ == '__main__':
    main(sys.argv[1:])
