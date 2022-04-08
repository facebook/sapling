# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import edenscm.mercurial.help
import edenscm.mercurial.revset
from testutil.autofix import eq
from testutil.dott import sh  # noqa: F401qqq

predicate = edenscm.mercurial.revset.predicate
excluded = edenscm.mercurial.help._exclkeywords


@predicate("notarealpredicate()")
def notarealpredicate():
    pass


# Assert that all predicates have docstrings unless they are explicitly
# marked as hidden or have name starting with an underscore
eq(
    [
        name
        for name, func in predicate._table.items()
        if not name.startswith("_") and func.__doc__ is None
    ],
    ["notarealpredicate"],
)

# Assert that revsets from the "hg help revsets" command are the expected ones
helpoutput = (sh % "hg help revsets").output

expectedpredicates = set(
    str(func.__doc__.strip().split("(")[0][2:])
    for name, func in predicate._table.items()
    if name[0] != "_" and func.__doc__ and not any(w in func.__doc__ for w in excluded)
)

eq(
    set(pred for pred in expectedpredicates if pred + "(" not in helpoutput),
    set(),
)
