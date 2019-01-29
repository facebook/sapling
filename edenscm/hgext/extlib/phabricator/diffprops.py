# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re


diffrevisionregex = re.compile("^Differential Revision:.*/D(\d+)", re.M)


def parserevfromcommitmsg(description):
    """Parses the D123 revision number from a commit message.
    Returns just the revision number without the D prefix.
    Matches any URL as a candidate, not just our internal phabricator
    host, so this can also work with our public phabricator instance,
    or for others.
    """
    match = diffrevisionregex.search(description)
    return match.group(1) if match else None
