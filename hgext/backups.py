# backups.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""display recently made backups to recover stripped changesets

This extension allows the listing and restoring of strip backups.

By default, this will display a warning if you have obsolescence marker creation
enabled (since most hg installations use either strip or markers, but not both).
To disable that warning, set ``backups.warnobsolescence`` to False.
"""

import glob
import os
import time

from mercurial import (
    bundle2,
    bundlerepo,
    cmdutil,
    commands,
    error,
    exchange,
    hg,
    lock as lockmod,
    obsolete,
    pycompat,
    registrar,
)
from mercurial.i18n import _
from mercurial.node import nullid, short

from . import pager


pager.attended.append("backups")

cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-hgext"
msgwithcreatermarkers = """Marker creation is enabled so no changeset should be
stripped unless you explicitly called hg strip. hg backups will show you the
stripped changesets. If you are trying to recover a changeset hidden from a
previous command, use hg journal to get its sha1 and you will be able to access
it directly without recovering a backup.\n\n"""
verbosetemplate = "{label('status.modified', node|short)} {desc|firstline}\n"


@command(
    "^backups",
    [("", "recover", "", "brings the specified changeset back into the repository")]
    + commands.logopts,
    _("hg backups [--recover HASH]"),
)
def backups(ui, repo, *pats, **opts):
    """lists the changesets available in backup bundles

    Without any arguments, this command prints a list of the changesets in each
    backup bundle.

    --recover takes a changeset hash and unbundles the first bundle that
    contains that hash, which puts that changeset back in your repository.

    --verbose will print the entire commit message and the bundle path for that
    backup.
    """
    supportsmarkers = obsolete.isenabled(repo, obsolete.createmarkersopt)
    if supportsmarkers and ui.configbool("backups", "warnobsolescence", True):
        # Warn users of obsolescence markers that they probably don't want to
        # use backups but reflog instead
        ui.warn(msgwithcreatermarkers)
    backuppath = repo.vfs.join("strip-backup")
    backups = filter(os.path.isfile, glob.glob(backuppath + "/*.hg"))
    backups.sort(key=lambda x: os.path.getmtime(x), reverse=True)

    opts["bundle"] = ""
    opts["force"] = None

    def display(other, chlist, displayer):
        limit = cmdutil.loglimit(opts)
        if opts.get("newest_first"):
            chlist.reverse()
        count = 0
        for n in chlist:
            if limit is not None and count >= limit:
                break
            parents = [p for p in other.changelog.parents(n) if p != nullid]
            if opts.get("no_merges") and len(parents) == 2:
                continue
            count += 1
            displayer.show(other[n])

    recovernode = opts.get("recover")
    if recovernode:
        if recovernode in repo:
            ui.warn(_("%s already exists in the repo\n") % recovernode)
            return
    else:
        msg = _(
            "Recover changesets using: hg backups --recover "
            "<changeset hash>\n\nAvailable backup changesets:"
        )
        ui.status(msg, label="status.removed")

    for backup in backups:
        # Much of this is copied from the hg incoming logic
        source = os.path.relpath(backup, pycompat.getcwd())
        source = ui.expandpath(source)
        source, branches = hg.parseurl(source, opts.get("branch"))
        try:
            other = hg.peer(repo, opts, source)
        except error.LookupError as ex:
            msg = _("\nwarning: unable to open bundle %s") % source
            hint = _("\n(missing parent rev %s)\n") % short(ex.name)
            ui.warn(msg)
            ui.warn(hint)
            continue
        revs, checkout = hg.addbranchrevs(repo, other, branches, opts.get("rev"))

        if revs:
            revs = [other.lookup(rev) for rev in revs]

        quiet = ui.quiet
        try:
            ui.quiet = True
            other, chlist, cleanupfn = bundlerepo.getremotechanges(
                ui, repo, other, revs, opts["bundle"], opts["force"]
            )
        except error.LookupError:
            continue
        finally:
            ui.quiet = quiet

        try:
            if chlist:
                if recovernode:
                    tr = lock = None
                    try:
                        lock = repo.lock()
                        if recovernode in other:
                            ui.status(_("Unbundling %s\n") % (recovernode))
                            f = hg.openpath(ui, source)
                            gen = exchange.readbundle(ui, f, source)
                            tr = repo.transaction("unbundle")
                            if not isinstance(gen, bundle2.unbundle20):
                                gen.apply(repo, "unbundle", "bundle:" + source)
                            if isinstance(gen, bundle2.unbundle20):
                                bundle2.applybundle(
                                    repo,
                                    gen,
                                    tr,
                                    source="unbundle",
                                    url="bundle:" + source,
                                )
                            tr.close()
                            break
                    finally:
                        lockmod.release(lock, tr)
                else:
                    backupdate = os.path.getmtime(source)
                    backupdate = time.strftime(
                        "%a %H:%M, %Y-%m-%d", time.localtime(backupdate)
                    )
                    ui.status("\n%s\n" % (backupdate.ljust(50)))
                    if not ui.verbose:
                        opts["template"] = verbosetemplate
                    else:
                        ui.status("%s%s\n" % ("bundle:".ljust(13), source))
                    displayer = cmdutil.show_changeset(ui, other, opts, False)
                    display(other, chlist, displayer)
                    displayer.close()
        finally:
            cleanupfn()
