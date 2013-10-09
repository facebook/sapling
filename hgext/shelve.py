# shelve.py - save/restore working directory state
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""save and restore changes to the working directory

The "hg shelve" command saves changes made to the working directory
and reverts those changes, resetting the working directory to a clean
state.

Later on, the "hg unshelve" command restores the changes saved by "hg
shelve". Changes can be restored even after updating to a different
parent, in which case Mercurial's merge machinery will resolve any
conflicts if necessary.

You can have more than one shelved change outstanding at a time; each
shelved change has a distinct name. For details, see the help for "hg
shelve".
"""

try:
    import cPickle as pickle
    pickle.dump # import now
except ImportError:
    import pickle
from mercurial.i18n import _
from mercurial.node import nullid
from mercurial import changegroup, cmdutil, scmutil, phases
from mercurial import error, hg, mdiff, merge, patch, repair, util
from mercurial import templatefilters
from mercurial import lock as lockmod
import errno

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

class shelvedfile(object):
    """Handles common functions on shelve files (.hg/.files/.patch) using
    the vfs layer"""
    def __init__(self, repo, name, filetype=None):
        self.repo = repo
        self.name = name
        self.vfs = scmutil.vfs(repo.join('shelved'))
        if filetype:
            self.fname = name + '.' + filetype
        else:
            self.fname = name

    def exists(self):
        return self.vfs.exists(self.fname)

    def filename(self):
        return self.vfs.join(self.fname)

    def unlink(self):
        util.unlink(self.filename())

    def stat(self):
        return self.vfs.stat(self.fname)

    def opener(self, mode='rb'):
        try:
            return self.vfs(self.fname, mode)
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
            if mode[0] in 'wa':
                try:
                    self.vfs.mkdir()
                    return self.vfs(self.fname, mode)
                except IOError, err:
                    if err.errno != errno.EEXIST:
                        raise
            elif mode[0] == 'r':
                raise util.Abort(_("shelved change '%s' not found") %
                                 self.name)

class shelvedstate(object):
    """Handles saving and restoring a shelved state. Ensures that different
    versions of a shelved state are possible and handles them appropriate"""
    _version = 1
    _filename = 'shelvedstate'

    @classmethod
    def load(cls, repo):
        fp = repo.opener(cls._filename)
        (version, name, parents, stripnodes) = pickle.load(fp)

        if version != cls._version:
            raise util.Abort(_('this version of shelve is incompatible '
                               'with the version used in this repo'))

        obj = cls()
        obj.name = name
        obj.parents = parents
        obj.stripnodes = stripnodes

        return obj

    @classmethod
    def save(cls, repo, name, stripnodes):
        fp = repo.opener(cls._filename, 'wb')
        pickle.dump((cls._version, name,
                     repo.dirstate.parents(),
                     stripnodes), fp)
        fp.close()

    @staticmethod
    def clear(repo):
        util.unlinkpath(repo.join('shelvedstate'), ignoremissing=True)

def createcmd(ui, repo, pats, opts):
    def publicancestors(ctx):
        """Compute the heads of the public ancestors of a commit.

        Much faster than the revset heads(ancestors(ctx) - draft())"""
        seen = set()
        visit = util.deque()
        visit.append(ctx)
        while visit:
            ctx = visit.popleft()
            for parent in ctx.parents():
                rev = parent.rev()
                if rev not in seen:
                    seen.add(rev)
                    if parent.mutable():
                        visit.append(parent)
                    else:
                        yield parent.node()

    wctx = repo[None]
    parents = wctx.parents()
    if len(parents) > 1:
        raise util.Abort(_('cannot shelve while merging'))
    parent = parents[0]

    # we never need the user, so we use a generic user for all shelve operations
    user = 'shelve@localhost'
    label = repo._bookmarkcurrent or parent.branch() or 'default'

    # slashes aren't allowed in filenames, therefore we rename it
    origlabel, label = label, label.replace('/', '_')

    def gennames():
        yield label
        for i in xrange(1, 100):
            yield '%s-%02d' % (label, i)

    shelvedfiles = []

    def commitfunc(ui, repo, message, match, opts):
        # check modified, added, removed, deleted only
        for flist in repo.status(match=match)[:4]:
            shelvedfiles.extend(flist)
        hasmq = util.safehasattr(repo, 'mq')
        if hasmq:
            saved, repo.mq.checkapplied = repo.mq.checkapplied, False
        try:
            return repo.commit(message, user, opts.get('date'), match)
        finally:
            if hasmq:
                repo.mq.checkapplied = saved

    if parent.node() != nullid:
        desc = parent.description().split('\n', 1)[0]
    else:
        desc = '(empty repository)'

    if not opts['message']:
        opts['message'] = desc

    name = opts['name']

    wlock = lock = tr = bms = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()

        bms = repo._bookmarks.copy()
        # use an uncommited transaction to generate the bundle to avoid
        # pull races. ensure we don't print the abort message to stderr.
        tr = repo.transaction('commit', report=lambda x: None)

        if name:
            if shelvedfile(repo, name, 'hg').exists():
                raise util.Abort(_("a shelved change named '%s' already exists")
                                 % name)
        else:
            for n in gennames():
                if not shelvedfile(repo, n, 'hg').exists():
                    name = n
                    break
            else:
                raise util.Abort(_("too many shelved changes named '%s'") %
                                 label)

        # ensure we are not creating a subdirectory or a hidden file
        if '/' in name or '\\' in name:
            raise util.Abort(_('shelved change names may not contain slashes'))
        if name.startswith('.'):
            raise util.Abort(_("shelved change names may not start with '.'"))

        node = cmdutil.commit(ui, repo, commitfunc, pats, opts)

        if not node:
            stat = repo.status(match=scmutil.match(repo[None], pats, opts))
            if stat[3]:
                ui.status(_("nothing changed (%d missing files, see "
                            "'hg status')\n") % len(stat[3]))
            else:
                ui.status(_("nothing changed\n"))
            return 1

        phases.retractboundary(repo, phases.secret, [node])

        fp = shelvedfile(repo, name, 'files').opener('wb')
        fp.write('\0'.join(shelvedfiles))

        bases = list(publicancestors(repo[node]))
        cg = repo.changegroupsubset(bases, [node], 'shelve')
        changegroup.writebundle(cg, shelvedfile(repo, name, 'hg').filename(),
                                'HG10UN')
        cmdutil.export(repo, [node],
                       fp=shelvedfile(repo, name, 'patch').opener('wb'),
                       opts=mdiff.diffopts(git=True))


        if ui.formatted():
            desc = util.ellipsis(desc, ui.termwidth())
        ui.status(_('shelved as %s\n') % name)
        hg.update(repo, parent.node())
    finally:
        if bms:
            # restore old bookmarks
            repo._bookmarks.update(bms)
            repo._bookmarks.write()
        if tr:
            tr.abort()
        lockmod.release(lock, wlock)

def cleanupcmd(ui, repo):
    wlock = None
    try:
        wlock = repo.wlock()
        for (name, _) in repo.vfs.readdir('shelved'):
            suffix = name.rsplit('.', 1)[-1]
            if suffix in ('hg', 'files', 'patch'):
                shelvedfile(repo, name).unlink()
    finally:
        lockmod.release(wlock)

def deletecmd(ui, repo, pats):
    if not pats:
        raise util.Abort(_('no shelved changes specified!'))
    wlock = None
    try:
        wlock = repo.wlock()
        try:
            for name in pats:
                for suffix in 'hg files patch'.split():
                    shelvedfile(repo, name, suffix).unlink()
        except OSError, err:
            if err.errno != errno.ENOENT:
                raise
            raise util.Abort(_("shelved change '%s' not found") % name)
    finally:
        lockmod.release(wlock)

def listshelves(repo):
    try:
        names = repo.vfs.readdir('shelved')
    except OSError, err:
        if err.errno != errno.ENOENT:
            raise
        return []
    info = []
    for (name, _) in names:
        pfx, sfx = name.rsplit('.', 1)
        if not pfx or sfx != 'patch':
            continue
        st = shelvedfile(repo, name).stat()
        info.append((st.st_mtime, shelvedfile(repo, pfx).filename()))
    return sorted(info, reverse=True)

def listcmd(ui, repo, pats, opts):
    pats = set(pats)
    width = 80
    if not ui.plain():
        width = ui.termwidth()
    namelabel = 'shelve.newest'
    for mtime, name in listshelves(repo):
        sname = util.split(name)[1]
        if pats and sname not in pats:
            continue
        ui.write(sname, label=namelabel)
        namelabel = 'shelve.name'
        if ui.quiet:
            ui.write('\n')
            continue
        ui.write(' ' * (16 - len(sname)))
        used = 16
        age = '(%s)' % templatefilters.age(util.makedate(mtime), abbrev=True)
        ui.write(age, label='shelve.age')
        ui.write(' ' * (12 - len(age)))
        used += 12
        fp = open(name + '.patch', 'rb')
        try:
            while True:
                line = fp.readline()
                if not line:
                    break
                if not line.startswith('#'):
                    desc = line.rstrip()
                    if ui.formatted():
                        desc = util.ellipsis(desc, width - used)
                    ui.write(desc)
                    break
            ui.write('\n')
            if not (opts['patch'] or opts['stat']):
                continue
            difflines = fp.readlines()
            if opts['patch']:
                for chunk, label in patch.difflabel(iter, difflines):
                    ui.write(chunk, label=label)
            if opts['stat']:
                for chunk, label in patch.diffstatui(difflines, width=width,
                                                     git=True):
                    ui.write(chunk, label=label)
        finally:
            fp.close()

def readshelvedfiles(repo, basename):
    fp = shelvedfile(repo, basename, 'files').opener()
    return fp.read().split('\0')

def checkparents(repo, state):
    if state.parents != repo.dirstate.parents():
        raise util.Abort(_('working directory parents do not match unshelve '
                           'state'))

def unshelveabort(ui, repo, state, opts):
    wlock = repo.wlock()
    lock = None
    try:
        checkparents(repo, state)
        lock = repo.lock()
        merge.mergestate(repo).reset()
        if opts['keep']:
            repo.setparents(repo.dirstate.parents()[0])
        else:
            revertfiles = readshelvedfiles(repo, state.name)
            wctx = repo.parents()[0]
            cmdutil.revert(ui, repo, wctx, [wctx.node(), nullid],
                           *revertfiles, **{'no_backup': True})
            # fix up the weird dirstate states the merge left behind
            mf = wctx.manifest()
            dirstate = repo.dirstate
            for f in revertfiles:
                if f in mf:
                    dirstate.normallookup(f)
                else:
                    dirstate.drop(f)
            dirstate._pl = (wctx.node(), nullid)
            dirstate._dirty = True
        repair.strip(ui, repo, state.stripnodes, backup='none', topic='shelve')
        shelvedstate.clear(repo)
        ui.warn(_("unshelve of '%s' aborted\n") % state.name)
    finally:
        lockmod.release(lock, wlock)

def unshelvecleanup(ui, repo, name, opts):
    if not opts['keep']:
        for filetype in 'hg files patch'.split():
            shelvedfile(repo, name, filetype).unlink()

def finishmerge(ui, repo, ms, stripnodes, name, opts):
    # Reset the working dir so it's no longer in a merge state.
    dirstate = repo.dirstate
    for f in ms:
        if dirstate[f] == 'm':
            dirstate.normallookup(f)
    dirstate._pl = (dirstate._pl[0], nullid)
    dirstate._dirty = dirstate._dirtypl = True
    shelvedstate.clear(repo)

def unshelvecontinue(ui, repo, state, opts):
    # We're finishing off a merge. First parent is our original
    # parent, second is the temporary "fake" commit we're unshelving.
    wlock = repo.wlock()
    lock = None
    try:
        checkparents(repo, state)
        ms = merge.mergestate(repo)
        if [f for f in ms if ms[f] == 'u']:
            raise util.Abort(
                _("unresolved conflicts, can't continue"),
                hint=_("see 'hg resolve', then 'hg unshelve --continue'"))
        finishmerge(ui, repo, ms, state.stripnodes, state.name, opts)
        lock = repo.lock()
        repair.strip(ui, repo, state.stripnodes, backup='none', topic='shelve')
        unshelvecleanup(ui, repo, state.name, opts)
        ui.status(_("unshelve of '%s' complete\n") % state.name)
    finally:
        lockmod.release(lock, wlock)

@command('unshelve',
         [('a', 'abort', None,
           _('abort an incomplete unshelve operation')),
          ('c', 'continue', None,
           _('continue an incomplete unshelve operation')),
          ('', 'keep', None,
           _('keep shelve after unshelving'))],
         _('hg unshelve [SHELVED]'))
def unshelve(ui, repo, *shelved, **opts):
    """restore a shelved change to the working directory

    This command accepts an optional name of a shelved change to
    restore. If none is given, the most recent shelved change is used.

    If a shelved change is applied successfully, the bundle that
    contains the shelved changes is deleted afterwards.

    Since you can restore a shelved change on top of an arbitrary
    commit, it is possible that unshelving will result in a conflict
    between your changes and the commits you are unshelving onto. If
    this occurs, you must resolve the conflict, then use
    ``--continue`` to complete the unshelve operation. (The bundle
    will not be deleted until you successfully complete the unshelve.)

    (Alternatively, you can use ``--abort`` to abandon an unshelve
    that causes a conflict. This reverts the unshelved changes, and
    does not delete the bundle.)
    """
    abortf = opts['abort']
    continuef = opts['continue']
    if not abortf and not continuef:
        cmdutil.checkunfinished(repo)

    if abortf or continuef:
        if abortf and continuef:
            raise util.Abort(_('cannot use both abort and continue'))
        if shelved:
            raise util.Abort(_('cannot combine abort/continue with '
                               'naming a shelved change'))

        try:
            state = shelvedstate.load(repo)
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
            raise util.Abort(_('no unshelve operation underway'))

        if abortf:
            return unshelveabort(ui, repo, state, opts)
        elif continuef:
            return unshelvecontinue(ui, repo, state, opts)
    elif len(shelved) > 1:
        raise util.Abort(_('can only unshelve one change at a time'))
    elif not shelved:
        shelved = listshelves(repo)
        if not shelved:
            raise util.Abort(_('no shelved changes to apply!'))
        basename = util.split(shelved[0][1])[1]
        ui.status(_("unshelving change '%s'\n") % basename)
    else:
        basename = shelved[0]

    shelvedfiles = readshelvedfiles(repo, basename)

    m, a, r, d = repo.status()[:4]
    unsafe = set(m + a + r + d).intersection(shelvedfiles)
    if unsafe:
        ui.warn(_('the following shelved files have been modified:\n'))
        for f in sorted(unsafe):
            ui.warn('  %s\n' % f)
        ui.warn(_('you must commit, revert, or shelve your changes before you '
                  'can proceed\n'))
        raise util.Abort(_('cannot unshelve due to local changes\n'))

    wlock = lock = tr = None
    try:
        lock = repo.lock()

        tr = repo.transaction('unshelve', report=lambda x: None)
        oldtiprev = len(repo)
        try:
            fp = shelvedfile(repo, basename, 'hg').opener()
            gen = changegroup.readbundle(fp, fp.name)
            repo.addchangegroup(gen, 'unshelve', 'bundle:' + fp.name)
            nodes = [ctx.node() for ctx in repo.set('%d:', oldtiprev)]
            phases.retractboundary(repo, phases.secret, nodes)
            tr.close()
        finally:
            fp.close()

        tip = repo['tip']
        wctx = repo['.']
        ancestor = tip.ancestor(wctx)

        wlock = repo.wlock()

        if ancestor.node() != wctx.node():
            conflicts = hg.merge(repo, tip.node(), force=True, remind=False)
            ms = merge.mergestate(repo)
            stripnodes = [repo.changelog.node(rev)
                          for rev in xrange(oldtiprev, len(repo))]
            if conflicts:
                shelvedstate.save(repo, basename, stripnodes)
                # Fix up the dirstate entries of files from the second
                # parent as if we were not merging, except for those
                # with unresolved conflicts.
                parents = repo.parents()
                revertfiles = set(parents[1].files()).difference(ms)
                cmdutil.revert(ui, repo, parents[1],
                               (parents[0].node(), nullid),
                               *revertfiles, **{'no_backup': True})
                raise error.InterventionRequired(
                    _("unresolved conflicts (see 'hg resolve', then "
                      "'hg unshelve --continue')"))
            finishmerge(ui, repo, ms, stripnodes, basename, opts)
        else:
            parent = tip.parents()[0]
            hg.update(repo, parent.node())
            cmdutil.revert(ui, repo, tip, repo.dirstate.parents(), *tip.files(),
                           **{'no_backup': True})

        prevquiet = ui.quiet
        ui.quiet = True
        try:
            repo.rollback(force=True)
        finally:
            ui.quiet = prevquiet

        unshelvecleanup(ui, repo, basename, opts)
    finally:
        if tr:
            tr.release()
        lockmod.release(lock, wlock)

@command('shelve',
         [('A', 'addremove', None,
           _('mark new/missing files as added/removed before shelving')),
          ('', 'cleanup', None,
           _('delete all shelved changes')),
          ('', 'date', '',
           _('shelve with the specified commit date'), _('DATE')),
          ('d', 'delete', None,
           _('delete the named shelved change(s)')),
          ('l', 'list', None,
           _('list current shelves')),
          ('m', 'message', '',
           _('use text as shelve message'), _('TEXT')),
          ('n', 'name', '',
           _('use the given name for the shelved commit'), _('NAME')),
          ('p', 'patch', None,
           _('show patch')),
          ('', 'stat', None,
           _('output diffstat-style summary of changes'))],
         _('hg shelve'))
def shelvecmd(ui, repo, *pats, **opts):
    '''save and set aside changes from the working directory

    Shelving takes files that "hg status" reports as not clean, saves
    the modifications to a bundle (a shelved change), and reverts the
    files so that their state in the working directory becomes clean.

    To restore these changes to the working directory, using "hg
    unshelve"; this will work even if you switch to a different
    commit.

    When no files are specified, "hg shelve" saves all not-clean
    files. If specific files or directories are named, only changes to
    those files are shelved.

    Each shelved change has a name that makes it easier to find later.
    The name of a shelved change defaults to being based on the active
    bookmark, or if there is no active bookmark, the current named
    branch.  To specify a different name, use ``--name``.

    To see a list of existing shelved changes, use the ``--list``
    option. For each shelved change, this will print its name, age,
    and description; use ``--patch`` or ``--stat`` for more details.

    To delete specific shelved changes, use ``--delete``. To delete
    all shelved changes, use ``--cleanup``.
    '''
    cmdutil.checkunfinished(repo)

    def checkopt(opt, incompatible):
        if opts[opt]:
            for i in incompatible.split():
                if opts[i]:
                    raise util.Abort(_("options '--%s' and '--%s' may not be "
                                       "used together") % (opt, i))
            return True
    if checkopt('cleanup', 'addremove delete list message name patch stat'):
        if pats:
            raise util.Abort(_("cannot specify names when using '--cleanup'"))
        return cleanupcmd(ui, repo)
    elif checkopt('delete', 'addremove cleanup list message name patch stat'):
        return deletecmd(ui, repo, pats)
    elif checkopt('list', 'addremove cleanup delete message name'):
        return listcmd(ui, repo, pats, opts)
    else:
        for i in ('patch', 'stat'):
            if opts[i]:
                raise util.Abort(_("option '--%s' may not be "
                                   "used when shelving a change") % (i,))
        return createcmd(ui, repo, pats, opts)

def extsetup(ui):
    cmdutil.unfinishedstates.append(
        [shelvedstate._filename, False, True, _('unshelve already in progress'),
         _("use 'hg unshelve --continue' or 'hg unshelve --abort'")])
