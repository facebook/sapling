#!/usr/bin/env python
#
# check-perf-code - (historical) portability checker for contrib/perf.py

from __future__ import absolute_import

import os
import sys

# write static check patterns here
perfpypats = [
  [
    (r'(branchmap|repoview)\.subsettable',
     "use getbranchmapsubsettable() for early Mercurial"),
    (r'\.(vfs|svfs|opener|sopener)',
     "use getvfs()/getsvfs() for early Mercurial"),
    (r'ui\.configint',
     "use getint() instead of ui.configint() for early Mercurial"),
  ],
  # warnings
  [
  ]
]

def modulewhitelist(names):
    replacement = [('.py', ''), ('.c', ''), # trim suffix
                   ('mercurial%s' % (os.sep), ''), # trim "mercurial/" path
                  ]
    ignored = {'__init__'}
    modules = {}

    # convert from file name to module name, and count # of appearances
    for name in names:
        name = name.strip()
        for old, new in replacement:
            name = name.replace(old, new)
        if name not in ignored:
            modules[name] = modules.get(name, 0) + 1

    # list up module names, which appear multiple times
    whitelist = []
    for name, count in modules.items():
        if count > 1:
            whitelist.append(name)

    return whitelist

if __name__ == "__main__":
    # in this case, it is assumed that result of "hg files" at
    # multiple revisions is given via stdin
    whitelist = modulewhitelist(sys.stdin)
    assert whitelist, "module whitelist is empty"

    # build up module whitelist check from file names given at runtime
    perfpypats[0].append(
        # this matching pattern assumes importing modules from
        # "mercurial" package in the current style below, for simplicity
        #
        #    from mercurial import (
        #        foo,
        #        bar,
        #        baz
        #    )
        ((r'from mercurial import [(][a-z0-9, \n#]*\n(?! *%s,|^[ #]*\n|[)])'
          % ',| *'.join(whitelist)),
         "import newer module separately in try clause for early Mercurial"
         ))

    # import contrib/check-code.py as checkcode
    assert 'RUNTESTDIR' in os.environ, "use check-perf-code.py in *.t script"
    contribpath = os.path.join(os.environ['RUNTESTDIR'], '..', 'contrib')
    sys.path.insert(0, contribpath)
    checkcode = __import__('check-code')

    # register perf.py specific entry with "checks" in check-code.py
    checkcode.checks.append(('perf.py', r'contrib/perf.py$', '',
                             checkcode.pyfilters, perfpypats))

    sys.exit(checkcode.main())
