# Copyright (C) 2007 Brendan Cully <brendan@kublai.com>
# Published under the GNU GPL

'''
imerge - interactive merge
'''

from mercurial.i18n import _
from mercurial.node import *
from mercurial import commands, cmdutil, hg, merge, util
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
            abssrc = self.im.repo.wjoin(f)
            tf.add(abssrc, f)

        tf.close()

    def load(self, source):
        wlock = self.im.repo.wlock()
        lock = self.im.repo.lock()

        tf = tarfile.open(source, 'r')
        contents = tf.getnames()
        statusfile = os.path.join('.hg', 'imerge', 'status')
        if statusfile not in contents:
            raise InvalidStateFileException('no status file')

        tf.extract(statusfile, self.im.repo.root)
        self.im.load()
        p1 = self.im.parents[0].node()
        p2 = self.im.parents[1].node()
        if self.im.repo.dirstate.parents()[0] != p1:
            hg.clean(self.im.repo, self.im.parents[0].node())
        self.im.start(p2)
        tf.extractall(self.im.repo.root)
        self.im.load()

class Imerge(object):
    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo

        self.path = repo.join('imerge')
        self.opener = util.opener(self.path)

        self.parents = [self.repo.changectx(n)
                        for n in self.repo.dirstate.parents()]
        self.conflicts = {}
        self.resolved = []

    def merging(self):
        return self.parents[1].node() != nullid

    def load(self):
        # status format. \0-delimited file, fields are
        # p1, p2, conflict count, conflict filenames, resolved filenames
        # conflict filenames are pairs of localname, remotename

        statusfile = self.opener('status')

        status = statusfile.read().split('\0')
        if len(status) < 3:
            raise util.Abort('invalid imerge status file')

        try:
            self.parents = [self.repo.changectx(n) for n in status[:2]]
        except LookupError:
            raise util.Abort('merge parent %s not in repository' % short(p))

        status = status[2:]
        conflicts = int(status.pop(0)) * 2
        self.resolved = status[conflicts:]
        for i in xrange(0, conflicts, 2):
            self.conflicts[status[i]] = status[i+1]

    def save(self):
        lock = self.repo.lock()

        if not os.path.isdir(self.path):
            os.mkdir(self.path)
        fd = self.opener('status', 'wb')

        out = [hex(n.node()) for n in self.parents]
        out.append(str(len(self.conflicts)))
        for f in sorted(self.conflicts):
            out.append(f)
            out.append(self.conflicts[f])
        out.extend(self.resolved)

        fd.write('\0'.join(out))

    def remaining(self):
        return [f for f in self.conflicts if f not in self.resolved]

    def filemerge(self, fn):
        wlock = self.repo.wlock()

        fo = self.conflicts[fn]
        return merge.filemerge(self.repo, fn, fo, self.parents[0],
                               self.parents[1])

    def start(self, rev=None):
        _filemerge = merge.filemerge
        def filemerge(repo, fw, fo, wctx, mctx):
            self.conflicts[fw] = fo

        merge.filemerge = filemerge
        commands.merge(self.ui, self.repo, rev=rev)
        merge.filemerge = _filemerge

        self.parents = [self.repo.changectx(n)
                        for n in self.repo.dirstate.parents()]
        self.save()

    def resume(self):
        self.load()

        dp = self.repo.dirstate.parents()
        if self.parents[0].node() != dp[0] or self.parents[1].node() != dp[1]:
            raise util.Abort('imerge state does not match working directory')

    def status(self):
        self.ui.write('merging %s and %s\n' % \
                      (short(self.parents[0].node()),
                       short(self.parents[1].node())))

        if self.resolved:
            self.ui.write('resolved:\n')
            for fn in self.resolved:
                self.ui.write('  %s\n' % fn)
        remaining = [f for f in self.conflicts if f not in self.resolved]
        if remaining:
            self.ui.write('remaining:\n')
            for fn in remaining:
                fo = self.conflicts[fn]
                if fn == fo:
                    self.ui.write('  %s\n' % (fn,))
                else:
                    self.ui.write('  %s (%s)\n' % (fn, fo))
        else:
            self.ui.write('all conflicts resolved\n')

    def next(self):
        remaining = self.remaining()
        return remaining and remaining[0]

    def resolve(self, files):
        resolved = dict.fromkeys(self.resolved)
        for fn in files:
            if fn not in self.conflicts:
                raise util.Abort('%s is not in the merge set' % fn)
            resolved[fn] = True
        self.resolved = sorted(resolved)
        self.save()
        return 0

    def unresolve(self, files):
        resolved = dict.fromkeys(self.resolved)
        for fn in files:
            if fn not in resolved:
                raise util.Abort('%s is not resolved' % fn)
            del resolved[fn]
        self.resolved = sorted(resolved)
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
        im.status()
    return rc

def merge_(im, filename=None):
    if not filename:
        filename = im.next()
        if not filename:
            im.ui.write('all conflicts resolved\n')
            return 0

    rc = im.filemerge(filename)
    if not rc:
        im.resolve([filename])
        if not im.next():
            im.ui.write('all conflicts resolved\n')
            return 0
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

def status(im):
    im.status()
    return 0

def unresolve(im, *files):
    if not files:
        raise util.Abort('unresolve requires at least one filename')
    return im.unresolve(files)

subcmdtable = {
    'load': load,
    'merge': merge_,
    'next': next,
    'resolve': resolve,
    'save': save,
    'status': status,
    'unresolve': unresolve
}

def dispatch(im, args, opts):
    def complete(s, choices):
        candidates = []
        for choice in choices:
            if choice.startswith(s):
                candidates.append(choice)
        return candidates

    c, args = args[0], args[1:]
    cmd = complete(c, subcmdtable.keys())
    if not cmd:
        raise cmdutil.UnknownCommand('imerge ' + c)
    if len(cmd) > 1:
        raise cmdutil.AmbiguousCommand('imerge ' + c, sorted(cmd))
    cmd = cmd[0]

    func = subcmdtable[cmd]
    try:
        return func(im, *args)
    except TypeError:
        raise cmdutil.ParseError('imerge', '%s: invalid arguments' % cmd)

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
            args = ['status']

    if not args:
        args = ['merge']

    return dispatch(im, args, opts)

cmdtable = {
    '^imerge':
    (imerge,
     [('r', 'rev', '', _('revision to merge'))], 'hg imerge [command]')
}
