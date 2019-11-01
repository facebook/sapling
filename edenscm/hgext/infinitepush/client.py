# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import re
import sys
import time

from edenscm.mercurial import (
    bundle2,
    cmdutil,
    commands,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    hg,
    hintutil,
    obsolete,
    peer,
    phases,
    pushkey,
    scmutil,
    ui as uimod,
    visibility,
    wireproto,
)
from edenscm.mercurial.i18n import _

from . import bookmarks, constants


_maybehash = re.compile(r"^[a-f0-9]+$").search


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
    extensions.wrapcommand(commands.table, "update", _update)

    extensions.wrapfunction(bundle2, "_addpartsfromopts", _addpartsfromopts)

    wireproto.wirepeer.listkeyspatterns = listkeyspatterns
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


def preparepush(ui, dest):
    # If the user is saying they want to push to "default", and this is a
    # scratch push, then we're going to go to the infinitepush destination
    # instead, if it exists. This ensures that as long as infinitepush and
    # infinitepush-other (see below) route to different places (respectively
    # Mercurial and Mononoke), then infinite pushes without a path OR with a
    # path of "default" will be routed to both of them. Put it another way: when
    # you do a scratch push, "default" means the infinitepush path.
    if dest == "default":
        try:
            return (True, ui.paths.getpath("infinitepush"))
        except error.RepoError:
            # Fallthrough to the next block.
            pass

    if dest is None or dest in ("default", "infinitepush"):
        path = ui.paths.getpath(
            dest, default=("infinitepush", "default-push", "default")
        )
        return (True, path)

    return (False, ui.paths.getpath(dest))


def _push(orig, ui, repo, dest=None, *args, **opts):
    bookmark = opts.get("to") or ""
    create = opts.get("create") or False

    oldphasemove = None
    overrides = {
        ("experimental", "server-bundlestore-bookmark"): bookmark,
        ("experimental", "server-bundlestore-create"): create,
    }

    with ui.configoverride(overrides, "infinitepush"):
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

        otherpath = None

        if scratchpush:
            ui.setconfig("experimental", "infinitepush-scratchpush", True)

            oldphasemove = extensions.wrapfunction(
                exchange, "_localphasemove", _phasemove
            )

            replicate, path = preparepush(ui, dest)

            # We'll replicate the push if the user intended their push to go to
            # the default infinitepush destination.
            if replicate:
                try:
                    otherpath = repo.ui.paths.getpath("infinitepush-other")
                except error.RepoError:
                    pass
        else:
            path = ui.paths.getpath(dest, default=("default-push", "default"))

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

        otherdest = otherpath and (otherpath.pushloc or otherpath.loc)

        if scratchpush:
            ui.log(
                "infinitepush_destinations",
                dest=dest,
                real_dest=realdest,
                other_dest=otherdest,
                bookmark=bookmark,
            )

        # Remote scratch bookmarks will be deleted because remotenames doesn't
        # know about them. Let's save it before push and restore after
        remotescratchbookmarks = bookmarks.readremotebookmarks(ui, repo, realdest)
        result = orig(ui, repo, realdest, *args, **opts)

        # If an alternate Infinitepush destination is specified, replicate the
        # push there. This ensures scratch bookmarks (and their commits) can
        # properly be replicated to Mononoke.

        if otherdest is not None and otherdest != realdest:
            m = _(
                "please wait while we replicate this push to an alternate repository\n"
            )
            ui.warn(m)
            # NOTE: We ignore the result here (which only represents whether
            # there were changes to land).
            orig(ui, repo, otherdest, *args, **opts)

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
    path = ui.paths.getpath(remotepath or None, default=("default"))
    if pattern:
        destpath = path.pushloc or path.loc
        other = hg.peer(repo, opts, destpath)
        if not names:
            raise error.Abort(
                "--list-remote requires a bookmark pattern",
                hint='use "hg book" to get a list of your local bookmarks',
            )
        else:
            fetchedbookmarks = other.listkeyspatterns("bookmarks", patterns=names)
        _showbookmarks(ui, fetchedbookmarks, **opts)
        return
    elif delete and "remotenames" in extensions._extensions:
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
                remotepath = "default"
            bookmarks.deleteremotebookmarks(ui, repo, remotepath, scratch_bms)

        if len(other_bms) > 0 or len(scratch_bms) == 0:
            return orig(ui, repo, *other_bms, **opts)
    else:
        return orig(ui, repo, *names, **opts)


def _showbookmarks(ui, remotebookmarks, **opts):
    # Copy-paste from commands.py
    fm = ui.formatter("bookmarks", opts)
    for bmark, n in sorted(remotebookmarks.iteritems()):
        fm.startitem()
        if not ui.quiet:
            fm.plain("   ")
        fm.write("bookmark", "%s", bmark)
        pad = " " * (25 - encoding.colwidth(bmark))
        fm.condwrite(not ui.quiet, "node", pad + " %s", n)
        fm.plain("\n")
    fm.end()


def _pull(orig, ui, repo, source="default", **opts):
    # If '-r' or '-B' option is set, then prefer to pull from 'infinitepush' path
    # if it exists. 'infinitepush' path has both infinitepush and non-infinitepush
    # revisions, so pulling from it is safer.
    # This is useful for dogfooding other hg backend that stores only public commits
    # (e.g. Mononoke)
    if opts.get("rev") or opts.get("bookmark"):
        with _resetinfinitepushpath(ui):
            return _dopull(orig, ui, repo, source, **opts)

    return _dopull(orig, ui, repo, source, **opts)


@contextlib.contextmanager
def _resetinfinitepushpath(ui):
    """
    Sets "default" path to "infinitepush" path and deletes "infinitepush" path.
    In some cases (e.g. when testing new hg backend which doesn't have commit cloud
    commits) we want to do normal `hg pull` from "default" path but `hg pull -r HASH`
    from "infinitepush" path if it's present. This is better than just setting
    another path because of "remotenames" extension. Pulling or pushing to
    another path will add lots of new remote bookmarks and that can be slow
    and slow down smartlog.
    """

    overrides = {}
    if "infinitepush" in ui.paths:
        overrides[("paths", "default")] = ui.paths["infinitepush"].loc
        overrides[("paths", "infinitepush")] = "!"
        with ui.configoverride(overrides, "infinitepush"):
            loc, sub = ui.configsuboptions("paths", "default")
            ui.paths["default"] = uimod.path(ui, "default", rawloc=loc, suboptions=sub)
            del ui.paths["infinitepush"]
            yield
    else:
        yield


def _dopull(orig, ui, repo, source="default", **opts):
    # Copy paste from `pull` command
    source, branches = hg.parseurl(ui.expandpath(source), opts.get("branch"))

    scratchbookmarks = {}
    unfi = repo.unfiltered()
    unknownnodes = []
    pullbookmarks = opts.get("bookmark") or []
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

    # Pulling revisions that were filtered results in a error.
    # Let's revive them.
    unfi = repo.unfiltered()
    torevive = []
    for rev in opts.get("rev", []):
        try:
            repo[rev]
        except error.FilteredRepoLookupError:
            torevive.append(rev)
        except error.RepoLookupError:
            pass
    obsolete.revive([unfi[r] for r in torevive])
    visibility.add(repo, [unfi[r].node() for r in torevive])

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


def _update(orig, ui, repo, node=None, rev=None, **opts):
    """commit cloud (infinitepush) extension for hg up
    `hg up` will access:
    * local repo
    * hidden commits
    * remote commits
    * commit cloud (infinitepush) storage
    """
    if rev and node:
        raise error.Abort(_("please specify just one revision"))

    unfi = repo.unfiltered()
    if not opts.get("date") and (rev or node) not in unfi:
        mayberemote = rev or node
        mayberemote = _tryhoist(ui, mayberemote)
        dopull = False
        kwargs = {}
        if repo._scratchbranchmatcher.match(mayberemote):
            dopull = True
            kwargs["bookmark"] = [mayberemote]
        elif _maybehash(mayberemote):
            dopull = True
            kwargs["rev"] = [mayberemote]

        if dopull:
            ui.warn(
                _("'%s' does not exist locally - looking for it remotely...\n")
                % mayberemote
            )
            # Try pulling node from remote repo
            pullstarttime = time.time()

            try:
                (pullcmd, pullopts) = cmdutil.getcmdanddefaultopts(
                    "pull", commands.table
                )
                pullopts.update(kwargs)
                # Prefer to pull from 'infinitepush' path if it exists.
                # 'infinitepush' path has both infinitepush and non-infinitepush
                # revisions, so pulling from it is safer.
                # This is useful for dogfooding other hg backend that stores
                # only public commits (e.g. Mononoke)
                with _resetinfinitepushpath(ui):
                    pullcmd(ui, unfi, **pullopts)
            except Exception:
                remoteerror = str(sys.exc_info()[1])
                replacements = {
                    "commitcloud.changeset": ("changeset:",),
                    "commitcloud.meta": ("date:", "summary:", "author:"),
                    "commitcloud.commitcloud": ("#commitcloud",),
                }
                for label, keywords in replacements.iteritems():
                    for kw in keywords:
                        remoteerror = remoteerror.replace(kw, ui.label(kw, label))

                ui.warn(_("pull failed: %s\n") % remoteerror)

                # User updates to own commit from Commit Cloud
                if ui.username() in remoteerror:
                    hintutil.trigger("commitcloud-sync-education", ui)
            else:
                ui.warn(_("'%s' found remotely\n") % mayberemote)
                pulltime = time.time() - pullstarttime
                ui.warn(_("pull finished in %.3f sec\n") % pulltime)

    try:
        return orig(ui, repo, node, rev, **opts)
    except Exception:
        # Show the triggered hints anyway
        hintutil.show(ui)
        raise


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
    """ adds a stream level part to bundle2 storing whether this is an
    infinitepush bundle or not """
    if ui.configbool("infinitepush", "bundle-stream", False):
        bundler.addparam("infinitepush", True)
    return orig(ui, repo, bundler, *args, **kwargs)


@peer.batchable
def listkeyspatterns(self, namespace, patterns):
    if not self.capable("pushkey"):
        yield {}, None
    f = peer.future()
    self.ui.debug(
        'preparing listkeys for "%s" with pattern "%s"\n' % (namespace, patterns)
    )
    yield {
        "namespace": encoding.fromlocal(namespace),
        "patterns": wireproto.encodelist(patterns),
    }, f
    d = f.value
    self.ui.debug('received listkey for "%s": %i bytes\n' % (namespace, len(d)))
    yield pushkey.decodekeys(d)


@peer.batchable
def knownnodes(self, nodes):
    f = peer.future()
    yield {"nodes": wireproto.encodelist(nodes)}, f
    d = f.value
    try:
        yield [bool(int(b)) for b in d]
    except ValueError:
        error.Abort(error.ResponseError(_("unexpected response:"), d))
