# __init__.py - sqldirstate extension
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

testedwith = 'internal'

from sqldirstate import makedirstate, DBFILE, toflat, tosql, writefakedirstate

from mercurial import commands, error, extensions, cmdutil, localrepo, util
from mercurial.i18n import _
from mercurial.extensions import wrapfunction

def issqldirstate(repo):
    return util.safehasattr(repo, 'requirements') and \
        'sqldirstate' in repo.requirements

def wrapjournalfiles(orig, self):
    if issqldirstate(self):
        files = ()
        for vfs, filename in orig(self):
            if filename != 'journal.dirstate':
                files += ((vfs, filename),)

        if not self.ui.configbool('sqldirstate', 'skipbackups', True):
            files += ((self.vfs, 'journal.dirstate.sqlite3'),)
    else:
        files = orig(self)

    return files

def wrapdirstate(orig, self):
    ds = orig(self)
    if issqldirstate(self):
        ds.__class__ = makedirstate(ds.__class__)
        ds._sqlinit()
    return ds

def wrapnewreporequirements(orig, repo):
    reqs = orig(repo)
    if repo.ui.configbool('format', 'sqldirstate', False):
        reqs.add('sqldirstate')
    return reqs

def wrapshelveaborttransaction(orig, repo):
    if issqldirstate(repo):
        tr = repo.currenttransaction()
        repo.dirstate._writesqldirstate()
        tr.abort()
    else:
        return orig(repo)

def upgrade(ui, repo):
    if issqldirstate(repo):
        raise error.Abort('repo already has sqldirstate')
    wlock = repo.wlock()
    try:
        repo.dirstate._read()
        tosql(repo.dirstate)
        repo.requirements.add('sqldirstate')
        repo._writerequirements()
        writefakedirstate(repo.dirstate)
        del repo.dirstate

    finally:
        wlock.release()

def downgrade(ui, repo):
    if not issqldirstate(repo):
        raise error.Abort('repo doesn\'t have sqldirstate')
    wlock = repo.lock()
    try:
        toflat(repo.dirstate)
        repo.requirements.remove('sqldirstate')
        repo._writerequirements()
        repo.dirstate._opener.unlink('dirstate.sqlite3')
        del repo.dirstate
    finally:
        wlock.release()

def wrappull(orig, ui, repo, *args, **kwargs):
    if ui.configbool('sqldirstate', 'downgrade', False) and \
            issqldirstate(repo):
        ui.status(_('disabling sqldirstate...\n'))
        downgrade(ui, repo)
    elif ui.configbool('sqldirstate', 'upgrade', False) and \
            not issqldirstate(repo):
        ui.status(_('migrating your repo to sqldirstate which will make your '
                'hg commands faster...\n'))
        upgrade(ui, repo)

    return orig(ui, repo, *args, **kwargs)

def featuresetup(ui, supported):
    # don't die on seeing a repo with the sqldirstate requirement
    supported |= set(['sqldirstate'])

def extsetup(ui):
    localrepo.localrepository.featuresetupfuncs.add(featuresetup)
    wrapfunction(localrepo, 'newreporequirements',
                 wrapnewreporequirements)
    wrapfunction(localrepo.localrepository, '_journalfiles',
                 wrapjournalfiles)
    fcdescr = localrepo.localrepository.dirstate
    wrapfunction(fcdescr, 'func', wrapdirstate)

    try:
        shelve = extensions.find('shelve')
        wrapfunction(shelve, '_aborttransaction', wrapshelveaborttransaction)
    except KeyError:
        pass
    extensions.wrapcommand(commands.table, 'pull', wrappull)

def reposetup(ui, repo):
    ui.setconfig('sqldirstate', 'enabled', issqldirstate(repo))

# debug commands
cmdtable = {}
command = cmdutil.command(cmdtable)

@command('debugsqldirstate', [], 'hg debugsqldirstate [on|off]')
def debugsqldirstate(ui, repo, cmd, **opts):
    """ migrate to sqldirstate """

    if cmd == "on":
        upgrade(ui, repo)
    elif cmd == "off":
        downgrade(ui, repo)
    else:
        raise error.Abort("bad command")
