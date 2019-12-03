# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# fbconduit.py
# An extension to query remote servers for extra information via conduit RPC

import json
import re
from urllib import urlencode

from edenscm.mercurial import (
    extensions,
    namespaces,
    node,
    registrar,
    revset,
    smartset,
    templater,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin
from edenscm.mercurial.pycompat import range
from edenscm.mercurial.util import httplib

from .extlib.phabricator import arcconfig, graphql


namespacepredicate = registrar.namespacepredicate()

conduit_host = None
conduit_path = None
conduit_protocol = None
connection = None

DEFAULT_TIMEOUT = 60
MAX_CONNECT_RETRIES = 3


class ConduitError(Exception):
    pass


class HttpError(Exception):
    pass


githashre = re.compile("g([0-9a-f]{40})")
svnrevre = re.compile("^r[A-Z]+(\d+)$")
phabhashre = re.compile("^r([A-Z]+)([0-9a-f]{12,40})$")


def uisetup(ui):
    if not conduit_config(ui):
        ui.warn(_("No conduit host specified in config; disabling fbconduit\n"))
        return

    def _globalrevswrapper(loaded):
        if loaded:
            globalrevsmod = extensions.find("globalrevs")
            extensions.wrapfunction(
                globalrevsmod, "_lookupglobalrev", _scmquerylookupglobalrev
            )

    if ui.configbool("globalrevs", "scmquerylookup"):
        extensions.afterloaded("globalrevs", _globalrevswrapper)

    revset.symbols["gitnode"] = gitnode
    gitnode._weight = 10


def conduit_config(ui, host=None, path=None, protocol=None):
    global conduit_host, conduit_path, conduit_protocol
    conduit_host = host or ui.config("fbconduit", "host")
    conduit_path = path or ui.config("fbconduit", "path")
    conduit_protocol = protocol or ui.config("fbconduit", "protocol")
    if conduit_host is None:
        return False

    if conduit_protocol is None:
        conduit_protocol = "https"

    return True


def call_conduit(method, timeout=DEFAULT_TIMEOUT, **kwargs):
    global connection, conduit_host, conduit_path, conduit_protocol

    # start connection
    # TODO: move to python-requests
    if connection is None:
        if conduit_protocol == "https":
            connection = httplib.HTTPSConnection(conduit_host, timeout=timeout)
        elif conduit_protocol == "http":
            connection = httplib.HTTPConnection(conduit_host, timeout=timeout)

    # send request
    path = conduit_path + method
    args = urlencode({"params": json.dumps(kwargs)})
    headers = {
        "Connection": "Keep-Alive",
        "Content-Type": "application/x-www-form-urlencoded",
    }
    e = None
    for attempt in range(MAX_CONNECT_RETRIES):
        try:
            connection.request("POST", path, args, headers)
            break
        except httplib.HTTPException:
            connection.connect()
    if e:
        raise e

    # read http response
    response = connection.getresponse()
    if response.status != 200:
        raise HttpError(response.reason)
    result = response.read()

    # strip jsonp header and parse
    assert result.startswith("for(;;);")
    result = json.loads(result[8:])

    # check for conduit errors
    if result["error_code"]:
        raise ConduitError(result["error_info"])

    # return RPC result
    return result["result"]

    # don't close the connection b/c we want to avoid the connection overhead


@templater.templatefunc("mirrornode")
def mirrornode(ctx, mapping, args):
    """template: find this commit in other repositories"""

    reponame = mapping["repo"].ui.config("fbconduit", "reponame")
    if not reponame:
        # We don't know who we are, so we can't ask for a translation
        return ""

    if mapping["ctx"].mutable():
        # Local commits don't have translations
        return ""

    node = mapping["ctx"].hex()
    args = [f(ctx, mapping, a) for f, a in args]
    if len(args) == 1:
        torepo, totype = reponame, args[0]
    else:
        torepo, totype = args

    try:
        client = graphql.Client(repo=mapping["repo"])
        return client.getmirroredrev(reponame, "hg", torepo, totype, node)
    except arcconfig.ArcConfigError:
        mapping["repo"].ui.warn(_("couldn't read .arcconfig or .arcrc"))
        return ""
    except graphql.ClientError as e:
        mapping["repo"].ui.warn((str(e.msg) + "\n"))
        return ""


templatekeyword = registrar.templatekeyword()


@templatekeyword("gitnode")
def showgitnode(repo, ctx, templ, **args):
    """Return the git revision corresponding to a given hg rev"""
    reponame = repo.ui.config("fbconduit", "reponame")
    if not reponame:
        # We don't know who we are, so we can't ask for a translation
        return ""
    backingrepos = repo.ui.configlist("fbconduit", "backingrepos", default=[reponame])

    if ctx.mutable():
        # Local commits don't have translations
        return ""

    matches = []
    for backingrepo in backingrepos:
        try:
            client = graphql.Client(repo=repo)
            githash = client.getmirroredrev(
                reponame, "hg", backingrepo, "git", ctx.hex()
            )
            if githash != "":
                matches.append((backingrepo, githash))
        except (graphql.ClientError, arcconfig.ArcConfigError):
            pass

    if len(matches) == 0:
        return ""
    elif len(backingrepos) == 1:
        return matches[0][1]
    else:
        # in case it's not clear, the sort() is to ensure the output is in a
        # deterministic order.
        matches.sort()
        return "; ".join(["{0}: {1}".format(*match) for match in matches])


def gitnode(repo, subset, x):
    """``gitnode(id)``
    Return the hg revision corresponding to a given git rev."""
    l = revset.getargs(x, 1, 1, _("id requires one argument"))
    n = revset.getstring(l[0], _("id requires a string"))

    reponame = repo.ui.config("fbconduit", "reponame")
    if not reponame:
        # We don't know who we are, so we can't ask for a translation
        return subset.filter(lambda r: False)
    backingrepos = repo.ui.configlist("fbconduit", "backingrepos", default=[reponame])

    lasterror = None
    hghash = None
    for backingrepo in backingrepos:
        try:
            client = graphql.Client(repo=repo)
            hghash = client.getmirroredrev(backingrepo, "git", reponame, "hg", n)
            if hghash != "":
                break
        except Exception as ex:
            lasterror = ex

    if not hghash:
        if lasterror:
            repo.ui.warn(
                ("Could not translate revision {0}: {1}\n".format(n, lasterror))
            )
        else:
            repo.ui.warn(("Could not translate revision {0}\n".format(n)))
        return subset.filter(lambda r: False)

    rn = repo[node.bin(hghash)].rev()
    return subset & smartset.baseset([rn])


@namespacepredicate("conduit", priority=70)
def _getnamespace(_repo):
    return namespaces.namespace(
        listnames=lambda repo: [], namemap=_phablookup, nodemap=lambda repo, node: []
    )


def _phablookup(repo, phabrev):
    # Is the given revset a phabricator hg hash (ie: rHGEXTaaacb34aacb34aa)
    cl = repo.changelog
    tonode = cl.node

    def gittohg(githash):
        return [tonode(rev) for rev in repo.revs("gitnode(%s)" % githash)]

    phabmatch = phabhashre.match(phabrev)
    if phabmatch:
        phabrepo = phabmatch.group(1)
        phabhash = phabmatch.group(2)

        # The hash may be a git hash
        if phabrepo in repo.ui.configlist("fbconduit", "gitcallsigns", []):
            return gittohg(phabhash)

        return [repo[phabhash].node()]

    # TODO: 's/svnrev/globalrev' after turning off Subversion servers. We will
    # know about this when we remove the `svnrev` revset.
    svnrevmatch = svnrevre.match(phabrev)
    if svnrevmatch is not None:
        svnrev = svnrevmatch.group(1)
        return [tonode(rev) for rev in repo.revs("svnrev(%s)" % svnrev)]

    m = githashre.match(phabrev)
    if m is not None:
        githash = m.group(1)
        if len(githash) == 40:
            return gittohg(githash)
        else:
            return []


def _scmquerylookupglobalrev(orig, repo, rev):
    reponame = repo.ui.config("fbconduit", "reponame")
    if reponame:
        try:
            client = graphql.Client(repo=repo)
            hghash = str(
                client.getmirroredrev(reponame, "globalrev", reponame, "hg", str(rev))
            )
            matchedrevs = []
            if hghash:
                matchedrevs.append(bin(hghash))
            return matchedrevs
        except Exception:
            pass

    return orig(repo, rev)
