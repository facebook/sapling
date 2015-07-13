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

import collections
import itertools
from mercurial.i18n import _
from mercurial.node import nullid, nullrev, bin, hex
from mercurial import changegroup, cmdutil, scmutil, phases, commands
from mercurial import error, hg, mdiff, merge, patch, repair, util
from mercurial import templatefilters, exchange, bundlerepo
from mercurial import lock as lockmod
from hgext import rebase
import errno

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

backupdir = 'shelve-backup'

class shelvedfile(object):
    """Helper for the file storing a single shelve

    Handles common functions on shelve files (.hg/.patch) using
    the vfs layer"""
    def __init__(self, repo, name, filetype=None):
        self.repo = repo
        self.name = name
        self.vfs = scmutil.vfs(repo.join('shelved'))
        self.backupvfs = scmutil.vfs(repo.join(backupdir))
        self.ui = self.repo.ui
        if filetype:
            self.fname = name + '.' + filetype
        else:
            self.fname = name

    def exists(self):
        return self.vfs.exists(self.fname)

    def filename(self):
        return self.vfs.join(self.fname)

    def backupfilename(self):
        def gennames(base):
            yield base
            base, ext = base.rsplit('.', 1)
            for i in itertools.count(1):
                yield '%s-%d.%s' % (base, i, ext)

        name = self.backupvfs.join(self.fname)
        for n in gennames(name):
            if not self.backupvfs.exists(n):
                return n

    def movetobackup(self):
        if not self.backupvfs.isdir():
            self.backupvfs.makedir()
        util.rename(self.filename(), self.backupfilename())

    def stat(self):
        return self.vfs.stat(self.fname)

    def opener(self, mode='rb'):
        try:
            return self.vfs(self.fname, mode)
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            raise util.Abort(_("shelved change '%s' not found") % self.name)

    def applybundle(self):
        fp = self.opener()
        try:
            gen = exchange.readbundle(self.repo.ui, fp, self.fname, self.vfs)
            changegroup.addchangegroup(self.repo, gen, 'unshelve',
                                       'bundle:' + self.vfs.join(self.fname),
                                       targetphase=phases.secret)
        finally:
            fp.close()

    def bundlerepo(self):
        return bundlerepo.bundlerepository(self.repo.baseui, self.repo.root,
                                           self.vfs.join(self.fname))
    def writebundle(self, cg):
        changegroup.writebundle(self.ui, cg, self.fname, 'HG10UN', self.vfs)

class shelvedstate(object):
    """Handle persistence during unshelving operations.

    Handles saving and restoring a shelved state. Ensures that different
    versions of a shelved state are possible and handles them appropriately.
    """
    _version = 1
    _filename = 'shelvedstate'

    @classmethod
    def load(cls, repo):
        fp = repo.vfs(cls._filename)
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
        finally:
            fp.close()

        obj = cls()
        obj.name = name
        obj.wctx = repo[bin(wctx)]
        obj.pendingctx = repo[bin(pendingctx)]
        obj.parents = parents
        obj.stripnodes = stripnodes

        return obj

    @classmethod
    def save(cls, repo, name, originalwctx, pendingctx, stripnodes):
        fp = repo.vfs(cls._filename, 'wb')
        fp.write('%i\n' % cls._version)
        fp.write('%s\n' % name)
        fp.write('%s\n' % hex(originalwctx.node()))
        fp.write('%s\n' % hex(pendingctx.node()))
        fp.write('%s\n' % ' '.join([hex(p) for p in repo.dirstate.parents()]))
        fp.write('%s\n' % ' '.join([hex(n) for n in stripnodes]))
        fp.close()

    @classmethod
    def clear(cls, repo):
        util.unlinkpath(repo.join(cls._filename), ignoremissing=True)

def cleanupoldbackups(repo):
    vfs = scmutil.vfs(repo.join(backupdir))
    maxbackups = repo.ui.configint('shelve', 'maxbackups', 10)
    hgfiles = [f for f in vfs.listdir() if f.endswith('.hg')]
    hgfiles = sorted([(vfs.stat(f).st_mtime, f) for f in hgfiles])
    if 0 < maxbackups and maxbackups < len(hgfiles):
        bordermtime = hgfiles[-maxbackups][0]
    else:
        bordermtime = None
    for mtime, f in hgfiles[:len(hgfiles) - maxbackups]:
        if mtime == bordermtime:
            # keep it, because timestamp can't decide exact order of backups
            continue
        base = f[:-3]
        for ext in 'hg patch'.split():
            try:
                vfs.unlink(base + '.' + ext)
            except OSError as err:
                if err.errno != errno.ENOENT:
                    raise

def createcmd(ui, repo, pats, opts):
    """subcommand that creates a new shelve"""

    def publicancestors(ctx):
        """Compute the public ancestors of a commit.

        Much faster than the revset ancestors(ctx) & draft()"""
        seen = set([nullrev])
        visit = collections.deque()
        visit.append(ctx)
        while visit:
            ctx = visit.popleft()
            yield ctx.node()
            for parent in ctx.parents():
                rev = parent.rev()
                if rev not in seen:
                    seen.add(rev)
                    if parent.mutable():
                        visit.append(parent)

    wctx = repo[None]
    parents = wctx.parents()
    if len(parents) > 1:
        raise util.Abort(_('cannot shelve while merging'))
    parent = parents[0]

    # we never need the user, so we use a generic user for all shelve operations
    user = 'shelve@localhost'
    label = repo._activebookmark or parent.branch() or 'default'

    # slashes aren't allowed in filenames, therefore we rename it
    label = label.replace('/', '_')

    def gennames():
        yield label
        for i in xrange(1, 100):
            yield '%s-%02d' % (label, i)

    def commitfunc(ui, repo, message, match, opts):
        hasmq = util.safehasattr(repo, 'mq')
        if hasmq:
            saved, repo.mq.checkapplied = repo.mq.checkapplied, False
        backup = repo.ui.backupconfig('phases', 'new-commit')
        try:
            repo.ui. setconfig('phases', 'new-commit', phases.secret)
            editor = cmdutil.getcommiteditor(editform='shelve.shelve', **opts)
            return repo.commit(message, user, opts.get('date'), match,
                               editor=editor)
        finally:
            repo.ui.restoreconfig(backup)
            if hasmq:
                repo.mq.checkapplied = saved

    if parent.node() != nullid:
        desc = "changes to '%s'" % parent.description().split('\n', 1)[0]
    else:
        desc = '(changes in empty repository)'

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
        interactive = opts.get('interactive', False)

        def interactivecommitfunc(ui, repo, *pats, **opts):
            match = scmutil.match(repo['.'], pats, {})
            message = opts['message']
            return commitfunc(ui, repo, message, match, opts)
        if not interactive:
            node = cmdutil.commit(ui, repo, commitfunc, pats, opts)
        else:
            node = cmdutil.dorecord(ui, repo, interactivecommitfunc, 'commit',
                                    False, cmdutil.recordfilter, *pats, **opts)
        if not node:
            stat = repo.status(match=scmutil.match(repo[None], pats, opts))
            if stat.deleted:
                ui.status(_("nothing changed (%d missing files, see "
                            "'hg status')\n") % len(stat.deleted))
            else:
                ui.status(_("nothing changed\n"))
            return 1

        bases = list(publicancestors(repo[node]))
        cg = changegroup.changegroupsubset(repo, bases, [node], 'shelve')
        shelvedfile(repo, name, 'hg').writebundle(cg)
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
        for (name, _type) in repo.vfs.readdir('shelved'):
            suffix = name.rsplit('.', 1)[-1]
            if suffix in ('hg', 'patch'):
                shelvedfile(repo, name).movetobackup()
            cleanupoldbackups(repo)
    finally:
        lockmod.release(wlock)

def deletecmd(ui, repo, pats):
    """subcommand that deletes a specific shelve"""
    if not pats:
        raise util.Abort(_('no shelved changes specified!'))
    wlock = repo.wlock()
    try:
        for name in pats:
            for suffix in 'hg patch'.split():
                shelvedfile(repo, name, suffix).movetobackup()
        cleanupoldbackups(repo)
    except OSError as err:
        if err.errno != errno.ENOENT:
            raise
        raise util.Abort(_("shelved change '%s' not found") % name)
    finally:
        lockmod.release(wlock)

def listshelves(repo):
    """return all shelves in repo as list of (time, filename)"""
    try:
        names = repo.vfs.readdir('shelved')
    except OSError as err:
        if err.errno != errno.ENOENT:
            raise
        return []
    info = []
    for (name, _type) in names:
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

def singlepatchcmds(ui, repo, pats, opts, subcommand):
    """subcommand that displays a single shelf"""
    if len(pats) != 1:
        raise util.Abort(_("--%s expects a single shelf") % subcommand)
    shelfname = pats[0]

    if not shelvedfile(repo, shelfname, 'patch').exists():
        raise util.Abort(_("cannot find shelf %s") % shelfname)

    listcmd(ui, repo, pats, opts)

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

        mergefiles(ui, repo, state.wctx, state.pendingctx)

        repair.strip(ui, repo, state.stripnodes, backup=False, topic='shelve')
        shelvedstate.clear(repo)
        ui.warn(_("unshelve of '%s' aborted\n") % state.name)
    finally:
        lockmod.release(lock, wlock)

def mergefiles(ui, repo, wctx, shelvectx):
    """updates to wctx and merges the changes from shelvectx into the
    dirstate."""
    oldquiet = ui.quiet
    try:
        ui.quiet = True
        hg.update(repo, wctx.node())
        files = []
        files.extend(shelvectx.files())
        files.extend(shelvectx.parents()[0].files())

        # revert will overwrite unknown files, so move them out of the way
        for file in repo.status(unknown=True).unknown:
            if file in files:
                util.rename(file, file + ".orig")
        ui.pushbuffer(True)
        cmdutil.revert(ui, repo, shelvectx, repo.dirstate.parents(),
                       *pathtofiles(repo, files),
                       **{'no_backup': True})
        ui.popbuffer()
    finally:
        ui.quiet = oldquiet

def unshelvecleanup(ui, repo, name, opts):
    """remove related files after an unshelve"""
    if not opts['keep']:
        for filetype in 'hg patch'.split():
            shelvedfile(repo, name, filetype).movetobackup()
        cleanupoldbackups(repo)

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
        else:
            # only strip the shelvectx if the rebase produced it
            state.stripnodes.append(shelvectx.node())

        mergefiles(ui, repo, state.wctx, shelvectx)

        repair.strip(ui, repo, state.stripnodes, backup=False, topic='shelve')
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
           _('keep shelve after unshelving')),
          ('', 'date', '',
           _('set date for temporary commits (DEPRECATED)'), _('DATE'))],
         _('hg unshelve [SHELVED]'))
def unshelve(ui, repo, *shelved, **opts):
    """restore a shelved change to the working directory

    This command accepts an optional name of a shelved change to
    restore. If none is given, the most recent shelved change is used.

    If a shelved change is applied successfully, the bundle that
    contains the shelved changes is moved to a backup location
    (.hg/shelve-backup).

    Since you can restore a shelved change on top of an arbitrary
    commit, it is possible that unshelving will result in a conflict
    between your changes and the commits you are unshelving onto. If
    this occurs, you must resolve the conflict, then use
    ``--continue`` to complete the unshelve operation. (The bundle
    will not be moved until you successfully complete the unshelve.)

    (Alternatively, you can use ``--abort`` to abandon an unshelve
    that causes a conflict. This reverts the unshelved changes, and
    leaves the bundle in place.)

    After a successful unshelve, the shelved changes are stored in a
    backup directory. Only the N most recent backups are kept. N
    defaults to 10 but can be overridden using the shelve.maxbackups
    configuration option.

    .. container:: verbose

       Timestamp in seconds is used to decide order of backups. More
       than ``maxbackups`` backups are kept, if same timestamp
       prevents from deciding exact order of them, for safety.
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
        except IOError as err:
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

    if not shelvedfile(repo, basename, 'patch').exists():
        raise util.Abort(_("shelved change '%s' not found") % basename)

    oldquiet = ui.quiet
    wlock = lock = tr = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()

        tr = repo.transaction('unshelve', report=lambda x: None)
        oldtiprev = len(repo)

        pctx = repo['.']
        tmpwctx = pctx
        # The goal is to have a commit structure like so:
        # ...-> pctx -> tmpwctx -> shelvectx
        # where tmpwctx is an optional commit with the user's pending changes
        # and shelvectx is the unshelved changes. Then we merge it all down
        # to the original pctx.

        # Store pending changes in a commit
        s = repo.status()
        if s.modified or s.added or s.removed or s.deleted:
            ui.status(_("temporarily committing pending changes "
                        "(restore with 'hg unshelve --abort')\n"))
            def commitfunc(ui, repo, message, match, opts):
                hasmq = util.safehasattr(repo, 'mq')
                if hasmq:
                    saved, repo.mq.checkapplied = repo.mq.checkapplied, False

                backup = repo.ui.backupconfig('phases', 'new-commit')
                try:
                    repo.ui. setconfig('phases', 'new-commit', phases.secret)
                    return repo.commit(message, 'shelve@localhost',
                                       opts.get('date'), match)
                finally:
                    repo.ui.restoreconfig(backup)
                    if hasmq:
                        repo.mq.checkapplied = saved

            tempopts = {}
            tempopts['message'] = "pending changes temporary commit"
            tempopts['date'] = opts.get('date')
            ui.quiet = True
            node = cmdutil.commit(ui, repo, commitfunc, [], tempopts)
            tmpwctx = repo[node]

        ui.quiet = True
        shelvedfile(repo, basename, 'hg').applybundle()

        ui.quiet = oldquiet

        shelvectx = repo['tip']

        # If the shelve is not immediately on top of the commit
        # we'll be merging with, rebase it to be on top.
        if tmpwctx.node() != shelvectx.parents()[0].node():
            ui.status(_('rebasing shelved changes\n'))
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
                shelvedstate.save(repo, basename, pctx, tmpwctx, stripnodes)

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

        mergefiles(ui, repo, pctx, shelvectx)
        shelvedstate.clear(repo)

        # The transaction aborting will strip all the commits for us,
        # but it doesn't update the inmemory structures, so addchangegroup
        # hooks still fire and try to operate on the missing commits.
        # Clean up manually to prevent this.
        repo.unfiltered().changelog.strip(oldtiprev, tr)

        unshelvecleanup(ui, repo, basename, opts)
    finally:
        ui.quiet = oldquiet
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
          ('e', 'edit', False,
           _('invoke editor on commit messages')),
          ('l', 'list', None,
           _('list current shelves')),
          ('m', 'message', '',
           _('use text as shelve message'), _('TEXT')),
          ('n', 'name', '',
           _('use the given name for the shelved commit'), _('NAME')),
          ('p', 'patch', None,
           _('show patch')),
          ('i', 'interactive', None,
           _('interactive mode, only works while creating a shelve')),
          ('', 'stat', None,
           _('output diffstat-style summary of changes'))] + commands.walkopts,
         _('hg shelve [OPTION]... [FILE]...'))
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

    allowables = [
        ('addremove', set(['create'])), # 'create' is pseudo action
        ('cleanup', set(['cleanup'])),
#       ('date', set(['create'])), # ignored for passing '--date "0 0"' in tests
        ('delete', set(['delete'])),
        ('edit', set(['create'])),
        ('list', set(['list'])),
        ('message', set(['create'])),
        ('name', set(['create'])),
        ('patch', set(['patch', 'list'])),
        ('stat', set(['stat', 'list'])),
    ]
    def checkopt(opt):
        if opts[opt]:
            for i, allowable in allowables:
                if opts[i] and opt not in allowable:
                    raise util.Abort(_("options '--%s' and '--%s' may not be "
                                       "used together") % (opt, i))
            return True
    if checkopt('cleanup'):
        if pats:
            raise util.Abort(_("cannot specify names when using '--cleanup'"))
        return cleanupcmd(ui, repo)
    elif checkopt('delete'):
        return deletecmd(ui, repo, pats)
    elif checkopt('list'):
        return listcmd(ui, repo, pats, opts)
    elif checkopt('patch'):
        return singlepatchcmds(ui, repo, pats, opts, subcommand='patch')
    elif checkopt('stat'):
        return singlepatchcmds(ui, repo, pats, opts, subcommand='stat')
    else:
        return createcmd(ui, repo, pats, opts)

def extsetup(ui):
    cmdutil.unfinishedstates.append(
        [shelvedstate._filename, False, False,
         _('unshelve already in progress'),
         _("use 'hg unshelve --continue' or 'hg unshelve --abort'")])
