import re
from operator import itemgetter

diffrevisionregex = re.compile('Differential Revision:.*/D(\d+)')

def parserevfromcommitmsg(description):
    """Parses the D123 revision number from a commit message.
    Returns just the revision number without the D prefix.
    Matches any URL as a candidate, not just our internal phabricator
    host, so this can also work with our public phabricator instance,
    or for others.
    """
    match = diffrevisionregex.search(description)
    return match.group(1) if match else None

def getcurrentdiffidforrev(client, phabrev):
    """Given a revision number (the 123 from D123), returns the current
    diff id associated with that revision. """

    res = client.call('differential.query', {'ids': [phabrev]})
    if not res:
        return None

    info = res[0]
    if not info:
        return None

    diffs = info.get('diffs', [])
    if not diffs:
        return None

    return max(diffs)

def getlocalcommitfordiffid(client, diffid):
    """Returns the most recent local:commits entry for a phabricator diff_id"""

    res = client.call('differential.getdiffproperties', {
                       'diff_id': diffid,
                       'names': ['local:commits']})
    if not res:
        return None

    localcommits = res.get('local:commits', {})
    if not localcommits:
        return None

    # Order with most recent commit time first.  A more completely correct
    # implementation would toposort based on the parents properties, however,
    # wez thinks that we should only contain a single entry most of the time,
    # and our best prior art used to just take the first item that showed up
    # in the dictionary.  Sorting gives us some determinism, so we will at
    # least be consistently wrong if we're wrong.
    localcommits = sorted(localcommits.values(),
                          key=itemgetter('time'), reverse=True)

    return localcommits[0]

