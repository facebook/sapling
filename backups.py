# backups.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import extensions, util, cmdutil, commands, error, bundlerepo
from mercurial import hg, time, changegroup
from mercurial.extensions import wrapfunction
from hgext import pager
from mercurial.node import hex, nullrev, nullid
from mercurial.i18n import _
import errno, os, re, glob

pager.attended.append('backups')

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

@command('^backups', [
    ('', 'recover', '', 'brings the specified commit back into the repository')
    ] + commands.logopts, _('hg backups [--recover HASH]'))
def backups(ui, repo, *pats, **opts):
    '''lists the commits available in backup bundles 

    Without any arguments, this command prints a list of the commits in each
    backup bundle.

    --recover takes a commit hash and unbundles the first bundle that contains
    that commit hash, which puts that commit back in your repository.

    --verbose will print the entire commit message and the bundle path for that
    backup.
    '''
    backuppath = repo.join("strip-backup")
    backups = filter(os.path.isfile, glob.glob(backuppath + "/*.hg"))
    backups.sort(key=lambda x: os.path.getmtime(x), reverse=True)

    opts['bundle'] = ''
    opts['force'] = None

    def display(other, chlist, displayer):
        limit = cmdutil.loglimit(opts)
        if opts.get('newest_first'):
            chlist.reverse()
        count = 0
        for n in chlist:
            if limit is not None and count >= limit:
                break
            parents = [p for p in other.changelog.parents(n) if p != nullid]
            if opts.get('no_merges') and len(parents) == 2:
                continue
            count += 1
            displayer.show(other[n])

    recovernode = opts.get('recover')
    if recovernode:
        if recovernode in repo:
            ui.warn("%s already exists in the repo\n" % recovernode)
            return
    else:
        ui.status("Recover commits using: hg backups <commit hash>\n", label="status.removed")

    for backup in backups:
        # Much of this is copied from the hg incoming logic
        source = os.path.relpath(backup, os.getcwd())
        source = ui.expandpath(source)
        source, branches = hg.parseurl(source, opts.get('branch'))
        other = hg.peer(repo, opts, source)
        revs, checkout = hg.addbranchrevs(repo, other, branches, opts.get('rev'))

        if revs:
            revs = [other.lookup(rev) for rev in revs]

        quiet = ui.quiet
        try:
            ui.quiet = True
            other, chlist, cleanupfn = bundlerepo.getremotechanges(ui, repo, other,
                                        revs, opts["bundle"], opts["force"])
        except error.LookupError:
            continue
        finally:
            ui.quiet = quiet

        try:
            if chlist:
                if recovernode:
                    if recovernode in other:
                        ui.status("Unbundling %s\n" % (recovernode))
                        f = hg.openpath(ui, source)
                        gen = changegroup.readbundle(f, source)
                        modheads = repo.addchangegroup(gen, 'unbundle', 'bundle:' + source)
                        break
                else:
                    backupdate = os.path.getmtime(source)
                    backupdate = time.strftime('%a %H:%M, %Y-%m-%d', time.localtime(backupdate))
                    ui.status("\n%s\n" % (backupdate.ljust(50)))
                    if not ui.verbose:
                        opts['template'] = "{label('status.modified', node|short)} {desc|firstline}\n"
                    else:
                        ui.status("%s%s\n" % ("bundle:".ljust(13), source))
                    displayer = cmdutil.show_changeset(ui, other, opts, False)
                    display(other, chlist, displayer)
                    displayer.close()
        finally:
            cleanupfn()
