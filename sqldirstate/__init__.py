# __init__.py - sqldirstate extension
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

testedwith = 'internal'

from sqldirstate import makedirstate, DBFILE, toflat, tosql

from mercurial import error, extensions, cmdutil, localrepo, util
from mercurial.extensions import wrapfunction


def issqldirstate(repo):
    return util.safehasattr(repo, 'requirements') and \
        'sqldirstate' in repo.requirements

def wrapfilecache(cls, propname, wrapper, *paths):
    """Wraps a filecache property. These can't be wrapped using the normal
    wrapfunction. This should eventually go into upstream Mercurial.
    """
    assert callable(wrapper)
    for currcls in cls.__mro__:
        if propname in currcls.__dict__:
            origfn = currcls.__dict__[propname].func
            assert callable(origfn)
            def wrap(*args, **kwargs):
                return wrapper(origfn, *args, **kwargs)
            currcls.__dict__[propname].func = wrap
            currcls.__dict__[propname].paths = paths
            break

    if currcls is object:
        raise AttributeError(
            _("type '%s' has no property '%s'") % (cls, propname))

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
    if repo.ui.configbool('format', 'sqldirstate', True):
        reqs.add('sqldirstate')
    return reqs

def wrapshelveaborttransaction(orig, repo):
    if issqldirstate(repo):
        tr = repo.currenttransaction()
        repo.dirstate._writesqldirstate()
        tr.abort()
    else:
        return orig(repo)

def featuresetup(ui, supported):
    # don't die on seeing a repo with the sqldirstate requirement
    supported |= set(['sqldirstate'])

def uisetup(ui):
    localrepo.localrepository.featuresetupfuncs.add(featuresetup)
    wrapfunction(localrepo, 'newreporequirements',
                 wrapnewreporequirements)
    wrapfunction(localrepo.localrepository, '_journalfiles',
                 wrapjournalfiles)
    wrapfilecache(localrepo.localrepository, 'dirstate',
                  wrapdirstate)
    try:
        shelve = extensions.find('shelve')
        wrapfunction(shelve, '_aborttransaction', wrapshelveaborttransaction)
    except KeyError:
        pass

# debug commands
cmdtable = {}
command = cmdutil.command(cmdtable)
@command('debugsqldirstate', [], 'hg debugsqldirstate [on|off]')
def debugsqldirstate(ui, repo, cmd, **opts):
    """ migrate to sqldirstate """

    if cmd == "on":
        if 'sqldirstate' not in repo.requirements:
            repo.dirstate._read()
            tosql(repo.dirstate)
            repo.requirements.add('sqldirstate')
            repo._writerequirements()
            repo.dirstate._opener.unlink('dirstate')
        else:
            raise error.Abort("sqldirstate is already enabled")

    if cmd == "off":
        if 'sqldirstate' in repo.requirements:
            toflat(repo.dirstate)
            repo.requirements.remove('sqldirstate')
            repo._writerequirements()
            repo.dirstate._opener.unlink('dirstate.sqlite3')
        else:
            raise error.Abort("sqldirstate is disabled")
