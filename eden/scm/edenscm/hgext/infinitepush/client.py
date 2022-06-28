# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re

from edenscm.mercurial import (
    autopull,
    bundle2,
    commands,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    hg,
    peer,
    phases,
    pycompat,
    registrar,
    scmutil,
    util,
    wireproto,
)
from edenscm.mercurial.i18n import _

from . import bookmarks, constants
from .constants import pathname


_maybehash = re.compile(r"^[a-f0-9]+$").search
# Technically it can still be a bookmark, but we consider it unlikely
_definitelyhash = re.compile(r"^[a-f0-9]{40}$").search


configtable = {}
configitem = registrar.configitem(configtable)
# Use the http Edenapi protocol to fetch bookmarks
configitem("infinitepush", "httpbookmarks", default=True)


def extsetup(ui):
    entry = extensions.wrapcommand(commands.table, "push", _push)
    # Don't add the 'to' arg if it already exists
    if not any(a for a in entry[1] if a[1] == "to"):
        entry[1].append(("", "to", "", _("push revs to this bookmark")))

    if not any(a for a in entry[1] if a[1] == "non-forward-move"):
        entry[1].append(
            (
                "",
                "non-forward-move",
                None,
                _("allows moving a remote bookmark to an " "arbitrary place"),
            )
        )

    if not any(a for a in entry[1] if a[1] == "create"):
        entry[1].append(("", "create", None, _("create a new remote bookmark")))

    entry[1].append(
        ("", "bundle-store", None, _("force push to go to bundle store (EXPERIMENTAL)"))
    )

    bookcmd = extensions.wrapcommand(commands.table, "bookmarks", _bookmarks)
    bookcmd[1].append(
        (
            "",
            "list-remote",
            None,
            "list remote bookmarks. "
            "Positional arguments are interpreted as wildcard patterns. "
            "Only allowed wildcard is '*' in the end of the pattern. "
            "If no positional arguments are specified then it will list "
            'the most "important" remote bookmarks. '
            "Otherwise it will list remote bookmarks "
            "that match at least one pattern "
            "",
        )
    )
    bookcmd[1].append(
        ("", "remote-path", "", "name of the remote path to list the bookmarks")
    )

    extensions.wrapcommand(commands.table, "pull", _pull)
    extensions.wrapfunction(bundle2, "_addpartsfromopts", _addpartsfromopts)

    wireproto.wirepeer.knownnodes = knownnodes

    # Move infinitepush part before pushrebase part
    # to avoid generation of both parts.
    partorder = exchange.b2partsgenorder
    index = partorder.index("changeset")
    if constants.pushrebaseparttype in partorder:
        index = min(index, partorder.index(constants.pushrebaseparttype))
    partorder.insert(
        index, partorder.pop(partorder.index(constants.scratchbranchparttype))
    )


def _push(orig, ui, repo, dest=None, *args, **opts):
    # use the original push logic to handle "no default path" case.
    if "default" not in repo.ui.paths:
        return orig(ui, repo, dest, *args, **opts)

    bookmark = opts.get("to") or ""
    create = opts.get("create") or False

    oldphasemove = None
    overrides = {
        ("experimental", "server-bundlestore-bookmark"): bookmark,
        ("experimental", "server-bundlestore-create"): create,
    }

    with ui.configoverride(
        overrides, "infinitepush"
    ), repo.wlock(), repo.lock(), repo.transaction("push"):
        scratchpush = opts.get("bundle_store")
        if repo._scratchbranchmatcher.match(bookmark):
            # We are pushing to a scratch bookmark.  Check that there is
            # exactly one revision that is being pushed (this will be the
            # new bookmarked node).
            revs = opts.get("rev")
            if revs:
                revs = [repo[r] for r in scmutil.revrange(repo, revs)]
            else:
                revs = [repo["."]]
            if len(revs) != 1:
                msg = _("--to requires exactly one commit to push")
                hint = _("use --rev HASH or omit --rev for current commit (.)")
                raise error.Abort(msg, hint=hint)

            # Put the bookmarked node hash in the bundle to avoid ambiguity.
            ui.setconfig(
                "experimental", "server-bundlestore-bookmarknode", revs[0].hex()
            )

            # If the bookmark destination is a public commit, then there will
            # be nothing to push.  We still need to send a changegroup part
            # to update the bookmark, so send the null rev instead.
            if not revs[0].mutable():
                opts["rev"] = ["null"]

            # Hack to fix interaction with remotenames. Remotenames push
            # '--to' bookmark to the server but we don't want to push scratch
            # bookmark to the server. Let's delete '--to' and '--create' and
            # also set allow_anon to True (because if --to is not set
            # remotenames will think that we are pushing anonymoush head)
            if "to" in opts:
                del opts["to"]
            if "create" in opts:
                del opts["create"]
            opts["allow_anon"] = True
            scratchpush = True
            # bundle2 can be sent back after push (for example, bundle2
            # containing `pushkey` part to update bookmarks)
            ui.setconfig("experimental", "bundle2.pushback", True)

        ui.setconfig(
            "experimental",
            "non-forward-move",
            opts.get("non_forward_move"),
            "--non-forward-move",
        )

        if scratchpush:
            ui.setconfig("experimental", "infinitepush-scratchpush", True)

            oldphasemove = extensions.wrapfunction(
                exchange, "_localphasemove", _phasemove
            )

        path = ui.paths.getpath(dest, default=(pathname.defaultpush, pathname.default))

        # Copy-paste from `push` command
        if not path:
            raise error.Abort(
                _("default repository not configured!"),
                hint=_("see 'hg help config.paths'"),
            )

        realdest = path.pushloc or path.loc
        if realdest.startswith("svn+") and scratchpush:
            raise error.Abort(
                "infinite push does not work with svn repo",
                hint="did you forget to `hg push default`?",
            )

        if scratchpush:
            ui.log(
                "infinitepush_destinations",
                dest=dest,
                real_dest=realdest,
                bookmark=bookmark,
            )

        # Remote scratch bookmarks will be deleted because remotenames doesn't
        # know about them. Let's save it before push and restore after
        remotescratchbookmarks = bookmarks.readremotebookmarks(ui, repo, realdest)
        result = orig(ui, repo, realdest, *args, **opts)

        if bookmarks.remotebookmarksenabled(ui):
            if bookmark and scratchpush:
                other = hg.peer(repo, opts, realdest)
                fetchedbookmarks = other.listkeyspatterns(
                    "bookmarks", patterns=[bookmark]
                )
                remotescratchbookmarks.update(fetchedbookmarks)
            bookmarks.saveremotebookmarks(repo, remotescratchbookmarks, realdest)
    if oldphasemove:
        exchange._localphasemove = oldphasemove
    return result


def _phasemove(orig, pushop, nodes, phase=phases.public):
    """prevent commits from being marked public

    Since these are going to a scratch branch, they aren't really being
    published."""

    if phase != phases.public:
        orig(pushop, nodes, phase)


def _bookmarks(orig, ui, repo, *names, **opts):
    pattern = opts.get("list_remote")
    delete = opts.get("delete")
    remotepath = opts.get("remote_path")
    path = ui.paths.getpath(remotepath or None, default=(pathname.default,))

    if pattern:
        destpath = path.pushloc or path.loc
        other = hg.peer(repo, opts, destpath)
        if not names:
            raise error.Abort(
                "--list-remote requires a bookmark pattern",
                hint='use "hg book" to get a list of your local bookmarks',
            )
        else:
            # prefix bookmark listing is not yet supported by Edenapi.
            usehttp = repo.ui.configbool("infinitepush", "httpbookmarks") and not any(
                n.endswith("*") for n in names
            )

            if usehttp:
                fetchedbookmarks = _http_bookmark_fetch(repo, names)
            else:
                fetchedbookmarks = other.listkeyspatterns("bookmarks", patterns=names)
        _showbookmarks(ui, fetchedbookmarks, **opts)
        return
    elif delete and "remotenames" in extensions._extensions:
        with repo.wlock(), repo.lock(), repo.transaction("bookmarks"):
            existing_local_bms = set(repo._bookmarks.keys())
            scratch_bms = []
            other_bms = []
            for name in names:
                if (
                    repo._scratchbranchmatcher.match(name)
                    and name not in existing_local_bms
                ):
                    scratch_bms.append(name)
                else:
                    other_bms.append(name)

            if len(scratch_bms) > 0:
                if remotepath == "":
                    remotepath = pathname.default
                bookmarks.deleteremotebookmarks(ui, repo, remotepath, scratch_bms)

            if len(other_bms) > 0 or len(scratch_bms) == 0:
                return orig(ui, repo, *other_bms, **opts)
    else:
        return orig(ui, repo, *names, **opts)


def _showbookmarks(ui, remotebookmarks, **opts):
    # Copy-paste from commands.py
    fm = ui.formatter("bookmarks", opts)
    for bmark, n in sorted(pycompat.iteritems(remotebookmarks)):
        fm.startitem()
        if not ui.quiet:
            fm.plain("   ")
        fm.write("bookmark", "%s", bmark)
        pad = " " * (25 - encoding.colwidth(bmark))
        fm.condwrite(not ui.quiet, "node", pad + " %s", n)
        fm.plain("\n")
    fm.end()


def _http_bookmark_fetch(repo, names):
    bookmarks = repo.edenapi.bookmarks(names)
    return util.sortdict(((bm, n) for (bm, n) in bookmarks.items() if n is not None))


def _pull(orig, ui, repo, source="default", **opts):
    return _dopull(orig, ui, repo, source, **opts)


def _dopull(orig, ui, repo, source="default", **opts):
    # Copy paste from `pull` command
    source, branches = hg.parseurl(ui.expandpath(source), opts.get("branch"))

    scratchbookmarks = {}
    unfi = repo
    unknownnodes = []
    pullbookmarks = opts.get("bookmark") or []
    if opts.get("rev", None):
        opts["rev"] = autopull.rewritepullrevs(repo, opts["rev"])

    for rev in opts.get("rev", []):
        if repo._scratchbranchmatcher.match(rev):
            # rev is a scratch bookmark, treat it as a bookmark
            pullbookmarks.append(rev)
        elif rev not in unfi:
            unknownnodes.append(rev)
    if pullbookmarks:
        realbookmarks = []
        revs = opts.get("rev") or []
        for bookmark in pullbookmarks:
            if repo._scratchbranchmatcher.match(bookmark):
                # rev is not known yet
                # it will be fetched with listkeyspatterns next
                scratchbookmarks[bookmark] = "REVTOFETCH"
            else:
                realbookmarks.append(bookmark)

        if scratchbookmarks:
            other = hg.peer(repo, opts, source)
            fetchedbookmarks = other.listkeyspatterns(
                "bookmarks", patterns=scratchbookmarks
            )
            for bookmark in scratchbookmarks:
                if bookmark not in fetchedbookmarks:
                    raise error.Abort("remote bookmark %s not found!" % bookmark)
                scratchbookmarks[bookmark] = fetchedbookmarks[bookmark]
                revs.append(fetchedbookmarks[bookmark])
        opts["bookmark"] = realbookmarks
        opts["rev"] = [rev for rev in revs if rev not in scratchbookmarks]

    if scratchbookmarks or unknownnodes:
        # Set anyincoming to True
        extensions.wrapfunction(discovery, "findcommonincoming", _findcommonincoming)
    try:
        # Remote scratch bookmarks will be deleted because remotenames doesn't
        # know about them. Let's save it before pull and restore after
        remotescratchbookmarks = bookmarks.readremotebookmarks(ui, repo, source)
        result = orig(ui, repo, source, **opts)
        # TODO(stash): race condition is possible
        # if scratch bookmarks was updated right after orig.
        # But that's unlikely and shouldn't be harmful.
        with repo.wlock(), repo.lock(), repo.transaction("pull"):
            if bookmarks.remotebookmarksenabled(ui):
                remotescratchbookmarks.update(scratchbookmarks)
                bookmarks.saveremotebookmarks(repo, remotescratchbookmarks, source)
            else:
                bookmarks.savelocalbookmarks(repo, scratchbookmarks)
        return result
    finally:
        if scratchbookmarks:
            extensions.unwrapfunction(discovery, "findcommonincoming")


def _findcommonincoming(orig, *args, **kwargs):
    common, inc, remoteheads = orig(*args, **kwargs)
    return common, True, remoteheads


def _tryhoist(ui, remotebookmark):
    """returns a (possibly remote) bookmark with hoisted part removed

    Remotenames extension has a 'hoist' config that allows to use remote
    bookmarks without specifying remote path. For example, 'hg update master'
    works as well as 'hg update remote/master'. We want to allow the same in
    infinitepush.
    """

    if bookmarks.remotebookmarksenabled(ui):
        hoist = ui.config("remotenames", "hoist") + "/"
        if remotebookmark.startswith(hoist):
            return remotebookmark[len(hoist) :]
    return remotebookmark


def _addpartsfromopts(orig, ui, repo, bundler, *args, **kwargs):
    """adds a stream level part to bundle2 storing whether this is an
    infinitepush bundle or not"""
    if ui.configbool("infinitepush", "bundle-stream", False):
        bundler.addparam("infinitepush", True)
    return orig(ui, repo, bundler, *args, **kwargs)


@peer.batchable
def knownnodes(self, nodes):
    f = peer.future()
    yield {"nodes": wireproto.encodelist(nodes)}, f
    d = f.value
    try:
        yield [bool(int(b)) for b in pycompat.decodeutf8(d)]
    except ValueError:
        error.Abort(error.ResponseError(_("unexpected response:"), d))
