# Copyright (C) 2007 Brendan Cully <brendan@kublai.com>
# Published under the GNU GPL

'''
imerge - interactive merge
'''

from mercurial.i18n import _
from mercurial.node import hex, short
from mercurial import commands, cmdutil, dispatch, fancyopts
from mercurial import hg, filemerge, util, revlog
import os, tarfile

class InvalidStateFileException(Exception): pass

class ImergeStateFile(object):
    def __init__(self, im):
        self.im = im

    def save(self, dest):
        tf = tarfile.open(dest, 'w:gz')

        st = os.path.join(self.im.path, 'status')
        tf.add(st, os.path.join('.hg', 'imerge', 'status'))

        for f in self.im.resolved:
            (fd, fo) = self.im.conflicts[f]
            abssrc = self.im.repo.wjoin(fd)
            tf.add(abssrc, fd)

        tf.close()

    def load(self, source):
        wlock = self.im.repo.wlock()
        lock = self.im.repo.lock()

        tf = tarfile.open(source, 'r')
        contents = tf.getnames()
        # tarfile normalizes path separators to '/'
        statusfile = '.hg/imerge/status'
        if statusfile not in contents:
            raise InvalidStateFileException('no status file')

        tf.extract(statusfile, self.im.repo.root)
        p1, p2 = self.im.load()
        if self.im.repo.dirstate.parents()[0] != p1.node():
            hg.clean(self.im.repo, p1.node())
        self.im.start(p2.node())
        for tarinfo in tf:
            tf.extract(tarinfo, self.im.repo.root)
        self.im.load()

class Imerge(object):
    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo

        self.path = repo.join('imerge')
        self.opener = util.opener(self.path)

        self.wctx = self.repo.workingctx()
        self.conflicts = {}
        self.resolved = []

    def merging(self):
        return len(self.wctx.parents()) > 1

    def load(self):
        # status format. \0-delimited file, fields are
        # p1, p2, conflict count, conflict filenames, resolved filenames
        # conflict filenames are tuples of localname, remoteorig, remotenew

        statusfile = self.opener('status')

        status = statusfile.read().split('\0')
        if len(status) < 3:
            raise util.Abort('invalid imerge status file')

        try:
            parents = [self.repo.changectx(n) for n in status[:2]]
        except revlog.LookupError, e:
            raise util.Abort(_('merge parent %s not in repository') %
                             short(e.name))

        status = status[2:]
        conflicts = int(status.pop(0)) * 3
        self.resolved = status[conflicts:]
        for i in xrange(0, conflicts, 3):
            self.conflicts[status[i]] = (status[i+1], status[i+2])

        return parents

    def save(self):
        lock = self.repo.lock()

        if not os.path.isdir(self.path):
            os.mkdir(self.path)
        statusfile = self.opener('status', 'wb')

        out = [hex(n.node()) for n in self.wctx.parents()]
        out.append(str(len(self.conflicts)))
        conflicts = self.conflicts.items()
        conflicts.sort()
        for fw, fd_fo in conflicts:
            out.append(fw)
            out.extend(fd_fo)
        out.extend(self.resolved)

        statusfile.write('\0'.join(out))

    def remaining(self):
        return [f for f in self.conflicts if f not in self.resolved]

    def filemerge(self, fn, interactive=True):
        wlock = self.repo.wlock()

        (fd, fo) = self.conflicts[fn]
        p1, p2 = self.wctx.parents()

        # this could be greatly improved
        realmerge = os.environ.get('HGMERGE')
        if not interactive:
            os.environ['HGMERGE'] = 'internal:merge'

        # The filemerge ancestor algorithm does not work if self.wctx
        # already has two parents (in normal merge it doesn't yet). But
        # this is very dirty.
        self.wctx._parents.pop()
        try:
            # TODO: we should probably revert the file if merge fails
            return filemerge.filemerge(self.repo, fn, fd, fo, self.wctx, p2)
        finally:
            self.wctx._parents.append(p2)
            if realmerge:
                os.environ['HGMERGE'] = realmerge
            elif not interactive:
                del os.environ['HGMERGE']

    def start(self, rev=None):
        _filemerge = filemerge.filemerge
        def filemerge_(repo, fw, fd, fo, wctx, mctx):
            self.conflicts[fw] = (fd, fo)

        filemerge.filemerge = filemerge_
        commands.merge(self.ui, self.repo, rev=rev)
        filemerge.filemerge = _filemerge

        self.wctx = self.repo.workingctx()
        self.save()

    def resume(self):
        self.load()

        dp = self.repo.dirstate.parents()
        p1, p2 = self.wctx.parents()
        if p1.node() != dp[0] or p2.node() != dp[1]:
            raise util.Abort('imerge state does not match working directory')

    def next(self):
        remaining = self.remaining()
        return remaining and remaining[0]

    def resolve(self, files):
        resolved = dict.fromkeys(self.resolved)
        for fn in files:
            if fn not in self.conflicts:
                raise util.Abort('%s is not in the merge set' % fn)
            resolved[fn] = True
        self.resolved = resolved.keys()
        self.resolved.sort()
        self.save()
        return 0

    def unresolve(self, files):
        resolved = dict.fromkeys(self.resolved)
        for fn in files:
            if fn not in resolved:
                raise util.Abort('%s is not resolved' % fn)
            del resolved[fn]
        self.resolved = resolved.keys()
        self.resolved.sort()
        self.save()
        return 0

    def pickle(self, dest):
        '''write current merge state to file to be resumed elsewhere'''
        state = ImergeStateFile(self)
        return state.save(dest)

    def unpickle(self, source):
        '''read merge state from file'''
        state = ImergeStateFile(self)
        return state.load(source)

def load(im, source):
    if im.merging():
        raise util.Abort('there is already a merge in progress '
                         '(update -C <rev> to abort it)' )
    m, a, r, d =  im.repo.status()[:4]
    if m or a or r or d:
        raise util.Abort('working directory has uncommitted changes')

    rc = im.unpickle(source)
    if not rc:
        status(im)
    return rc

def merge_(im, filename=None, auto=False):
    success = True
    if auto and not filename:
        for fn in im.remaining():
            rc = im.filemerge(fn, interactive=False)
            if rc:
                success = False
            else:
                im.resolve([fn])
        if success:
            im.ui.write('all conflicts resolved\n')
        else:
            status(im)
        return 0

    if not filename:
        filename = im.next()
        if not filename:
            im.ui.write('all conflicts resolved\n')
            return 0

    rc = im.filemerge(filename, interactive=not auto)
    if not rc:
        im.resolve([filename])
        if not im.next():
            im.ui.write('all conflicts resolved\n')
    return rc

def next(im):
    n = im.next()
    if n:
        im.ui.write('%s\n' % n)
    else:
        im.ui.write('all conflicts resolved\n')
    return 0

def resolve(im, *files):
    if not files:
        raise util.Abort('resolve requires at least one filename')
    return im.resolve(files)

def save(im, dest):
    return im.pickle(dest)

def status(im, **opts):
    if not opts.get('resolved') and not opts.get('unresolved'):
        opts['resolved'] = True
        opts['unresolved'] = True

    if im.ui.verbose:
        p1, p2 = [short(p.node()) for p in im.wctx.parents()]
        im.ui.note(_('merging %s and %s\n') % (p1, p2))

    conflicts = im.conflicts.keys()
    conflicts.sort()
    remaining = dict.fromkeys(im.remaining())
    st = []
    for fn in conflicts:
        if opts.get('no_status'):
            mode = ''
        elif fn in remaining:
            mode = 'U '
        else:
            mode = 'R '
        if ((opts.get('resolved') and fn not in remaining)
            or (opts.get('unresolved') and fn in remaining)):
            st.append((mode, fn))
    st.sort()
    for (mode, fn) in st:
        if im.ui.verbose:
            fo, fd = im.conflicts[fn]
            if fd != fn:
                fn = '%s (%s)' % (fn, fd)
        im.ui.write('%s%s\n' % (mode, fn))
    if opts.get('unresolved') and not remaining:
        im.ui.write(_('all conflicts resolved\n'))

    return 0

def unresolve(im, *files):
    if not files:
        raise util.Abort('unresolve requires at least one filename')
    return im.unresolve(files)

subcmdtable = {
    'load': (load, []),
    'merge':
        (merge_,
         [('a', 'auto', None, _('automatically resolve if possible'))]),
    'next': (next, []),
    'resolve': (resolve, []),
    'save': (save, []),
    'status':
        (status,
         [('n', 'no-status', None, _('hide status prefix')),
          ('', 'resolved', None, _('only show resolved conflicts')),
          ('', 'unresolved', None, _('only show unresolved conflicts'))]),
    'unresolve': (unresolve, [])
}

def dispatch_(im, args, opts):
    def complete(s, choices):
        candidates = []
        for choice in choices:
            if choice.startswith(s):
                candidates.append(choice)
        return candidates

    c, args = args[0], list(args[1:])
    cmd = complete(c, subcmdtable.keys())
    if not cmd:
        raise cmdutil.UnknownCommand('imerge ' + c)
    if len(cmd) > 1:
        cmd.sort()
        raise cmdutil.AmbiguousCommand('imerge ' + c, cmd)
    cmd = cmd[0]

    func, optlist = subcmdtable[cmd]
    opts = {}
    try:
        args = fancyopts.fancyopts(args, optlist, opts)
        return func(im, *args, **opts)
    except fancyopts.getopt.GetoptError, inst:
        raise dispatch.ParseError('imerge', '%s: %s' % (cmd, inst))
    except TypeError:
        raise dispatch.ParseError('imerge', _('%s: invalid arguments') % cmd)

def imerge(ui, repo, *args, **opts):
    '''interactive merge

    imerge lets you split a merge into pieces. When you start a merge
    with imerge, the names of all files with conflicts are recorded.
    You can then merge any of these files, and if the merge is
    successful, they will be marked as resolved. When all files are
    resolved, the merge is complete.

    If no merge is in progress, hg imerge [rev] will merge the working
    directory with rev (defaulting to the other head if the repository
    only has two heads). You may also resume a saved merge with
    hg imerge load <file>.

    If a merge is in progress, hg imerge will default to merging the
    next unresolved file.

    The following subcommands are available:

    status:
      show the current state of the merge
      options:
        -n --no-status:  do not print the status prefix
           --resolved:   only print resolved conflicts
           --unresolved: only print unresolved conflicts
    next:
      show the next unresolved file merge
    merge [<file>]:
      merge <file>. If the file merge is successful, the file will be
      recorded as resolved. If no file is given, the next unresolved
      file will be merged.
    resolve <file>...:
      mark files as successfully merged
    unresolve <file>...:
      mark files as requiring merging.
    save <file>:
      save the state of the merge to a file to be resumed elsewhere
    load <file>:
      load the state of the merge from a file created by save
    '''

    im = Imerge(ui, repo)

    if im.merging():
        im.resume()
    else:
        rev = opts.get('rev')
        if rev and args:
            raise util.Abort('please specify just one revision')

        if len(args) == 2 and args[0] == 'load':
            pass
        else:
            if args:
                rev = args[0]
            im.start(rev=rev)
            if opts.get('auto'):
                args = ['merge', '--auto']
            else:
                args = ['status']

    if not args:
        args = ['merge']

    return dispatch_(im, args, opts)

cmdtable = {
    '^imerge':
    (imerge,
     [('r', 'rev', '', _('revision to merge')),
      ('a', 'auto', None, _('automatically merge where possible'))],
      'hg imerge [command]')
}
