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

from mercurial.i18n import _
from mercurial.node import nullid, bin, hex
from mercurial import changegroup, cmdutil, scmutil, phases
from mercurial import error, hg, mdiff, merge, patch, repair, util
from mercurial import templatefilters
from mercurial import lock as lockmod
from hgext import rebase
import errno

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

class shelvedfile(object):
    """Helper for the file storing a single shelve

    Handles common functions on shelve files (.hg/.files/.patch) using
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
            raise util.Abort(_("shelved change '%s' not found") % self.name)

class shelvedstate(object):
    """Handle persistence during unshelving operations.

    Handles saving and restoring a shelved state. Ensures that different
    versions of a shelved state are possible and handles them appropriately.
    """
    _version = 1
    _filename = 'shelvedstate'

    @classmethod
    def load(cls, repo):
        fp = repo.opener(cls._filename)
        try:
            version = int(fp.readline().strip())

            if version != cls._version:
                raise util.Abort(_('this version of shelve is incompatible '
                                   'with the version used in this repo'))
            name = fp.readline().strip()
            wctx = fp.readline().strip()
            pendingctx = fp.readline().strip()
            parents = [bin(h) for h in fp.readline().split()]
            stripnodes = [bin(h) for h in fp.readline().split()]
            unknownfiles = fp.readline()[:-1].split('\0')
        finally:
            fp.close()

        obj = cls()
        obj.name = name
        obj.wctx = repo[bin(wctx)]
        obj.pendingctx = repo[bin(pendingctx)]
        obj.parents = parents
        obj.stripnodes = stripnodes
        obj.unknownfiles = unknownfiles

        return obj

    @classmethod
    def save(cls, repo, name, originalwctx, pendingctx, stripnodes,
             unknownfiles):
        fp = repo.opener(cls._filename, 'wb')
        fp.write('%i\n' % cls._version)
        fp.write('%s\n' % name)
        fp.write('%s\n' % hex(originalwctx.node()))
        fp.write('%s\n' % hex(pendingctx.node()))
        fp.write('%s\n' % ' '.join([hex(p) for p in repo.dirstate.parents()]))
        fp.write('%s\n' % ' '.join([hex(n) for n in stripnodes]))
        fp.write('%s\n' % '\0'.join(unknownfiles))
        fp.close()

    @classmethod
    def clear(cls, repo):
        util.unlinkpath(repo.join(cls._filename), ignoremissing=True)

def createcmd(ui, repo, pats, opts):
    """subcommand that creates a new shelve"""

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
        # use an uncommitted transaction to generate the bundle to avoid
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
    """subcommand that deletes all shelves"""

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
    """subcommand that deletes a specific shelve"""
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
    """return all shelves in repo as list of (time, filename)"""
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
    """subcommand that displays the list of shelves"""
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

def checkparents(repo, state):
    """check parent while resuming an unshelve"""
    if state.parents != repo.dirstate.parents():
        raise util.Abort(_('working directory parents do not match unshelve '
                           'state'))

def pathtofiles(repo, files):
    cwd = repo.getcwd()
    return [repo.pathto(f, cwd) for f in files]

def unshelveabort(ui, repo, state, opts):
    """subcommand that abort an in-progress unshelve"""
    wlock = repo.wlock()
    lock = None
    try:
        checkparents(repo, state)

        util.rename(repo.join('unshelverebasestate'),
                    repo.join('rebasestate'))
        try:
            rebase.rebase(ui, repo, **{
                'abort' : True
            })
        except Exception:
            util.rename(repo.join('rebasestate'),
                        repo.join('unshelverebasestate'))
            raise

        lock = repo.lock()

        mergefiles(ui, repo, state.wctx, state.pendingctx, state.unknownfiles)

        repair.strip(ui, repo, state.stripnodes, backup='none', topic='shelve')
        shelvedstate.clear(repo)
        ui.warn(_("unshelve of '%s' aborted\n") % state.name)
    finally:
        lockmod.release(lock, wlock)

def mergefiles(ui, repo, wctx, shelvectx, unknownfiles):
    """updates to wctx and merges the changes from shelvectx into the
    dirstate. drops any files in unknownfiles from the dirstate."""
    oldquiet = ui.quiet
    try:
        ui.quiet = True
        hg.update(repo, wctx.node())
        files = []
        files.extend(shelvectx.files())
        files.extend(shelvectx.parents()[0].files())
        cmdutil.revert(ui, repo, shelvectx, repo.dirstate.parents(),
                       *pathtofiles(repo, files),
                       **{'no_backup': True})
    finally:
        ui.quiet = oldquiet

    # Send untracked files back to being untracked
    dirstate = repo.dirstate
    for f in unknownfiles:
        dirstate.drop(f)

def unshelvecleanup(ui, repo, name, opts):
    """remove related files after an unshelve"""
    if not opts['keep']:
        for filetype in 'hg files patch'.split():
            shelvedfile(repo, name, filetype).unlink()

def unshelvecontinue(ui, repo, state, opts):
    """subcommand to continue an in-progress unshelve"""
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

        lock = repo.lock()

        util.rename(repo.join('unshelverebasestate'),
                    repo.join('rebasestate'))
        try:
            rebase.rebase(ui, repo, **{
                'continue' : True
            })
        except Exception:
            util.rename(repo.join('rebasestate'),
                        repo.join('unshelverebasestate'))
            raise

        shelvectx = repo['tip']
        if not shelvectx in state.pendingctx.children():
            # rebase was a no-op, so it produced no child commit
            shelvectx = state.pendingctx

        mergefiles(ui, repo, state.wctx, shelvectx, state.unknownfiles)

        state.stripnodes.append(shelvectx.node())
        repair.strip(ui, repo, state.stripnodes, backup='none', topic='shelve')
        shelvedstate.clear(repo)
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

    if not shelvedfile(repo, basename, 'files').exists():
        raise util.Abort(_("shelved change '%s' not found") % basename)

    wlock = lock = tr = None
    try:
        lock = repo.lock()
        wlock = repo.wlock()

        tr = repo.transaction('unshelve', report=lambda x: None)
        oldtiprev = len(repo)

        wctx = repo['.']
        tmpwctx = wctx
        # The goal is to have a commit structure like so:
        # ...-> wctx -> tmpwctx -> shelvectx
        # where tmpwctx is an optional commit with the user's pending changes
        # and shelvectx is the unshelved changes. Then we merge it all down
        # to the original wctx.

        # Store pending changes in a commit
        m, a, r, d, u = repo.status(unknown=True)[:5]
        if m or a or r or d or u:
            def commitfunc(ui, repo, message, match, opts):
                hasmq = util.safehasattr(repo, 'mq')
                if hasmq:
                    saved, repo.mq.checkapplied = repo.mq.checkapplied, False

                try:
                    return repo.commit(message, 'shelve@localhost',
                                       opts.get('date'), match)
                finally:
                    if hasmq:
                        repo.mq.checkapplied = saved

            tempopts = {}
            tempopts['message'] = "pending changes temporary commit"
            tempopts['addremove'] = True
            oldquiet = ui.quiet
            try:
                ui.quiet = True
                node = cmdutil.commit(ui, repo, commitfunc, None, tempopts)
            finally:
                ui.quiet = oldquiet
            tmpwctx = repo[node]

        try:
            fp = shelvedfile(repo, basename, 'hg').opener()
            gen = changegroup.readbundle(fp, fp.name)
            repo.addchangegroup(gen, 'unshelve', 'bundle:' + fp.name)
            nodes = [ctx.node() for ctx in repo.set('%d:', oldtiprev)]
            phases.retractboundary(repo, phases.secret, nodes)
        finally:
            fp.close()

        shelvectx = repo['tip']

        # If the shelve is not immediately on top of the commit
        # we'll be merging with, rebase it to be on top.
        if tmpwctx.node() != shelvectx.parents()[0].node():
            try:
                rebase.rebase(ui, repo, **{
                    'rev' : [shelvectx.rev()],
                    'dest' : str(tmpwctx.rev()),
                    'keep' : True,
                })
            except error.InterventionRequired:
                tr.close()

                stripnodes = [repo.changelog.node(rev)
                              for rev in xrange(oldtiprev, len(repo))]
                shelvedstate.save(repo, basename, wctx, tmpwctx, stripnodes, u)

                util.rename(repo.join('rebasestate'),
                            repo.join('unshelverebasestate'))
                raise error.InterventionRequired(
                    _("unresolved conflicts (see 'hg resolve', then "
                      "'hg unshelve --continue')"))

            # refresh ctx after rebase completes
            shelvectx = repo['tip']

            if not shelvectx in tmpwctx.children():
                # rebase was a no-op, so it produced no child commit
                shelvectx = tmpwctx

        mergefiles(ui, repo, wctx, shelvectx, u)
        shelvedstate.clear(repo)

        # The transaction aborting will strip all the commits for us,
        # but it doesn't update the inmemory structures, so addchangegroup
        # hooks still fire and try to operate on the missing commits.
        # Clean up manually to prevent this.
        repo.unfiltered().changelog.strip(oldtiprev, tr)

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
        [shelvedstate._filename, False, False,
         _('unshelve already in progress'),
         _("use 'hg unshelve --continue' or 'hg unshelve --abort'")])
