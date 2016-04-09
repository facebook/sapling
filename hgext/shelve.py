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
from __future__ import absolute_import

import collections
import errno
import itertools
from mercurial import (
    bundle2,
    bundlerepo,
    changegroup,
    cmdutil,
    commands,
    error,
    exchange,
    hg,
    lock as lockmod,
    mdiff,
    merge,
    node as nodemod,
    patch,
    phases,
    repair,
    scmutil,
    templatefilters,
    util,
)
from mercurial.i18n import _

from . import (
    rebase,
)

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

backupdir = 'shelve-backup'
shelvedir = 'shelved'

class shelvedfile(object):
    """Helper for the file storing a single shelve

    Handles common functions on shelve files (.hg/.patch) using
    the vfs layer"""
    def __init__(self, repo, name, filetype=None):
        self.repo = repo
        self.name = name
        self.vfs = scmutil.vfs(repo.join(shelvedir))
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
            raise error.Abort(_("shelved change '%s' not found") % self.name)

    def applybundle(self):
        fp = self.opener()
        try:
            gen = exchange.readbundle(self.repo.ui, fp, self.fname, self.vfs)
            if not isinstance(gen, bundle2.unbundle20):
                gen.apply(self.repo, 'unshelve',
                          'bundle:' + self.vfs.join(self.fname),
                          targetphase=phases.secret)
            if isinstance(gen, bundle2.unbundle20):
                bundle2.applybundle(self.repo, gen,
                                    self.repo.currenttransaction(),
                                    source='unshelve',
                                    url='bundle:' + self.vfs.join(self.fname))
        finally:
            fp.close()

    def bundlerepo(self):
        return bundlerepo.bundlerepository(self.repo.baseui, self.repo.root,
                                           self.vfs.join(self.fname))
    def writebundle(self, bases, node):
        cgversion = changegroup.safeversion(self.repo)
        if cgversion == '01':
            btype = 'HG10BZ'
            compression = None
        else:
            btype = 'HG20'
            compression = 'BZ'

        cg = changegroup.changegroupsubset(self.repo, bases, [node], 'shelve',
                                           version=cgversion)
        bundle2.writebundle(self.ui, cg, self.fname, btype, self.vfs,
                                compression=compression)

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
                raise error.Abort(_('this version of shelve is incompatible '
                                   'with the version used in this repo'))
            name = fp.readline().strip()
            wctx = fp.readline().strip()
            pendingctx = fp.readline().strip()
            parents = [nodemod.bin(h) for h in fp.readline().split()]
            stripnodes = [nodemod.bin(h) for h in fp.readline().split()]
            branchtorestore = fp.readline().strip()
        finally:
            fp.close()

        obj = cls()
        obj.name = name
        obj.wctx = repo[nodemod.bin(wctx)]
        obj.pendingctx = repo[nodemod.bin(pendingctx)]
        obj.parents = parents
        obj.stripnodes = stripnodes
        obj.branchtorestore = branchtorestore

        return obj

    @classmethod
    def save(cls, repo, name, originalwctx, pendingctx, stripnodes,
             branchtorestore):
        fp = repo.vfs(cls._filename, 'wb')
        fp.write('%i\n' % cls._version)
        fp.write('%s\n' % name)
        fp.write('%s\n' % nodemod.hex(originalwctx.node()))
        fp.write('%s\n' % nodemod.hex(pendingctx.node()))
        fp.write('%s\n' %
                 ' '.join([nodemod.hex(p) for p in repo.dirstate.parents()]))
        fp.write('%s\n' %
                 ' '.join([nodemod.hex(n) for n in stripnodes]))
        fp.write('%s\n' % branchtorestore)
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

def _aborttransaction(repo):
    '''Abort current transaction for shelve/unshelve, but keep dirstate
    '''
    backupname = 'dirstate.shelve'
    dirstatebackup = None
    try:
        # create backup of (un)shelved dirstate, because aborting transaction
        # should restore dirstate to one at the beginning of the
        # transaction, which doesn't include the result of (un)shelving
        fp = repo.vfs.open(backupname, "w")
        dirstatebackup = backupname
        # clearing _dirty/_dirtypl of dirstate by _writedirstate below
        # is unintentional. but it doesn't cause problem in this case,
        # because no code path refers them until transaction is aborted.
        repo.dirstate._writedirstate(fp) # write in-memory changes forcibly

        tr = repo.currenttransaction()
        tr.abort()

        # restore to backuped dirstate
        repo.vfs.rename(dirstatebackup, 'dirstate')
        dirstatebackup = None
    finally:
        if dirstatebackup:
            repo.vfs.unlink(dirstatebackup)

def createcmd(ui, repo, pats, opts):
    """subcommand that creates a new shelve"""
    with repo.wlock():
        cmdutil.checkunfinished(repo)
        return _docreatecmd(ui, repo, pats, opts)

def _docreatecmd(ui, repo, pats, opts):
    def mutableancestors(ctx):
        """return all mutable ancestors for ctx (included)

        Much faster than the revset ancestors(ctx) & draft()"""
        seen = set([nodemod.nullrev])
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
        raise error.Abort(_('cannot shelve while merging'))
    parent = parents[0]
    origbranch = wctx.branch()

    # we never need the user, so we use a generic user for all shelve operations
    user = 'shelve@localhost'
    label = repo._activebookmark or parent.branch() or 'default'

    # slashes aren't allowed in filenames, therefore we rename it
    label = label.replace('/', '_')

    def gennames():
        yield label
        for i in xrange(1, 100):
            yield '%s-%02d' % (label, i)

    if parent.node() != nodemod.nullid:
        desc = "changes to: %s" % parent.description().split('\n', 1)[0]
    else:
        desc = '(changes in empty repository)'

    if not opts.get('message'):
        opts['message'] = desc

    name = opts.get('name')

    lock = tr = None
    try:
        lock = repo.lock()

        # use an uncommitted transaction to generate the bundle to avoid
        # pull races. ensure we don't print the abort message to stderr.
        tr = repo.transaction('commit', report=lambda x: None)

        if name:
            if shelvedfile(repo, name, 'hg').exists():
                raise error.Abort(_("a shelved change named '%s' already exists"
                                   ) % name)
        else:
            for n in gennames():
                if not shelvedfile(repo, n, 'hg').exists():
                    name = n
                    break
            else:
                raise error.Abort(_("too many shelved changes named '%s'") %
                                 label)

        # ensure we are not creating a subdirectory or a hidden file
        if '/' in name or '\\' in name:
            raise error.Abort(_('shelved change names may not contain slashes'))
        if name.startswith('.'):
            raise error.Abort(_("shelved change names may not start with '.'"))
        interactive = opts.get('interactive', False)
        includeunknown = (opts.get('unknown', False) and
                          not opts.get('addremove', False))

        extra={}
        if includeunknown:
            s = repo.status(match=scmutil.match(repo[None], pats, opts),
                            unknown=True)
            if s.unknown:
                extra['shelve_unknown'] = '\0'.join(s.unknown)
                repo[None].add(s.unknown)

        if _iswctxonnewbranch(repo) and not _isbareshelve(pats, opts):
            # In non-bare shelve we don't store newly created branch
            # at bundled commit
            repo.dirstate.setbranch(repo['.'].branch())

        def commitfunc(ui, repo, message, match, opts):
            hasmq = util.safehasattr(repo, 'mq')
            if hasmq:
                saved, repo.mq.checkapplied = repo.mq.checkapplied, False
            backup = repo.ui.backupconfig('phases', 'new-commit')
            try:
                repo.ui. setconfig('phases', 'new-commit', phases.secret)
                editor = cmdutil.getcommiteditor(editform='shelve.shelve',
                                                 **opts)
                return repo.commit(message, user, opts.get('date'), match,
                                   editor=editor, extra=extra)
            finally:
                repo.ui.restoreconfig(backup)
                if hasmq:
                    repo.mq.checkapplied = saved

        def interactivecommitfunc(ui, repo, *pats, **opts):
            match = scmutil.match(repo['.'], pats, {})
            message = opts['message']
            return commitfunc(ui, repo, message, match, opts)
        if not interactive:
            node = cmdutil.commit(ui, repo, commitfunc, pats, opts)
        else:
            node = cmdutil.dorecord(ui, repo, interactivecommitfunc, None,
                                    False, cmdutil.recordfilter, *pats, **opts)
        if not node:
            stat = repo.status(match=scmutil.match(repo[None], pats, opts))
            if stat.deleted:
                ui.status(_("nothing changed (%d missing files, see "
                            "'hg status')\n") % len(stat.deleted))
            else:
                ui.status(_("nothing changed\n"))
            return 1

        bases = list(mutableancestors(repo[node]))
        shelvedfile(repo, name, 'hg').writebundle(bases, node)
        cmdutil.export(repo, [node],
                       fp=shelvedfile(repo, name, 'patch').opener('wb'),
                       opts=mdiff.diffopts(git=True))


        if ui.formatted():
            desc = util.ellipsis(desc, ui.termwidth())
        ui.status(_('shelved as %s\n') % name)
        hg.update(repo, parent.node())
        if origbranch != repo['.'].branch() and not _isbareshelve(pats, opts):
            repo.dirstate.setbranch(origbranch)

        _aborttransaction(repo)
    finally:
        lockmod.release(tr, lock)

def _isbareshelve(pats, opts):
    return (not pats
            and not opts.get('interactive', False)
            and not opts.get('include', False)
            and not opts.get('exclude', False))

def _iswctxonnewbranch(repo):
    return repo[None].branch() != repo['.'].branch()

def cleanupcmd(ui, repo):
    """subcommand that deletes all shelves"""

    with repo.wlock():
        for (name, _type) in repo.vfs.readdir(shelvedir):
            suffix = name.rsplit('.', 1)[-1]
            if suffix in ('hg', 'patch'):
                shelvedfile(repo, name).movetobackup()
            cleanupoldbackups(repo)

def deletecmd(ui, repo, pats):
    """subcommand that deletes a specific shelve"""
    if not pats:
        raise error.Abort(_('no shelved changes specified!'))
    with repo.wlock():
        try:
            for name in pats:
                for suffix in 'hg patch'.split():
                    shelvedfile(repo, name, suffix).movetobackup()
            cleanupoldbackups(repo)
        except OSError as err:
            if err.errno != errno.ENOENT:
                raise
            raise error.Abort(_("shelved change '%s' not found") % name)

def listshelves(repo):
    """return all shelves in repo as list of (time, filename)"""
    try:
        names = repo.vfs.readdir(shelvedir)
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
        with open(name + '.patch', 'rb') as fp:
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

def singlepatchcmds(ui, repo, pats, opts, subcommand):
    """subcommand that displays a single shelf"""
    if len(pats) != 1:
        raise error.Abort(_("--%s expects a single shelf") % subcommand)
    shelfname = pats[0]

    if not shelvedfile(repo, shelfname, 'patch').exists():
        raise error.Abort(_("cannot find shelf %s") % shelfname)

    listcmd(ui, repo, pats, opts)

def checkparents(repo, state):
    """check parent while resuming an unshelve"""
    if state.parents != repo.dirstate.parents():
        raise error.Abort(_('working directory parents do not match unshelve '
                           'state'))

def pathtofiles(repo, files):
    cwd = repo.getcwd()
    return [repo.pathto(f, cwd) for f in files]

def unshelveabort(ui, repo, state, opts):
    """subcommand that abort an in-progress unshelve"""
    with repo.lock():
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

            mergefiles(ui, repo, state.wctx, state.pendingctx)
            repair.strip(ui, repo, state.stripnodes, backup=False,
                         topic='shelve')
        finally:
            shelvedstate.clear(repo)
            ui.warn(_("unshelve of '%s' aborted\n") % state.name)

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
                util.rename(file, scmutil.origpath(ui, repo, file))
        ui.pushbuffer(True)
        cmdutil.revert(ui, repo, shelvectx, repo.dirstate.parents(),
                       *pathtofiles(repo, files),
                       **{'no_backup': True})
        ui.popbuffer()
    finally:
        ui.quiet = oldquiet

def restorebranch(ui, repo, branchtorestore):
    if branchtorestore and branchtorestore != repo.dirstate.branch():
        repo.dirstate.setbranch(branchtorestore)
        ui.status(_('marked working directory as branch %s\n')
                  % branchtorestore)

def unshelvecleanup(ui, repo, name, opts):
    """remove related files after an unshelve"""
    if not opts.get('keep'):
        for filetype in 'hg patch'.split():
            shelvedfile(repo, name, filetype).movetobackup()
        cleanupoldbackups(repo)

def unshelvecontinue(ui, repo, state, opts):
    """subcommand to continue an in-progress unshelve"""
    # We're finishing off a merge. First parent is our original
    # parent, second is the temporary "fake" commit we're unshelving.
    with repo.lock():
        checkparents(repo, state)
        ms = merge.mergestate.read(repo)
        if [f for f in ms if ms[f] == 'u']:
            raise error.Abort(
                _("unresolved conflicts, can't continue"),
                hint=_("see 'hg resolve', then 'hg unshelve --continue'"))

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
        restorebranch(ui, repo, state.branchtorestore)

        repair.strip(ui, repo, state.stripnodes, backup=False, topic='shelve')
        shelvedstate.clear(repo)
        unshelvecleanup(ui, repo, state.name, opts)
        ui.status(_("unshelve of '%s' complete\n") % state.name)

@command('unshelve',
         [('a', 'abort', None,
           _('abort an incomplete unshelve operation')),
          ('c', 'continue', None,
           _('continue an incomplete unshelve operation')),
          ('k', 'keep', None,
           _('keep shelve after unshelving')),
          ('t', 'tool', '', _('specify merge tool')),
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

    If bare shelved change(when no files are specified, without interactive,
    include and exclude option) was done on newly created branch it would
    restore branch information to the working directory.

    After a successful unshelve, the shelved changes are stored in a
    backup directory. Only the N most recent backups are kept. N
    defaults to 10 but can be overridden using the ``shelve.maxbackups``
    configuration option.

    .. container:: verbose

       Timestamp in seconds is used to decide order of backups. More
       than ``maxbackups`` backups are kept, if same timestamp
       prevents from deciding exact order of them, for safety.
    """
    with repo.wlock():
        return _dounshelve(ui, repo, *shelved, **opts)

def _dounshelve(ui, repo, *shelved, **opts):
    abortf = opts.get('abort')
    continuef = opts.get('continue')
    if not abortf and not continuef:
        cmdutil.checkunfinished(repo)

    if abortf or continuef:
        if abortf and continuef:
            raise error.Abort(_('cannot use both abort and continue'))
        if shelved:
            raise error.Abort(_('cannot combine abort/continue with '
                               'naming a shelved change'))
        if abortf and opts.get('tool', False):
            ui.warn(_('tool option will be ignored\n'))

        try:
            state = shelvedstate.load(repo)
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            cmdutil.wrongtooltocontinue(repo, _('unshelve'))

        if abortf:
            return unshelveabort(ui, repo, state, opts)
        elif continuef:
            return unshelvecontinue(ui, repo, state, opts)
    elif len(shelved) > 1:
        raise error.Abort(_('can only unshelve one change at a time'))
    elif not shelved:
        shelved = listshelves(repo)
        if not shelved:
            raise error.Abort(_('no shelved changes to apply!'))
        basename = util.split(shelved[0][1])[1]
        ui.status(_("unshelving change '%s'\n") % basename)
    else:
        basename = shelved[0]

    if not shelvedfile(repo, basename, 'patch').exists():
        raise error.Abort(_("shelved change '%s' not found") % basename)

    oldquiet = ui.quiet
    lock = tr = None
    forcemerge = ui.backupconfig('ui', 'forcemerge')
    try:
        ui.setconfig('ui', 'forcemerge', opts.get('tool', ''), 'unshelve')
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

        # Store pending changes in a commit and remember added in case a shelve
        # contains unknown files that are part of the pending change
        s = repo.status()
        addedbefore = frozenset(s.added)
        if s.modified or s.added or s.removed or s.deleted:
            ui.status(_("temporarily committing pending changes "
                        "(restore with 'hg unshelve --abort')\n"))
            def commitfunc(ui, repo, message, match, opts):
                hasmq = util.safehasattr(repo, 'mq')
                if hasmq:
                    saved, repo.mq.checkapplied = repo.mq.checkapplied, False

                backup = repo.ui.backupconfig('phases', 'new-commit')
                try:
                    repo.ui.setconfig('phases', 'new-commit', phases.secret)
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

        branchtorestore = ''
        if shelvectx.branch() != shelvectx.p1().branch():
            branchtorestore = shelvectx.branch()

        # If the shelve is not immediately on top of the commit
        # we'll be merging with, rebase it to be on top.
        if tmpwctx.node() != shelvectx.parents()[0].node():
            ui.status(_('rebasing shelved changes\n'))
            try:
                rebase.rebase(ui, repo, **{
                    'rev' : [shelvectx.rev()],
                    'dest' : str(tmpwctx.rev()),
                    'keep' : True,
                    'tool' : opts.get('tool', ''),
                })
            except error.InterventionRequired:
                tr.close()

                stripnodes = [repo.changelog.node(rev)
                              for rev in xrange(oldtiprev, len(repo))]
                shelvedstate.save(repo, basename, pctx, tmpwctx, stripnodes,
                                  branchtorestore)

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
        restorebranch(ui, repo, branchtorestore)

        # Forget any files that were unknown before the shelve, unknown before
        # unshelve started, but are now added.
        shelveunknown = shelvectx.extra().get('shelve_unknown')
        if shelveunknown:
            shelveunknown = frozenset(shelveunknown.split('\0'))
            addedafter = frozenset(repo.status().added)
            toforget = (addedafter & shelveunknown) - addedbefore
            repo[None].forget(toforget)

        shelvedstate.clear(repo)

        # The transaction aborting will strip all the commits for us,
        # but it doesn't update the inmemory structures, so addchangegroup
        # hooks still fire and try to operate on the missing commits.
        # Clean up manually to prevent this.
        repo.unfiltered().changelog.strip(oldtiprev, tr)

        unshelvecleanup(ui, repo, basename, opts)

        _aborttransaction(repo)
    finally:
        ui.quiet = oldquiet
        if tr:
            tr.release()
        lockmod.release(lock)
        ui.restoreconfig(forcemerge)

@command('shelve',
         [('A', 'addremove', None,
           _('mark new/missing files as added/removed before shelving')),
          ('u', 'unknown', None,
           _('store unknown files in the shelve')),
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

    In bare shelve(when no files are specified, without interactive,
    include and exclude option), shelving remembers information if the
    working directory was on newly created branch, in other words working
    directory was on different branch than its first parent. In this
    situation unshelving restores branch information to the working directory.

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
    allowables = [
        ('addremove', set(['create'])), # 'create' is pseudo action
        ('unknown', set(['create'])),
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
        if opts.get(opt):
            for i, allowable in allowables:
                if opts[i] and opt not in allowable:
                    raise error.Abort(_("options '--%s' and '--%s' may not be "
                                       "used together") % (opt, i))
            return True
    if checkopt('cleanup'):
        if pats:
            raise error.Abort(_("cannot specify names when using '--cleanup'"))
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
    cmdutil.afterresolvedstates.append(
        [shelvedstate._filename, _('hg unshelve --continue')])
