# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""integration with Meta internal code review systems

Features:
- Resolve Phabricator commit identities like "D1234" or "rFBS<hash>".
- Provide Phabricator / CI templates.
- Mark commits as "Landed" on pull.

Config::

    [pullcreatemarkers]
    # Make sure commits being hidden matches the commit hashes in
    # Phabricator. Locally modified commits won't be hidden.
    check-local-versions = true

    [phrevset]
    callsign = E
    # Only ask GraphQL. Do not scan the local commits (which do not scale).
    graphqlonly = True
    # Automatically pull Dxxx.
    autopull = True

    [fbcodereview]
    # Whether to automatically hide landed draft commits after "pull".
    hide-landed-commits = true
"""

import os
import re
import socket
import ssl
import sys
from typing import Any, List, Optional, Pattern, Sized

from sapling import (
    autopull,
    cmdutil,
    commands,
    error,
    extensions,
    json,
    mutation,
    namespaces,
    node,
    registrar,
    revset,
    scmutil,
    smartset,
    templatekw,
    templater,
    ui as uimod,
    util,
    visibility,
)
from sapling.autopull import pullattempt
from sapling.i18n import _, _n, _x
from sapling.namespaces import namespace
from sapling.node import bin, hex, nullhex, short
from sapling.templatekw import _hybrid

from .extlib.phabricator import arcconfig, diffprops, graphql

cmdtable = {}
command = registrar.command(cmdtable)

namespacepredicate = registrar.namespacepredicate()
templatekeyword = registrar.templatekeyword()
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

# e.g.: Grafted e8470334d2058106534ac7d72485e6bfaa76ca01
GRAFT_INFO_REGEX: Pattern[str] = re.compile("(?m)^(Grafted [a-f0-9]+)$")

# Pattern for parsing diff ID and version (e.g., D1234567, D1234567V1, D1234567V1.2, D1234567v1)
DIFFID_VERSION_REGEX: Pattern[str] = re.compile(r"^D(\d+)(?:([Vv]\d+(?:\.\d+)?))?$")

DEFAULT_TIMEOUT = 60
MAX_CONNECT_RETRIES = 3
COMMITTEDSTATUS = "Committed"

githashre: Pattern[str] = re.compile(r"g([0-9a-f]{40})")
svnrevre: Pattern[str] = re.compile(r"^r[A-Z]+(\d+)$")
phabhashre: Pattern[str] = re.compile(r"^r([A-Z]+)([0-9a-f]{12,40})$")


@templatekeyword("phabdiff")
def showphabdiff(repo, ctx, templ, **args) -> str:
    """String. Return the phabricator diff id for a given @prog@ rev."""
    descr = ctx.description()
    revision = diffprops.parserevfromcommitmsg(descr)
    return "D" + revision if revision else ""


@templatekeyword("tasks")
def showtasks(**args) -> _hybrid:
    """String. Return the tasks associated with given @prog@ rev."""
    tasks = []
    descr = args["ctx"].description()
    match = re.search(r"Tasks?([\s-]?ID)?:\s*?[tT\d ,]+", descr)
    if match:
        tasks = re.findall(r"\d+", match.group(0))
    return templatekw.showlist("task", tasks, args)


@templatekeyword("singlepublicbase")
def singlepublicbase(repo, ctx, templ, **args):
    """String. Return the public base commit hash."""
    base = repo.revs("max(::%n & public())", ctx.node())
    if len(base):
        return hex(repo[base.first()].node())
    return ""


@templatekeyword("reviewers")
def showreviewers(repo, ctx, templ, **args):
    """String. Return a space-separated list of diff reviewers for a given @prog@ rev."""
    if ctx.node() is None:
        # working copy - use committemplate.reviewers, which can be found at
        # templ.t.cache.
        props = templ.cache
        reviewersconfig = props.get("reviewers")
        if reviewersconfig:
            return cmdutil.rendertemplate(repo.ui, reviewersconfig, props)
        else:
            return None
    else:
        reviewers = []
        descr = ctx.description()
        match = re.search("Reviewers:(.*)", descr)
        if match:
            reviewers = list(filter(None, re.split(r"[\s,]", match.group(1))))
        args = args.copy()
        args["templ"] = " ".join(reviewers)
        return templatekw.showlist("reviewer", reviewers, args)


def makebackoutmessage(orig, repo, message: str, node):
    message = orig(repo, message, node)
    olddescription = repo.changelog.changelogrevision(node).description
    revision = diffprops.parserevfromcommitmsg(olddescription)
    if revision:
        message += "\n\nOriginal Phabricator Diff: D%s" % revision
    return message


def makegraftmessage(orig, repo, ctx, opts, from_paths, to_paths, from_repo):
    message, is_from_user = orig(repo, ctx, opts, from_paths, to_paths, from_repo)
    if not from_paths:
        return message, is_from_user

    if is_from_user:
        new_message = message
    else:
        # only keep the summary section
        new_message = cmdutil.extract_summary(ctx.repo().ui, message)
    if revision := diffprops.parserevfromcommitmsg(message):
        new_message = GRAFT_INFO_REGEX.sub(r"\1 (D%s)" % revision, new_message)
    return new_message, is_from_user


def extsetup(ui) -> None:
    extensions.wrapfunction(commands, "_makebackoutmessage", makebackoutmessage)
    extensions.wrapfunction(commands, "_makegraftmessage", makegraftmessage)

    smartset.prefetchtemplatekw.update(
        {
            "phabsignalstatus": ["phabstatus"],
            "phabstatus": ["phabstatus"],
            "syncstatus": ["phabstatus"],
            "phabcommit": ["phabstatus"],
        }
    )
    smartset.prefetchtable["phabstatus"] = _prefetch


def memoize(f):
    """
    NOTE: This is a hack
    if f args are like (a, b1, b2, b3) and returns [o1, o2, o3] where
    o1, o2, o3 are output of f respectively for (a, b1), (a, b2) and
    (a, b3) then we memoize f(a, b1, b2, b3)'s result but also
    f(a, b1) => o1 , f(a, b2) => o2 and f(a, b3) => o3.
    Example:

    >>> partialsum = lambda a, *b: [a + bn for bn in b]
    >>> partialsum = memoize(partialsum)

    Create a class that wraps the integer '3', otherwise we cannot add
    _phabstatuscache to it for the test
    >>> class IntWrapperClass(int):
    ...     def __new__(cls, *args, **kwargs):
    ...         return  super(IntWrapperClass, cls).__new__(cls, 3)

    >>> three = IntWrapperClass()
    >>> partialsum(three, 1, 2, 3)
    [4, 5, 6]

    As expected, we have 4 entries in the cache for a call like f(a, b, c, d)
    >>> print(three._phabstatuscache)
    {(3, 1, 2, 3): [4, 5, 6], (3, 1): [4], (3, 2): [5], (3, 3): [6]}
    """

    def helper(*args):
        repo = args[0]
        if not hasattr(repo, "_phabstatuscache"):
            repo._phabstatuscache = {}
        if args not in repo._phabstatuscache:
            u = f(*args)
            repo._phabstatuscache[args] = u
            if isinstance(u, list):
                revs = args[1:]
                for x, r in enumerate(revs):
                    repo._phabstatuscache[(repo, r)] = [u[x]]
        return repo._phabstatuscache[args]

    return helper


def _fail(repo, diffids: Sized, *msgs) -> List[str]:
    for msg in msgs:
        repo.ui.warn(msg)
    return ["Error"] * len(diffids)


@memoize
def getdiffstatus(repo, *diffid):
    """Perform a GraphQL request to get the diff status

    Returns status of the diff"""

    if not diffid:
        return []
    timeout = repo.ui.configint("ssl", "timeout", 10)
    signalstatus = repo.ui.configbool("ssl", "signal_status", True)
    batchsize = repo.ui.configint("fbcodereview", "max-diff-count", 50)

    try:
        client = graphql.Client(repodir=os.getcwd(), repo=repo)
        statuses = {}
        # Limit how many we request at once to avoid timeouts.
        # Use itertools.batched once we are on Python 3.12.
        for i in range(0, len(diffid), batchsize):
            statuses.update(
                client.getrevisioninfo(timeout, signalstatus, diffid[i : i + batchsize])
            )
    except arcconfig.ArcConfigError as ex:
        msg = _(
            "arcconfig configuration problem. No diff information can be provided.\n"
        )
        hint = _("Error info: %s\n") % str(ex)
        ret = _fail(repo, diffid, msg, hint)
        return ret
    except (graphql.ClientError, ssl.SSLError, socket.timeout) as ex:
        msg = _("Error talking to phabricator. No diff information can be provided.\n")
        hint = _("Error info: %s\n") % str(ex)
        ret = _fail(repo, diffid, msg, hint)
        return ret
    except ValueError as ex:
        msg = _(
            "Error decoding GraphQL response. No diff information can be provided.\n"
        )
        hint = _("Error info: %s\n") % str(ex)
        ret = _fail(repo, diffid, msg, hint)
        return ret

    # This makes the code more robust in case we don't learn about any
    # particular revision
    result = []
    for diff in diffid:
        matchingresponse = statuses.get(str(diff))
        if not matchingresponse:
            result.append("Error")
        else:
            result.append(matchingresponse)
    return result


def populateresponseforphab(repo, diffnum) -> None:
    """:populateresponse: Runs the memoization function
    for use of phabstatus and sync status
    """
    if not hasattr(repo, "_phabstatusrevs"):
        return

    if hasattr(repo, "_phabstatuscache") and (repo, diffnum) in repo._phabstatuscache:
        # We already have cached data for this diff
        return

    next_revs = repo._phabstatusrevs.peekahead()
    if repo._phabstatusrevs.done:
        # repo._phabstatusrevs doesn't have anything else to process.
        # Remove it so we will bail out earlier next time.
        del repo._phabstatusrevs

    alldiffnumbers = [getdiffnum(repo, repo[rev]) for rev in next_revs]
    okdiffnumbers = set(d for d in alldiffnumbers if d is not None)
    # Make sure we always include the requested diff number
    okdiffnumbers.add(diffnum)
    # To populate the cache, the result will be used by the templater
    getdiffstatus(repo, *okdiffnumbers)


@templatekeyword("phabstatus")
def showphabstatus(repo, ctx, templ, **args):
    """String. Return the diff approval status for a given @prog@ rev"""
    diffnum = getdiffnum(repo, ctx)
    if diffnum is None:
        return None
    populateresponseforphab(repo, diffnum)

    result = getdiffstatus(repo, diffnum)[0]
    if isinstance(result, dict) and "status" in result:
        landstatus = result.get("land_job_status")
        finalreviewstatus = result.get("needs_final_review_status")
        if landstatus == "LAND_JOB_RUNNING":
            return "Landing"
        elif landstatus == "LAND_RECENTLY_SUCCEEDED":
            return "Committing"
        elif landstatus == "LAND_RECENTLY_FAILED":
            return "Recently Failed to Land"
        elif finalreviewstatus == "NEEDED":
            return "Needs Final Review"
        else:
            return result.get("status")
    else:
        return "Error"


@templatekeyword("phabsignalstatus")
def showphabsignalstatus(repo, ctx, templ, **args):
    """String. Return the diff Signal status for a given @prog@ rev"""
    diffnum = getdiffnum(repo, ctx)
    if diffnum is None:
        return None
    populateresponseforphab(repo, diffnum)

    result = getdiffstatus(repo, diffnum)[0]
    if isinstance(result, dict):
        return result.get("signal_status")


@templatekeyword("phabcommit")
def showphabcommit(repo, ctx, templ, **args):
    """String. Return the remote commit in Phabricator
    if any
    """
    # local = ctx.hex()
    # Copied from showsyncstatus
    if not ctx.mutable():
        return None

    diffnum = getdiffnum(repo, ctx)
    if diffnum is None:
        return None

    populateresponseforphab(repo, diffnum)
    results = getdiffstatus(repo, diffnum)
    try:
        result = results[0]
        remote = result["hash"]
    except (IndexError, KeyError, ValueError, TypeError):
        # We got no result back, or it did not contain all required fields
        return None

    return remote


@templatekeyword("syncstatus")
def showsyncstatus(repo, ctx, templ, **args) -> Optional[str]:
    """String. Return whether the local revision is in sync
    with the remote (phabricator) revision
    """
    if not ctx.mutable():
        return None

    diffnum = getdiffnum(repo, ctx)
    if diffnum is None:
        return None

    populateresponseforphab(repo, diffnum)
    results = getdiffstatus(repo, diffnum)
    try:
        result = results[0]
        remote = result.get("hash")
        status = result["status"]
    except (IndexError, KeyError, ValueError, TypeError, AttributeError):
        # We got no result back, or it did not contain all required fields
        return "Error"

    local = ctx.hex()
    if local == remote:
        return "sync"
    elif status == "Committed":
        return "committed"
    else:
        return "unsync"


@templatekeyword("diffversion")
def showdiffversion(repo, ctx, templ, **args) -> Optional[str]:
    """String. Returns which phabricator diff version this commit
    is in sync with (if any)
    """
    if not ctx.mutable():
        return None

    diffnum = getdiffnum(repo, ctx)
    if diffnum is None:
        return None

    populateresponseforphab(repo, diffnum)
    results = getdiffstatus(repo, diffnum)
    try:
        result = results[0]
        remote = result["hash"]
        alldiffversions = result["diff_versions"]
    except (IndexError, KeyError, ValueError, TypeError):
        # We got no result back, or it did not contain all required fields
        return "Error"

    if not alldiffversions:
        return None

    local = ctx.hex()
    version = alldiffversions.get(local)
    if version is not None:
        if local == remote:
            version += " (latest)"
        return version
    for pred in mutation.allpredecessors(repo, [ctx.node()]):
        predhex = hex(pred)
        if predhex in alldiffversions:
            version = alldiffversions[predhex]
            if predhex == remote:
                version += " (latest + local changes)"
            else:
                version += " (+ local changes)"
            return version
    return None


def getdiffnum(repo, ctx):
    return diffprops.parserevfromcommitmsg(ctx.description())


def _prefetch(repo, ctxstream):
    peekahead = repo.ui.configint("phabstatus", "logpeekaheadlist", 30)
    for batch in util.eachslice(ctxstream, peekahead):
        cached = getattr(repo, "_phabstatuscache", {})
        diffids = [getdiffnum(repo, ctx) for ctx in batch]
        diffids = {i for i in diffids if i is not None and i not in cached}
        if diffids:
            repo.ui.debug("prefetch phabstatus for %r\n" % sorted(diffids))
            # @memorize writes results to repo._phabstatuscache
            getdiffstatus(repo, *diffids)
        for ctx in batch:
            yield ctx


def _isrevert(message, diffid):
    result = ("Revert D%s" % diffid) in message
    return result


def _cleanuplanded(repo, dryrun=False):
    """Query Phabricator about states of draft commits and optionally mark them
    as landed.

    This uses mutation and visibility directly.
    """
    ui = repo.ui
    # return empty dict if there are no remote bookmarks
    if not len(repo._remotenames["bookmarks"]):
        ui.status(_("no remote bookmarks, cleanup skipped.\n"))
        return {}

    difftodraft = _get_diff_to_draft(repo)
    query_result = _query_phabricator(
        repo, list(difftodraft.keys()), ["Closed", "Abandoned"]
    )
    if query_result is None:
        return None
    difftopublic, difftolocal, difftostatus = query_result
    mutationentries = []
    tohide = set()
    markedcount_landed = 0
    markedcount_abandoned = 0
    visible_heads = visibility.heads(repo)

    checklocalversions = ui.configbool("pullcreatemarkers", "check-local-versions")
    for diffid, draftnodes in sorted(difftodraft.items()):
        status = difftostatus.get(diffid)
        if not status:
            continue
        if status == "Closed":
            markedcount_landed += _process_landed(
                repo,
                diffid,
                draftnodes,
                difftopublic,
                difftolocal,
                checklocalversions,
                tohide,
                mutationentries,
            )
        elif status == "Abandoned":
            # filter out unhidable nodes
            draftnodes = {node for node in draftnodes if node in visible_heads}
            markedcount_abandoned += _process_abandoned(
                repo,
                diffid,
                draftnodes,
                difftolocal,
                checklocalversions,
                tohide,
            )

    if markedcount_landed:
        ui.status(
            _n(
                "marked %d commit as landed\n",
                "marked %d commits as landed\n",
                markedcount_landed,
            )
            % markedcount_landed
        )
    if markedcount_abandoned:
        ui.status(
            _n(
                "marked %d commit as abandoned\n",
                "marked %d commits as abandoned\n",
                markedcount_abandoned,
            )
            % markedcount_abandoned
        )
    _hide_commits(repo, tohide, mutationentries, dryrun)


def _get_diff_to_draft(repo):
    limit = repo.ui.configint("pullcreatemarkers", "diff-limit", 100)
    difftodraft = {}  # {str: {node}}
    for ctx in repo.set("sort(draft() - obsolete(), -rev)"):
        diffid = diffprops.parserevfromcommitmsg(ctx.description())  # str or None
        if diffid and not _isrevert(ctx.description(), diffid):
            difftodraft.setdefault(diffid, set()).add(ctx.node())
            # Bound the number of diffs we query from Phabricator.
            if len(difftodraft) >= limit:
                break
    return difftodraft


def _query_phabricator(repo, diffids, diff_status_list):
    ui = repo.ui
    try:
        client = graphql.Client(repo=repo)
    except arcconfig.ArcConfigLoadError:
        # Not all repos have arcconfig. If a repo doesn't have one, that's not
        # a fatal error.
        return
    except Exception as ex:
        ui.warn(
            _(
                "warning: failed to initialize GraphQL client (%r), not marking commits as landed\n"
            )
            % ex
        )
        return

    try:
        return client.getnodes(repo, diffids, diff_status_list)
    except Exception as ex:
        ui.warn(
            _(
                "warning: failed to read from Phabricator for landed commits (%r), not marking commits as landed\n"
            )
            % ex
        )


def _process_abandoned(
    repo,
    diffid,
    draftnodes,
    difftolocal,
    checklocalversions,
    tohide,
):
    ui = repo.ui
    if checklocalversions:
        draftnodes = draftnodes & difftolocal.get(diffid, set())
    draftnodestr = ", ".join(short(d) for d in sorted(draftnodes))
    if draftnodestr:
        ui.note(_("marking D%s (%s) as abandoned\n") % (diffid, draftnodestr))
    tohide |= set(draftnodes)
    return len(draftnodes)


def _process_landed(
    repo,
    diffid,
    draftnodes,
    difftopublic,
    difftolocal,
    checklocalversions,
    tohide,
    mutationentries,
):
    ui = repo.ui
    publicnode = difftopublic.get(diffid)
    if publicnode is None or publicnode not in repo:
        return 0
    # skip it if the local repo does not think it's a public commit.
    if not repo[publicnode].ispublic():
        return 0
    # sanity check - the public commit should have a sane commit message.
    if diffprops.parserevfromcommitmsg(repo[publicnode].description()) != diffid:
        return 0

    if checklocalversions:
        draftnodes = draftnodes & difftolocal.get(diffid, set())
    draftnodestr = ", ".join(
        short(d) for d in sorted(draftnodes)
    )  # making output deterministic
    ui.note(
        _("marking D%s (%s) as landed as %s\n")
        % (diffid, draftnodestr, short(publicnode))
    )
    for draftnode in draftnodes:
        tohide.add(draftnode)
        mutationentries.append(
            mutation.createsyntheticentry(repo, [draftnode], publicnode, "land")
        )

    return len(draftnodes)


def _hide_commits(repo, tohide, mutationentries, dryrun):
    if not tohide or not repo.ui.configbool("fbcodereview", "hide-landed-commits"):
        return

    repo.ui.note(_("hiding %d commits\n") % (len(tohide)))

    if dryrun:
        return

    with repo.lock(), repo.transaction("pullcreatemarkers"):
        # Any commit hash's added to the idmap in the earlier code will have
        # been dropped by the repo.invalidate() that happens at lock time.
        # Let's refetch those hashes now. If we don't then the
        # mutation/obsolete computation will fail to consider this mutation
        # marker, since it ignores markers for which we don't have the hash
        # for the mutation target.
        repo.changelog.filternodes(list(e.succ() for e in mutationentries))
        if mutation.enabled(repo):
            mutation.recordentries(repo, mutationentries, skipexisting=False)
        if visibility.tracking(repo):
            visibility.remove(repo, tohide)


@command("debugmarklanded", commands.dryrunopts)
def debugmarklanded(ui, repo, **opts):
    """query Phabricator and mark landed commits"""
    dryrun = opts.get("dry_run")
    _cleanuplanded(repo, dryrun=dryrun)
    if dryrun:
        ui.status(_("(this is a dry-run, nothing was actually done)\n"))


@command(
    "url",
    [
        ("r", "rev", "", _("revision"), _("REV")),
    ]
    + cmdutil.walkopts,
)
def url(ui, repo, *pats, **opts):
    """show url for the given files, or the current directory if no files are provided"""
    from urllib.parse import quote

    url_reponame = ui.config("fbscmquery", "reponame")
    url_template = ui.config("fbcodereview", "code-browser-url")
    if not url_reponame or not url_template:
        raise error.Abort(_("repo is not configured for showing URL"))
    ctx = scmutil.revsingle(repo, opts.get("rev"))
    m = scmutil.match(ctx, pats, opts)
    if not m.anypats():
        paths = m.files()
    else:
        paths = ctx.walk(m)

    if not paths:
        paths = [repo.getcwd()]

    for path in paths:
        url = url_template % {
            "repo_name": url_reponame,
            "path": quote(path),
            "node_hex": ctx.hex(),
        }
        ui.write(("%s\n" % url))


def uisetup(ui) -> None:
    def _globalrevswrapper(loaded):
        if loaded:
            globalrevsmod = extensions.find("globalrevs")
            extensions.wrapfunction(
                globalrevsmod, "_lookupglobalrev", _scmquerylookupglobalrev
            )

    if ui.configbool("globalrevs", "scmquerylookup") and not ui.configbool(
        "globalrevs", "edenapilookup"
    ):
        extensions.afterloaded("globalrevs", _globalrevswrapper)

    revset.symbols["gitnode"] = gitnode
    gitnode._weight = 10

    if ui.configbool("fbscmquery", "auto-username"):

        def _auto_username(orig, ui):
            try:
                client = graphql.Client(ui=ui)
                return client.get_username()
            except Exception:
                return None

        extensions.wrapfunction(uimod, "_auto_username", _auto_username)


def reposetup(ui, repo):
    repo.ui.setconfig(
        "hooks", "post-pull.marklanded", _get_shell_cmd(ui, ["debugmarklanded"])
    )


@templater.templatefunc("mirrornode")
def mirrornode(ctx, mapping, args):
    """template: find this commit in other repositories"""

    reponame = mapping["repo"].ui.config("fbscmquery", "reponame")
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
        mapping["repo"].ui.warn(_("couldn't read .arcconfig or .arcrc\n"))
        return ""
    except graphql.ClientError as e:
        mapping["repo"].ui.warn(_x(str(e) + "\n"))
        return ""


@templatekeyword("gitnode")
def showgitnode(repo, ctx, templ, **args):
    """Return the git revision corresponding to a given hg rev"""
    # Try reading from commit extra first.
    extra = ctx.extra()
    if "hg-git-rename-source" in extra:
        hexnode = extra.get("convert_revision")
        if hexnode:
            return hexnode
    reponame = repo.ui.config("fbscmquery", "reponame")
    if not reponame:
        # We don't know who we are, so we can't ask for a translation
        return ""
    backingrepos = repo.ui.configlist("fbscmquery", "backingrepos", default=[reponame])

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

    reponame = repo.ui.config("fbscmquery", "reponame")
    if not reponame:
        # We don't know who we are, so we can't ask for a translation
        return smartset.baseset([], repo=repo)
    backingrepos = repo.ui.configlist("fbscmquery", "backingrepos", default=[reponame])

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
            repo.ui.warn(_x("Could not translate revision {0}\n".format(n)))
        # If we don't have a valid hg hash, return an empty set
        return smartset.baseset([], repo=repo)

    rn = repo[node.bin(hghash)].rev()
    return subset & smartset.baseset([rn], repo=repo)


@namespacepredicate("conduit", priority=70)
def _conduit_namespace(_repo) -> namespace:
    return namespaces.namespace(
        listnames=lambda repo: [], namemap=_phablookup, nodemap=lambda repo, node: []
    )


def _phablookup(repo: "Any", phabrev: str) -> "List[bytes]":
    # Is the given revset a phabricator hg hash (ie: rHGEXTaaacb34aacb34aa)

    def gittohg(githash):
        return list(repo.nodes("gitnode(%s)" % githash))

    phabmatch = phabhashre.match(phabrev)
    if phabmatch:
        phabrepo = phabmatch.group(1)
        phabhash = phabmatch.group(2)

        # The hash may be a git hash
        if phabrepo in repo.ui.configlist("fbscmquery", "gitcallsigns", []):
            return gittohg(phabhash)

        return [repo[phabhash].node()]

    # TODO: 's/svnrev/globalrev' after turning off Subversion servers. We will
    # know about this when we remove the `svnrev` revset.
    svnrevmatch = svnrevre.match(phabrev)
    if svnrevmatch is not None:
        svnrev = svnrevmatch.group(1)
        return list(repo.nodes("svnrev(%s)" % svnrev))

    m = githashre.match(phabrev)
    if m is not None:
        githash = m.group(1)
        if len(githash) == 40:
            return gittohg(githash)

    return []


def _scmquerylookupglobalrev(orig, repo, rev):
    reponame = repo.ui.config("fbscmquery", "reponame")
    if reponame:
        try:
            client = graphql.Client(repo=repo)
            hghash = str(
                client.getmirroredrev(reponame, "GLOBAL_REV", reponame, "hg", str(rev))
            )
            matchedrevs = []
            if hghash:
                matchedrevs.append(bin(hghash))
            return matchedrevs
        except Exception as exc:
            repo.ui.warn(
                _("failed to lookup globalrev %s from scmquery: %s\n") % (rev, exc)
            )

    return orig(repo, rev)


@command(
    "debuggraphql",
    [
        ("", "query", "", _("GraphQL query to execute"), _("QUERY")),
        ("", "variables", "", _("variables to use in GraphQL query"), _("JSON")),
    ],
    norepo=True,
)
def debuggraphql(ui, *args, **opts):
    """Runs authenticated phabricator graphql queries, and returns output in JSON. Used by ISL."""
    try:
        client = graphql.Client(ui=ui)

        query = opts.get("query")
        if not query:
            raise ValueError("query must be provided")

        try:
            var = opts.get("variables") or "{}"
            variables = json.loads(var)
        except json.JSONDecodeError:
            raise ValueError("variables input is invalid JSON")

        result = client.graphqlquery(query, variables)
        ui.write(json.dumps(result), "\n")
    except Exception as e:
        err = str(e)
        ui.write(json.dumps({"error": err}), "\n")
        return 32


@command(
    "debuginternusername",
    [("u", "unixname", "", _("unixname to lookup"))],
    norepo=True,
)
def debuginternusername(ui, **opts):
    client = graphql.Client(ui=ui)
    unixname = opts.get("unixname") or None
    name = client.get_username(unixname=unixname)
    ui.write("%s\n" % name)


def graphqlgetdiff(repo, diffid, version=None):
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
    try:
        client = graphql.Client(repodir=os.getcwd(), repo=repo)
        return client.getdiffversion(timeout, diffid, version=version)
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
        fullargs=repr(sys.argv),
        feature="phrevset-full-changelog-scan",
    )
    for rev in repo.changelog.revs(start=len(repo.changelog), stop=0):
        matched = check(repo, rev, diffid)
        if matched is not None:
            return matched

    return None


def search(repo, diffid, version=None):
    """Perform a GraphQL query first. If it fails, fallback to local search.

    Returns (node, None) or (None, graphql_response) tuple.
    """

    repo.ui.debug("[diffrev] Starting graphql call\n")
    if repo.ui.configbool("phrevset", "graphqlonly") or version:
        return (None, graphqlgetdiff(repo, diffid, version=version))

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
    repo_callsigns = _get_callsigns(repo)

    if callsign not in repo_callsigns:
        raise error.Abort(
            "Diff callsign '%s' is different from repo"
            " callsigns '%s'" % (callsign, repo_callsigns)
        )

    return match.group("id")


@util.lrucachefunc
def diffidtonode(repo, diffid, localreponame=None, version=None):
    """Return node that matches a given Differential ID or None.

    The node might exist or not exist in the repo.
    This function does not raise.
    """

    repo_callsigns = _get_callsigns(repo)
    if not repo_callsigns:
        msg = _("phrevset.callsign is not set - doing a linear search\n")
        hint = _("This will be slow if the diff was not committed recently\n")
        repo.ui.warn(msg)
        repo.ui.warn(hint)
        node = localgetdiff(repo, diffid)
        if node is None:
            repo.ui.warn(_("Could not find diff D%s in changelog\n") % diffid)
        return node

    node, resp = search(repo, diffid, version=version)

    if node is not None:
        # The log walk found the diff, nothing more to do
        return node

    if resp is None:
        # The graphql query finished but didn't return anything
        return None

    vcs = resp.get("source_control_system")

    if localreponame is None:
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

    if not util.istest() and not _matchreponames(diffreponame, localreponame):
        megarepo_can_handle = extensions.isenabled(
            repo.ui, "megarepo"
        ) and diffreponame in repo.ui.configlist("megarepo", "transparent-lookup")

        if megarepo_can_handle:
            # megarepo extension might be able to translate diff/commit to
            # local repo - don't abort the entire command.
            return None
        else:
            raise error.Abort(
                "D%s is for repo '%s', not this repo ('%s')"
                % (diffid, diffreponame, localreponame)
            )

    repo.ui.debug("[diffrev] VCS is %s\n" % vcs)

    if vcs == "git" or vcs == "hg":
        if not rev:
            if version:
                # If we are looking for a specific version, we can't use the
                # "description" property to find the commit hash, as it only
                # contains the latest version.
                return None
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

        hexnodes = []
        try:
            commit_hash = resp.get("commit_hash_best_effort")
            hexnodes = [] if commit_hash is None else [commit_hash]
        except (AttributeError, IndexError, KeyError):
            pass

        # find a better alternative of the commit hash specified in
        # graphql response by looking up successors.
        for hexnode in hexnodes:
            if len(hexnode) != len(nullhex):
                continue

            node = bin(hexnode)
            unfi = repo
            if node in unfi:
                # Find latest successor whose description still links to the target diff id.
                # successors() skips hidden commits. We don't subtract obsolete() because we want to
                # return obsolete-but-visible commits (if that happens to be the newest local
                # version of a diff).
                for successor in unfi.nodes("sort(successors(%n)-%n,-rev)", node, node):
                    if (
                        diffprops.parserevfromcommitmsg(repo[successor].description())
                        == diffid
                    ):
                        return successor
            if (
                vcs == "git"
                and repo.ui.configbool("phrevset", "abort-if-git-diff-unavailable")
                and node not in repo
            ):
                raise error.Abort(
                    _(
                        "A more recent version (%s) of D%s was found in Phabricator, you might want to run `jf get D%s`"
                    )
                    % (hex(node)[:8], diffid, diffid)
                )
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


def _try_parse_diffid_version(name):
    """Parse names like D1234567V1 or D1234567 to a tuple of (diffid, version).

    Returns None if the name is not valid.

    >>> _try_parse_diffid_version("D1234567V1")
    ('1234567', 'V1')
    >>> _try_parse_diffid_version("D1234567V1.1")
    ('1234567', 'V1.1')
    >>> _try_parse_diffid_version("D1234567")
    ('1234567', None)
    >>> _try_parse_diffid_version("D1234567V")
    >>> _try_parse_diffid_version("D1234567V1.1.1")
    >>> _try_parse_diffid_version("D1234567v1")
    ('1234567', 'V1')
    >>> _try_parse_diffid_version("D1234567v1.1")
    ('1234567', 'V1.1')
    >>> _try_parse_diffid_version("D1234567")
    ('1234567', None)
    >>> _try_parse_diffid_version("D1234567v")
    >>> _try_parse_diffid_version("D1234567v1.1.1")
    >>> _try_parse_diffid_version("D1234567A1")
    """
    match = DIFFID_VERSION_REGEX.match(name)

    if not match:
        return None

    diffid = match.group(1)
    version = match.group(2)
    if version and version[0] == "v":
        version = "V" + version[1:]

    return (diffid, version)


def _lookupname(repo, name):
    if diffid_version := _try_parse_diffid_version(name):
        diffid, version = diffid_version
        node = diffidtonode(repo, diffid, version=version)
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
    if repo.ui.plain(feature="phrevset"):
        return

    # Phrevset autopull is disabled.
    if not repo.ui.configbool("phrevset", "autopull"):
        return

    if (diffid_version := _try_parse_diffid_version(name)) and (
        rewritepullrev or name not in repo
    ):
        diffid, version = diffid_version
        node = diffidtonode(repo, diffid, version=version)
        if node and (rewritepullrev or node not in repo):
            # Attempt to pull it. This also rewrites "pull -r Dxxx" to "pull -r
            # HASH".
            if version:
                friendlyname = "D%s%s (%s)" % (diffid, version, hex(node))
            else:
                friendlyname = "D%s (%s)" % (diffid, hex(node))
            return autopull.pullattempt(headnodes=[node], friendlyname=friendlyname)


def _get_callsigns(repo) -> List[str]:
    callsigns = repo.ui.configlist("phrevset", "callsign")
    if not callsigns:
        # Try to read from '.arcconfig'
        try:
            parsed = json.loads(repo["."][".arcconfig"].data())
            callsigns = [parsed["repository.callsign"]]
        except Exception:
            pass
    return callsigns


def _matchreponames(diffreponame: Optional[str], localreponame: Optional[str]) -> bool:
    """Makes sure two different repo names look mostly the same, ignoring `.git`
    suffixes and checking that suffixes considering repos names separated by
    slashes look the same. It's assumed that `localreponame` should be a longer
    version of `diffreponame`.

    >>> _matchreponames("bar.git", "foo/bar")
    True
    >>> _matchreponames("bar", "foo/bar.git")
    True
    >>> _matchreponames("afoo/bar", "foo/bar")
    False
    >>> _matchreponames("foo/bar", "bar")
    False
    >>> _matchreponames("w/x/y", "z/x/y")
    False
    """

    def _processreponame(reponame: str) -> List[str]:
        return (reponame or "").removesuffix(".git").split("/")

    diffreponame = _processreponame(diffreponame)
    localreponame = _processreponame(localreponame)
    dilen = len(diffreponame)
    lolen = len(localreponame)
    return dilen <= lolen and diffreponame[-dilen:] == localreponame[-dilen:]


def _get_shell_cmd(ui, args: List[str]) -> str:
    full_args = util.hgcmd()
    if ui.quiet:
        full_args.append("-q")
    if ui.verbose:
        full_args.append("-v")
    if ui.debugflag:
        full_args.append("--debug")
    full_args += args
    return " ".join(map(util.shellquote, full_args))
