# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# phrevset.py - support for Phabricator revsets

"""provides support for Phabricator revsets

Allows for queries such as `hg log -r D1234567` to find the commit which
corresponds to a specific Differential revision.
Automatically handles commits already in subversion, or whose hash has
changed since submitting to Differential (due to amends or rebasing).

Requires arcanist to be installed and properly configured.
Repositories should include a callsign in their hgrc.

Example for www::

    [phrevset]
    callsign = E
    # Only ask GraphQL. Do not scan the local commits (which do not scale).
    graphqlonly = True

"""

import re
import threading

from edenscm.mercurial import error, hg, json, namespaces, pycompat, registrar, util
from edenscm.mercurial.i18n import _

from .extlib.phabricator import graphql


configtable = {}
configitem = registrar.configitem(configtable)

configitem("phrevset", "callsign", default=None)
configitem("phrevset", "graphqlonly", default=True)

namespacepredicate = registrar.namespacepredicate()

DIFFERENTIAL_REGEX = re.compile(
    "Differential Revision: http.+?/"  # Line start, URL
    "D(?P<id>[0-9]+)"  # Differential ID, just numeric part
)

DESCRIPTION_REGEX = re.compile(
    "Commit r"  # Prefix
    "(?P<callsign>[A-Z]{1,})"  # Callsign
    "(?P<id>[a-f0-9]+)"  # rev
)


def graphqlgetdiff(repo, diffid):
    """Resolves a phabricator Diff number to a commit hash of it's latest version """
    if util.istest():
        hexnode = repo.ui.config("phrevset", "mock-D%s" % diffid)
        if hexnode:
            return {
                "source_control_system": "hg",
                "description": "mock",
                "phabricator_version_properties": {
                    "edges": [
                        {
                            "node": {
                                "property_name": "local:commits",
                                "property_value": json.dumps(
                                    {hexnode: {"commit": hexnode, "rev": hexnode}}
                                ),
                            }
                        }
                    ]
                },
            }
    timeout = repo.ui.configint("ssl", "timeout", 10)
    ca_certs = repo.ui.configpath("web", "cacerts")
    try:
        client = graphql.Client(
            repodir=pycompat.getcwd(), ca_bundle=ca_certs, repo=repo
        )
        return client.getdifflatestversion(timeout, diffid)
    except Exception as e:
        raise error.Abort(
            "Could not call phabricator graphql API: %s" % e,
            hint="perhaps you need to run 'jf auth'?",
        )


def localgetdiff(repo, diffid, querythread=None):
    """Scans the changelog for commit lines mentioning the Differential ID

    If the optional querythread parameter is provided, it must be a threading.Thread
    instance. It will be polled during the iteration and if it indicates that
    the thread has finished, the function will raise StopIteration"""
    if repo.ui.configbool("phrevset", "graphqlonly"):
        raise error.Abort(
            _("phrevset.graphqlonly is set and Phabricator cannot resolve D%s") % diffid
        )

    repo.ui.debug("[diffrev] Traversing log for %s\n" % diffid)

    def check(repo, rev, diffid):
        changectx = repo[rev]
        desc = changectx.description()
        match = DIFFERENTIAL_REGEX.search(desc)

        if match and match.group("id") == diffid:
            return changectx.rev()
        else:
            return None

    # Search through draft commits first. This is still needed as there are
    # cases where Phabricator GraphQL cannot resolve the commit for some reason
    # and the user really wants to resolve the commit locally (ex. S199694).
    for rev in repo.revs("sort(draft(), -rev)"):
        matched = check(repo, rev, diffid)
        if matched is not None:
            return matched
        if querythread and querythread.is_alive() is False:
            raise StopIteration("Parallel query completed")

    repo.ui.warn(
        _("D%s not found in drafts. Perform (slow) full changelog scan.\n") % diffid
    )

    # Search through the whole changelog. This does not scale. Log this as we
    # plan to remove it at some point.
    repo.ui.log(
        "features",
        fullargs=repr(pycompat.sysargv),
        feature="phrevset-full-changelog-scan",
    )
    for rev in repo.changelog.revs(start=len(repo.changelog), stop=0):
        matched = check(repo, rev, diffid)
        if matched is not None:
            return matched
        if rev % 100 == 0 and querythread and querythread.is_alive() is False:
            raise StopIteration("Parallel query completed")

    return None


def forksearch(repo, diffid):
    """Perform a log traversal and GraphQL call in parallel

    Returns a (revisions, graphql_response) tuple, where one of the items will be
    None, depending on which process terminated first"""

    repo.ui.debug("[diffrev] Starting graphql call\n")
    if repo.ui.configbool("phrevset", "graphqlonly"):
        return (None, graphqlgetdiff(repo, diffid))

    result = [None, None]

    def makegraphqlcall():
        try:
            result[0] = graphqlgetdiff(repo, diffid)
        except Exception as exc:
            result[1] = exc

    querythread = threading.Thread(target=makegraphqlcall, name="graphqlquery")
    querythread.daemon = True
    querythread.start()

    try:
        repo.ui.debug("[diffrev] Starting log walk\n")
        rev = localgetdiff(repo, diffid, querythread)

        repo.ui.debug("[diffrev] Parallel log walk completed with %s\n" % rev)

        if rev is None:
            # walked the entire repo and couldn't find the diff
            raise error.Abort("Could not find diff D%s in changelog" % diffid)

        return ([rev], None)

    except StopIteration:
        # search terminated because arc returned
        # if returncode == 0, return arc's output

        repo.ui.debug("[diffrev] graphql call returned %s\n" % result[0])

        if result[1] is not None:
            raise result[1]

        return (None, result[0])


def parsedesc(repo, resp, ignoreparsefailure):
    desc = resp["description"]
    if desc is None:
        if ignoreparsefailure:
            return None
        else:
            raise error.Abort("No Conduit description")

    match = DESCRIPTION_REGEX.match(desc)

    if not match:
        if ignoreparsefailure:
            return None
        else:
            raise error.Abort("Cannot parse Conduit description '%s'" % desc)

    callsign = match.group("callsign")
    repo_callsign = repo.ui.config("phrevset", "callsign")

    if callsign != repo_callsign:
        raise error.Abort(
            "Diff callsign '%s' is different from repo"
            " callsign '%s'" % (callsign, repo_callsign)
        )

    return match.group("id")


@util.lrucachefunc
def revsetdiff(repo, diffid):
    """Return a set of revisions corresponding to a given Differential ID """

    repo_callsign = repo.ui.config("phrevset", "callsign")
    if repo_callsign is None:
        msg = _("phrevset.callsign is not set - doing a linear search\n")
        hint = _("This will be slow if the diff was not committed recently\n")
        repo.ui.warn(msg)
        repo.ui.warn(hint)
        rev = localgetdiff(repo, diffid)
        if rev is None:
            raise error.Abort("Could not find diff D%s in changelog" % diffid)
        else:
            return [rev]

    revs, resp = forksearch(repo, diffid)

    if revs is not None:
        # The log walk found the diff, nothing more to do
        return revs

    if resp is None:
        # The graphql query finished but didn't return anything
        return []

    vcs = resp["source_control_system"]

    repo.ui.debug("[diffrev] VCS is %s\n" % vcs)

    if vcs == "git":
        gitrev = parsedesc(repo, resp, ignoreparsefailure=False)
        repo.ui.debug("[diffrev] GIT rev is %s\n" % gitrev)

        peerpath = repo.ui.expandpath("default")
        remoterepo = hg.peer(repo, {}, peerpath)
        remoterev = remoterepo.lookup("_gitlookup_git_%s" % gitrev)

        repo.ui.debug("[diffrev] HG rev is %s\n" % remoterev.encode("hex"))
        if not remoterev:
            repo.ui.debug("[diffrev] Falling back to linear search\n")
            linear_search_result = localgetdiff(repo, diffid)
            if linear_search_result is None:
                # walked the entire repo and couldn't find the diff
                raise error.Abort("Could not find diff D%s in changelog" % diffid)

            return [linear_search_result]

        return [repo[remoterev].rev()]

    elif vcs == "hg":
        rev = parsedesc(repo, resp, ignoreparsefailure=True)
        if rev:
            # The response from phabricator contains a changeset ID.
            # Convert it back to a rev number.
            try:
                return [repo[rev].rev()]
            except error.RepoLookupError:
                # TODO: 's/svnrev/globalrev' after turning off Subversion
                # servers. We will know about this when we remove the `svnrev`
                # revset.
                #
                # Unfortunately the rev can also be a svnrev/globalrev :(.
                if rev.isdigit():
                    try:
                        return [r for r in repo.revs("svnrev(%s)" % rev)]
                    except error.RepoLookupError:
                        pass

                raise error.Abort(
                    "Landed commit for diff D%s not available "
                    'in current repository: run "hg pull" '
                    "to retrieve it" % diffid
                )

        # commit is still local, get its hash

        props = resp["phabricator_version_properties"]["edges"]
        commits = []
        for prop in props:
            if prop["node"]["property_name"] == "local:commits":
                commits = json.loads(prop["node"]["property_value"])

        revs = [c["commit"] for c in commits.values()]

        # verify all revisions exist in the current repo; if not, try to
        # find their counterpart by parsing the log
        results = set()
        for rev in revs:
            try:
                unfiltered = repo.unfiltered()
                node = unfiltered[rev]
            except error.RepoLookupError:
                raise error.Abort(
                    _("cannot find the latest version of D%s (%s) locally")
                    % (diffid, rev),
                    hint=_("try 'hg pull -r %s'") % rev,
                )
            successors = list(repo.revs("last(successors(%n))", node.node()))
            if len(successors) != 1:
                results.add(node.rev())
            else:
                results.add(successors[0])

        if not results:
            raise error.Abort("Could not find local commit for D%s" % diffid)

        return set(results)

    else:
        if not vcs:
            msg = (
                "D%s does not have an associated version control system\n"
                "You can view the diff at https:///our.internmc.facebook.com/intern/diff/D%s\n"
            )
            repo.ui.warn(msg % (diffid, diffid))

            return []
        else:
            raise error.Abort(
                "Conduit returned unknown " 'sourceControlSystem "%s"' % vcs
            )


def _lookupname(repo, name):
    cl = repo.changelog
    tonode = cl.node
    if name.startswith("D") and name[1:].isdigit():
        return [tonode(r) for r in revsetdiff(repo, name[1:])]
    else:
        return []


@namespacepredicate("phrevset", priority=70)
def _getnamespace(_repo):
    return namespaces.namespace(
        listnames=lambda repo: [], namemap=_lookupname, nodemap=lambda repo, node: []
    )
