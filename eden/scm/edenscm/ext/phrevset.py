# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# phrevset.py - support for Phabricator revsets

"""provides support for Phabricator revsets

Allows for queries such as `@prog@ log -r D1234567` to find the commit which
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
    # Automatically pull Dxxx.
    autopull = True

"""

import re
from typing import Optional, Pattern

from edenscm import autopull, error, hg, json, namespaces, pycompat, registrar, util
from edenscm.autopull import pullattempt
from edenscm.i18n import _
from edenscm.namespaces import namespace
from edenscm.node import bin, hex, nullhex

from .extlib.phabricator import graphql


configtable = {}
configitem = registrar.configitem(configtable)

configitem("phrevset", "autopull", default=True)
configitem("phrevset", "callsign", default=None)
configitem("phrevset", "graphqlonly", default=True)

namespacepredicate = registrar.namespacepredicate()
autopullpredicate = registrar.autopullpredicate()

DIFFERENTIAL_REGEX: Pattern[str] = re.compile(
    "Differential Revision: http.+?/"  # Line start, URL
    "D(?P<id>[0-9]+)"  # Differential ID, just numeric part
)

DESCRIPTION_REGEX: Pattern[str] = re.compile(
    "Commit r"  # Prefix
    "(?P<callsign>[A-Z]{1,})"  # Callsign
    "(?P<id>[a-f0-9]+)"  # rev
)


def graphqlgetdiff(repo, diffid):
    """Resolves a phabricator Diff number to a commit hash of it's latest version"""
    if util.istest():
        hexnode = repo.ui.config("phrevset", "mock-D%s" % diffid)
        if hexnode:
            return {
                "source_control_system": "hg",
                "description": "Commit rCALLSIGN{}".format(hexnode),
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
                "commits": {},
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
            hint="perhaps you need to connect to the VPN or run 'jf auth'?",
        )


def localgetdiff(repo, diffid):
    """Scans the changelog for commit lines mentioning the Differential ID"""

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
            return changectx.node()
        else:
            return None

    # Search through draft commits first. This is still needed as there are
    # cases where Phabricator GraphQL cannot resolve the commit for some reason
    # and the user really wants to resolve the commit locally (ex. S199694).
    for rev in repo.revs("sort(draft(), -rev)"):
        matched = check(repo, rev, diffid)
        if matched is not None:
            return matched

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

    return None


def search(repo, diffid):
    """Perform a GraphQL query first. If it fails, fallback to local search.

    Returns (node, None) or (None, graphql_response) tuple.
    """

    repo.ui.debug("[diffrev] Starting graphql call\n")
    if repo.ui.configbool("phrevset", "graphqlonly"):
        return (None, graphqlgetdiff(repo, diffid))

    try:
        return (None, graphqlgetdiff(repo, diffid))
    except Exception as ex:
        repo.ui.warn(_("cannot resolve D%s via GraphQL: %s\n") % (diffid, ex))
        repo.ui.warn(_("falling back to search commits locally\n"))
        repo.ui.debug("[diffrev] Starting log walk\n")
        node = localgetdiff(repo, diffid)
        if node is None:
            # walked the entire repo and couldn't find the diff
            raise error.Abort("Could not find diff D%s in changelog" % diffid)
        repo.ui.debug("[diffrev] Parallel log walk completed with %s\n" % hex(node))
        return (node, None)


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
    repo_callsigns = repo.ui.configlist("phrevset", "callsign")

    if callsign not in repo_callsigns:
        raise error.Abort(
            "Diff callsign '%s' is different from repo"
            " callsigns '%s'" % (callsign, repo_callsigns)
        )

    return match.group("id")


@util.lrucachefunc
def diffidtonode(repo, diffid):
    """Return node that matches a given Differential ID or None.

    The node might exist or not exist in the repo.
    This function does not raise.
    """

    repo_callsigns = repo.ui.configlist("phrevset", "callsign")
    if not repo_callsigns:
        msg = _("phrevset.callsign is not set - doing a linear search\n")
        hint = _("This will be slow if the diff was not committed recently\n")
        repo.ui.warn(msg)
        repo.ui.warn(hint)
        node = localgetdiff(repo, diffid)
        if node is None:
            repo.ui.warn(_("Could not find diff D%s in changelog\n") % diffid)
        return node

    node, resp = search(repo, diffid)

    if node is not None:
        # The log walk found the diff, nothing more to do
        return node

    if resp is None:
        # The graphql query finished but didn't return anything
        return None

    vcs = resp.get("source_control_system")
    localreponame = repo.ui.config("remotefilelog", "reponame")
    diffreponame = None

    # If already committed, prefer the commit that went to our local
    # repo to better handle the case when a diff was committed to
    # multiple repos.
    rev = resp["commits"].get(localreponame, None)
    if rev:
        diffreponame = localreponame
    else:
        repository = resp.get("repository")
        if repository is not None:
            diffreponame = repository.get("scm_name")
        if diffreponame in repo.ui.configlist("phrevset", "aliases"):
            diffreponame = localreponame

    if not util.istest() and (diffreponame != localreponame):
        raise error.Abort(
            "D%s is for repo '%s', not this repo ('%s')"
            % (diffid, diffreponame, localreponame)
        )

    repo.ui.debug("[diffrev] VCS is %s\n" % vcs)

    if vcs == "git":
        if not rev:
            rev = parsedesc(repo, resp, ignoreparsefailure=False)

        repo.ui.debug("[diffrev] GIT rev is %s\n" % rev)

        peerpath = repo.ui.expandpath("default")
        remoterepo = hg.peer(repo, {}, peerpath)
        remoterev = remoterepo.lookup("_gitlookup_git_%s" % rev)

        repo.ui.debug("[diffrev] HG rev is %s\n" % hex(remoterev))
        if not remoterev:
            repo.ui.debug("[diffrev] Falling back to linear search\n")
            node = localgetdiff(repo, diffid)
            if node is None:
                repo.ui.warn(_("Could not find diff D%s in changelog\n") % diffid)

            return node

        return remoterev

    elif vcs == "hg":
        if not rev:
            rev = parsedesc(repo, resp, ignoreparsefailure=True)

        if rev:
            # The response from phabricator contains a changeset ID.
            # Convert it back to a node.
            try:
                return repo[rev].node()
            except error.RepoLookupError:
                # TODO: 's/svnrev/globalrev' after turning off Subversion
                # servers. We will know about this when we remove the `svnrev`
                # revset.
                #
                # Unfortunately the rev can also be a svnrev/globalrev :(.
                if rev.isdigit():
                    try:
                        return list(repo.nodes("svnrev(%s)" % rev))[0]
                    except (IndexError, error.RepoLookupError):
                        pass

                if len(rev) == len(nullhex):
                    return bin(rev)
                else:
                    return None

        # commit is still local, get its hash

        try:
            props = resp["phabricator_version_properties"]["edges"]
            commits = {}
            for prop in props:
                if prop["node"]["property_name"] == "local:commits":
                    commits = json.loads(prop["node"]["property_value"])
            hexnodes = [c["commit"] for c in commits.values()]
        except (AttributeError, IndexError, KeyError):
            hexnodes = []

        # find a better alternative of the commit hash specified in
        # graphql response by looking up successors.
        for hexnode in hexnodes:
            if len(hexnode) != len(nullhex):
                continue

            node = bin(hexnode)
            unfi = repo
            if node in unfi:
                # Find a successor.
                successors = list(
                    unfi.nodes("last(successors(%n)-%n-obsolete())", node, node)
                )
                if successors:
                    return successors[0]
            return node

        # local:commits is empty
        return None

    else:
        if not vcs:
            msg = (
                "D%s does not have an associated version control system\n"
                "You can view the diff at https:///our.internmc.facebook.com/intern/diff/D%s\n"
            )
            repo.ui.warn(msg % (diffid, diffid))

            return None
        else:
            repo.ui.warn(
                _("Conduit returned unknown sourceControlSystem: '%s'\n") % vcs
            )

            return None


def _lookupname(repo, name):
    if name.startswith("D") and name[1:].isdigit():
        diffid = name[1:]
        node = diffidtonode(repo, diffid)
        if node is not None and node in repo:
            return [node]
    return []


@namespacepredicate("phrevset", priority=70)
def _getnamespace(_repo) -> namespace:
    return namespaces.namespace(
        listnames=lambda repo: [], namemap=_lookupname, nodemap=lambda repo, node: []
    )


@autopullpredicate("phrevset", priority=70, rewritepullrev=True)
def _autopullphabdiff(
    repo, name, rewritepullrev: bool = False
) -> Optional[pullattempt]:
    # Automation should use explicit commit hashes and do not depend on the
    # Dxxx autopull behavior.
    if repo.ui.plain():
        return

    # Phrevset autopull is disabled.
    if not repo.ui.configbool("phrevset", "autopull"):
        return

    if (
        name.startswith("D")
        and name[1:].isdigit()
        and (rewritepullrev or name not in repo)
    ):
        diffid = name[1:]
        node = diffidtonode(repo, diffid)
        if node and (rewritepullrev or node not in repo):
            # Attempt to pull it. This also rewrites "pull -r Dxxx" to "pull -r
            # HASH".
            friendlyname = "D%s (%s)" % (diffid, hex(node))
            return autopull.pullattempt(headnodes=[node], friendlyname=friendlyname)
