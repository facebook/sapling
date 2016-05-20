#!/usr/bin/env python
#
# check-perf-code - (historical) portability checker for contrib/perf.py

from __future__ import absolute_import

import os
import sys

# write static check patterns here
perfpypats = [
  [
  ],
  # warnings
  [
  ]
]

if __name__ == "__main__":
    # import contrib/check-code.py as checkcode
    assert 'RUNTESTDIR' in os.environ, "use check-perf-code.py in *.t script"
    contribpath = os.path.join(os.environ['RUNTESTDIR'], '..', 'contrib')
    sys.path.insert(0, contribpath)
    checkcode = __import__('check-code')

    # register perf.py specific entry with "checks" in check-code.py
    checkcode.checks.append(('perf.py', r'contrib/perf.py$', '',
                             checkcode.pyfilters, perfpypats))

    sys.exit(checkcode.main())
