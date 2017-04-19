# nodeprecate.py - a decorator to suppress DeprecationWarning notices
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""this is a horrible module that exists purely to squelch the
DeprecationWarning notices emitting by contextlib.nested when running on Python
2.7.  Once we've moved entirely beyond Python 2.6 we can remove this and fixup
the usage of contextlib.nested to use the natively supported with syntax.
"""

import warnings

def nodeprecate(func):
    def wrapper(*args, **kwargs):
        with warnings.catch_warnings():
            warnings.filterwarnings("ignore", category=DeprecationWarning)
            return func(*args, **kwargs)
    return wrapper
