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

from mercurial import dagop, error, registrar, revset
from mercurial.i18n import _


revsetpredicate = registrar.revsetpredicate()

_ageparser = re.compile(r"^(?:(\d+)d)?(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s?)?$")


def parseage(age):
    m = _ageparser.match(age)
    if not m:
        raise error.ParseError("invalid age in age range: %s" % age)
    days, hours, minutes, seconds = m.groups()
    agedelta = 0
    agedelta += int(days or 0) * 60 * 60 * 24
    agedelta += int(hours or 0) * 60 * 60
    agedelta += int(minutes or 0) * 60
    agedelta += int(seconds or 0)
    return agedelta


def parseagerange(agerange):
    now = time.time()
    if agerange.startswith("<"):
        start = now - parseage(agerange[1:])
        end = None
    elif agerange.startswith(">"):
        start = None
        end = now - parseage(agerange[1:])
    elif "-" in agerange:
        a1, a2 = agerange.split("-", 1)
        start = now - parseage(a2)
        end = now - parseage(a1)
    else:
        raise error.ParseError("invalid age range")
    return start, end


@revsetpredicate("age(string)", weight=10)
def age(repo, subset, x):
    """Changesets that are in a specific age range.

    The age range can be specified in days, hours, minutes or seconds:

    - ``<30d``  : Newer than 30 days old
    - ``>4h30m``: Older than 4 hours 30 minutes old
    - ``<15s``  : Newer than 15 seconds old
    - ``1h-5h`` : Between 1 and 5 hours old

    If no unit is specified, seconds are assumed.
    """
    agerange = revset.getstring(x, "age requires an age range")
    start, end = parseagerange(agerange)

    def agefunc(x):
        xdate = repo[x].date()[0]
        return (start is None or start < xdate) and (end is None or xdate < end)

    return subset.filter(agefunc, condrepr=("<age %r>", agerange))


@revsetpredicate("ancestorsaged(set, agerange)")
def ancestorsaged(repo, subset, x):
    """Ancestors in an age range, stopping at the first that is too old

    Similar to ``ancestors(set) & age(agerange)`` except that all ancestors that
    are ancestors of any commit that is older than the age range are also
    excluded. This only matters if there are changesets that have ancestors that
    are newer than them.

    For example, given the changesets:

        o aaa (1 hour ago)
        |
        o bbb (2 hours ago)
        |
        o ccc (3 hours ago)
        |
        o ddd (4 hours ago)
        |
        o eee (2 hours ago)
        |
        o fff (5 hours ago)
        |
        ~

    The expression ``ancestorsaged(aaa, "30m-3h30m")`` would match changesets
    ``bbb`` and ``ccc`` only.  The changeset ``eee`` is excluded by virtue of
    being an ancestor of ``ddd``, which is outside the age range.

    The age range can be specified in days, hours, minutes or seconds:

    - ``<30d``  : Newer than 30 days old
    - ``>4h30m``: Older than 4 hours 30 minutes old
    - ``<15s``  : Newer than 15 seconds old
    - ``1h-5h`` : Between 1 and 5 hours old

    If no unit is specified, seconds are assumed.
    """
    args = revset.getargsdict(x, "ancestorsaged", "set agerange")
    if "set" not in args or "agerange" not in args:
        # i18n: "ancestorsaged" is a keyword
        raise error.ParseError(_("ancestorsaged takes at least 2 arguments"))
    heads = revset.getset(repo, revset.fullreposet(repo), args["set"])
    if not heads:
        return revset.baseset()
    agerange = revset.getstring(
        args["agerange"], _("ancestorsaged requires an age range")
    )
    start, end = parseagerange(agerange)

    def older(x):
        return repo[x].date()[0] < start

    def notyounger(x):
        return repo[x].date()[0] < end

    s = dagop.revancestors(repo, heads, cutfunc=older if start is not None else None)
    if end is not None:
        s = s.filter(notyounger)

    return subset & s
