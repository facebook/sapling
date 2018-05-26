# age.py
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
a revset predicate for filtering by changeset age.

Adds the `age()` revset predicate.

This revset predicate differs from the built-in `date` by providing a more
granular way of considering relative time rather than absolute time.

The built-in `date()` predicate does provide full day resolution, so
`age("<Xd")` is equivalent to `date("-X")`.
"""

from __future__ import absolute_import

import re
import time

from mercurial import error, registrar, revsetlang


revsetpredicate = registrar.revsetpredicate()

_rangeparser = re.compile(r"^([<>])(?:(\d+)d)?(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s?)?$")


@revsetpredicate("age(string)")
def age(repo, subset, x):
    """Changesets that are older or newer than a specific age.

    The age range can be specified in days, hours, minutes or seconds:

    - ``<30d``  : Newer than 30 days old
    - ``>4h30m``: Older than 4 hours 30 minutes old
    - ``<15s``  : Newer than 15 seconds old

    If no unit is specified, seconds are assumed.
    """
    agerange = revsetlang.getstring(x, "age requires an age range")
    m = _rangeparser.match(agerange)
    if not m:
        raise error.ParseError("invalid age range for age predicate")
    dirn, days, hours, minutes, seconds = m.groups()
    cutoff = time.time()
    cutoff -= int(days or 0) * 60 * 60 * 24
    cutoff -= int(hours or 0) * 60 * 60
    cutoff -= int(minutes or 0) * 60
    cutoff -= int(seconds or 0)

    def newer(x):
        return repo[x].date()[0] > cutoff

    def older(x):
        return repo[x].date()[0] < cutoff

    if dirn == "<":
        return subset.filter(newer, condrepr=("<age %r>", agerange))
    else:
        return subset.filter(older, condrepr=("<age %r>", agerange))
