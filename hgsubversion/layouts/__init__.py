"""Code for dealing with subversion layouts

This package is intended to encapsulate everything about subversion
layouts.  This includes detecting the layout based on looking at
subversion, mapping subversion paths to hg branches, and doing any
other path translation necessary.

NB: this has a long way to go before it does everything it claims to

"""

import detect
import persist

__all__ = [
    "detect",
    "persist",
    ]
