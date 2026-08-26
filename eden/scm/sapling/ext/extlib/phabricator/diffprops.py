# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


import re
from typing import Pattern


# Mirrors jf's DIFF_IN_COMMIT_REGEX so both tools agree on which commits are
# bound to a diff: the line must start at column 0, exactly one space follows
# the colon, and a bare "D123" (no URL) also binds.
_diffrevisioncore = r"^Differential Revision: \S*D(\d+)"

diffrevisionregex: Pattern[str] = re.compile(_diffrevisioncore, re.M)

# Matches the entire binding line, for removing it from a commit message.
# Derived from the same core pattern so the two can never disagree about
# which lines are bindings.
diffrevisionlineregex: Pattern[str] = re.compile(
    _diffrevisioncore + r"[^\n]*(?:\n|$)", re.M
)


def parserevfromcommitmsg(description):
    """Parses the D123 revision number from a commit message.
    Returns just the revision number without the D prefix.
    Matches any URL as a candidate, not just our internal phabricator
    host, so this can also work with our public phabricator instance,
    or for others. Bare diff numbers without a URL are also accepted,
    matching jf's parsing.
    """
    match = diffrevisionregex.search(description)
    return match.group(1) if match else None
