# server.py - inotify status server
#
# Copyright 2006, 2007, 2008 Bryan O'Sullivan <bos@serpentine.com>
# Copyright 2007, 2008 Brendan Cully <brendan@kublai.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.i18n import gettext as _
from mercurial import osutil, ui, util
import common
import errno, os, select, socket, stat, struct, sys, time

try:
    import hgext.inotify.linux as inotify
    from hgext.inotify.linux import watcher
except ImportError:
    print >> sys.stderr, '*** native support is required for this extension'
    raise

class AlreadyStartedException(Exception): pass

def join(a, b):
    if a:
        if a[-1] == '/':
            return a + b
        return a + '/' + b
    return b

walk_ignored_errors = (errno.ENOENT, errno.ENAMETOOLONG)

def walkrepodirs(repo):
    '''Iterate over all subdirectories of this repo.
    Exclude the .hg directory, any nested repos, and ignored dirs.'''
    rootslash = repo.root + os.sep
    def walkit(dirname, top):
        hginside = False
        try:
            for name, kind in osutil.listdir(rootslash + dirname):
                if kind == stat.S_IFDIR:
                    if name == '.hg':
                        hginside = True
                        if not top: break
                    else:
                        d = join(dirname, name)
                        if repo.dirstate._ignore(d):
                            continue
                        for subdir, hginsub in walkit(d, False):
                            if not hginsub:
                                yield subdir, False
        except OSError, err:
            if err.errno not in walk_ignored_errors:
                raise
        yield rootslash + dirname, hginside
    for dirname, hginside in walkit('', True):
        yield dirname

def walk(repo, root):
    '''Like os.walk, but only yields regular files.'''

    # This function is critical to performance during startup.

    reporoot = root == ''
    rootslash = repo.root + os.sep

    def walkit(root, reporoot):
        files, dirs = [], []
        hginside = False

        try:
            fullpath = rootslash + root
            for name, kind in osutil.listdir(fullpath):
                if kind == stat.S_IFDIR:
                    if name == '.hg':
                        hginside = True
                        if reporoot:
                            continue
                        else:
                            break
                    dirs.append(name)
                elif kind in (stat.S_IFREG, stat.S_IFLNK):
                    path = join(root, name)
                    files.append((name, kind))

            yield hginside, fullpath, dirs, files

            for subdir in dirs:
                path = join(root, subdir)
                if repo.dirstate._ignore(path):
                    continue
                for result in walkit(path, False):
                    if not result[0]:
                        yield result
        except OSError, err:
            if err.errno not in walk_ignored_errors:
                raise
    for result in walkit(root, reporoot):
        yield result[1:]

def _explain_watch_limit(ui, repo, count):
    path = '/proc/sys/fs/inotify/max_user_watches'
    try:
        limit = int(file(path).read())
    except IOError, err:
        if err.errno != errno.ENOENT:
            raise
        raise util.Abort(_('this system does not seem to '
                           'support inotify'))
    ui.warn(_('*** the current per-user limit on the number '
              'of inotify watches is %s\n') % limit)
    ui.warn(_('*** this limit is too low to watch every '
              'directory in this repository\n'))
    ui.warn(_('*** counting directories: '))
    ndirs = len(list(walkrepodirs(repo)))
    ui.warn(_('found %d\n') % ndirs)
    newlimit = min(limit, 1024)
    while newlimit < ((limit + ndirs) * 1.1):
        newlimit *= 2
    ui.warn(_('*** to raise the limit from %d to %d (run as root):\n') %
            (limit, newlimit))
    ui.warn(_('***  echo %d > %s\n') % (newlimit, path))
    raise util.Abort(_('cannot watch %s until inotify watch limit is raised')
                     % repo.root)

class Watcher(object):
    poll_events = select.POLLIN
    statuskeys = 'almr!?'

    def __init__(self, ui, repo, master):
        self.ui = ui
        self.repo = repo
        self.wprefix = self.repo.wjoin('')
        self.timeout = None
        self.master = master
        self.mask = (
            inotify.IN_ATTRIB |
            inotify.IN_CREATE |
            inotify.IN_DELETE |
            inotify.IN_DELETE_SELF |
            inotify.IN_MODIFY |
            inotify.IN_MOVED_FROM |
            inotify.IN_MOVED_TO |
            inotify.IN_MOVE_SELF |
            inotify.IN_ONLYDIR |
            inotify.IN_UNMOUNT |
            0)
        try:
            self.watcher = watcher.Watcher()
        except OSError, err:
            raise util.Abort(_('inotify service not available: %s') %
                             err.strerror)
        self.threshold = watcher.Threshold(self.watcher)
        self.registered = True
        self.fileno = self.watcher.fileno

        self.repo.dirstate.__class__.inotifyserver = True

        self.tree = {}
        self.statcache = {}
        self.statustrees = dict([(s, {}) for s in self.statuskeys])

        self.watches = 0
        self.last_event = None

        self.eventq = {}
        self.deferred = 0

        self.ds_info = self.dirstate_info()
        self.scan()

    def event_time(self):
        last = self.last_event
        now = time.time()
        self.last_event = now

        if last is None:
            return 'start'
        delta = now - last
        if delta < 5:
            return '+%.3f' % delta
        if delta < 50:
            return '+%.2f' % delta
        return '+%.1f' % delta

    def dirstate_info(self):
        try:
            st = os.lstat(self.repo.join('dirstate'))
            return st.st_mtime, st.st_ino
        except OSError, err:
            if err.errno != errno.ENOENT:
                raise
            return 0, 0

    def add_watch(self, path, mask):
        if not path:
            return
        if self.watcher.path(path) is None:
            if self.ui.debugflag:
                self.ui.note(_('watching %r\n') % path[len(self.wprefix):])
            try:
                self.watcher.add(path, mask)
                self.watches += 1
            except OSError, err:
                if err.errno in (errno.ENOENT, errno.ENOTDIR):
                    return
                if err.errno != errno.ENOSPC:
                    raise
                _explain_watch_limit(self.ui, self.repo, self.watches)

    def setup(self):
        self.ui.note(_('watching directories under %r\n') % self.repo.root)
        self.add_watch(self.repo.path, inotify.IN_DELETE)
        self.check_dirstate()

    def wpath(self, evt):
        path = evt.fullpath
        if path == self.repo.root:
            return ''
        if path.startswith(self.wprefix):
            return path[len(self.wprefix):]
        raise 'wtf? ' + path

    def dir(self, tree, path):
        if path:
            for name in path.split('/'):
                tree.setdefault(name, {})
                tree = tree[name]
        return tree

    def lookup(self, path, tree):
        if path:
            try:
                for name in path.split('/'):
                    tree = tree[name]
            except KeyError:
                return 'x'
            except TypeError:
                return 'd'
        return tree

    def split(self, path):
        c = path.rfind('/')
        if c == -1:
            return '', path
        return path[:c], path[c+1:]

    def filestatus(self, fn, st):
        try:
            type_, mode, size, time = self.repo.dirstate._map[fn][:4]
        except KeyError:
            type_ = '?'
        if type_ == 'n':
            if not st:
                return '!'
            st_mode, st_size, st_mtime = st
            if size and (size != st_size or (mode ^ st_mode) & 0100):
                return 'm'
            if time != int(st_mtime):
                return 'l'
            return 'n'
        if type_ in 'ma' and not st:
            return '!'
        if type_ == '?' and self.repo.dirstate._ignore(fn):
            return 'i'
        return type_

    def updatestatus(self, wfn, st=None, status=None, oldstatus=None):
        if st:
            status = self.filestatus(wfn, st)
        else:
            self.statcache.pop(wfn, None)
        root, fn = self.split(wfn)
        d = self.dir(self.tree, root)
        if oldstatus is None:
            oldstatus = d.get(fn)
        isdir = False
        if oldstatus:
            try:
                if not status:
                    if oldstatus in 'almn':
                        status = '!'
                    elif oldstatus == 'r':
                        status = 'r'
            except TypeError:
                # oldstatus may be a dict left behind by a deleted
                # directory
                isdir = True
            else:
                if oldstatus in self.statuskeys and oldstatus != status:
                    del self.dir(self.statustrees[oldstatus], root)[fn]
        if self.ui.debugflag and oldstatus != status:
            if isdir:
                self.ui.note('status: %r dir(%d) -> %s\n' %
                             (wfn, len(oldstatus), status))
            else:
                self.ui.note('status: %r %s -> %s\n' %
                             (wfn, oldstatus, status))
        if not isdir:
            if status and status != 'i':
                d[fn] = status
                if status in self.statuskeys:
                    dd = self.dir(self.statustrees[status], root)
                    if oldstatus != status or fn not in dd:
                        dd[fn] = status
            else:
                d.pop(fn, None)

    def check_deleted(self, key):
        # Files that had been deleted but were present in the dirstate
        # may have vanished from the dirstate; we must clean them up.
        nuke = []
        for wfn, ignore in self.walk(key, self.statustrees[key]):
            if wfn not in self.repo.dirstate:
                nuke.append(wfn)
        for wfn in nuke:
            root, fn = self.split(wfn)
            del self.dir(self.statustrees[key], root)[fn]
            del self.dir(self.tree, root)[fn]

    def scan(self, topdir=''):
        self.handle_timeout()
        ds = self.repo.dirstate._map.copy()
        self.add_watch(join(self.repo.root, topdir), self.mask)
        for root, dirs, entries in walk(self.repo, topdir):
            for d in dirs:
                self.add_watch(join(root, d), self.mask)
            wroot = root[len(self.wprefix):]
            d = self.dir(self.tree, wroot)
            for fn, kind in entries:
                wfn = join(wroot, fn)
                self.updatestatus(wfn, self.getstat(wfn))
                ds.pop(wfn, None)
        wtopdir = topdir
        if wtopdir and wtopdir[-1] != '/':
            wtopdir += '/'
        for wfn, state in ds.iteritems():
            if not wfn.startswith(wtopdir):
                continue
            status = state[0]
            st = self.getstat(wfn)
            if status == 'r' and not st:
                self.updatestatus(wfn, st, status=status)
            else:
                self.updatestatus(wfn, st, oldstatus=status)
        self.check_deleted('!')
        self.check_deleted('r')

    def check_dirstate(self):
        ds_info = self.dirstate_info()
        if ds_info == self.ds_info:
            return
        self.ds_info = ds_info
        if not self.ui.debugflag:
            self.last_event = None
        self.ui.note(_('%s dirstate reload\n') % self.event_time())
        self.repo.dirstate.invalidate()
        self.scan()
        self.ui.note(_('%s end dirstate reload\n') % self.event_time())

    def walk(self, states, tree, prefix=''):
        # This is the "inner loop" when talking to the client.

        for name, val in tree.iteritems():
            path = join(prefix, name)
            try:
                if val in states:
                    yield path, val
            except TypeError:
                for p in self.walk(states, val, path):
                    yield p

    def update_hgignore(self):
        # An update of the ignore file can potentially change the
        # states of all unknown and ignored files.

        # XXX If the user has other ignore files outside the repo, or
        # changes their list of ignore files at run time, we'll
        # potentially never see changes to them.  We could get the
        # client to report to us what ignore data they're using.
        # But it's easier to do nothing than to open that can of
        # worms.

        if self.repo.dirstate.ignorefunc is not None:
            self.repo.dirstate.ignorefunc = None
            self.ui.note('rescanning due to .hgignore change\n')
            self.scan()

    def getstat(self, wpath):
        try:
            return self.statcache[wpath]
        except KeyError:
            try:
                return self.stat(wpath)
            except OSError, err:
                if err.errno != errno.ENOENT:
                    raise

    def stat(self, wpath):
        try:
            st = os.lstat(join(self.wprefix, wpath))
            ret = st.st_mode, st.st_size, st.st_mtime
            self.statcache[wpath] = ret
            return ret
        except OSError, err:
            self.statcache.pop(wpath, None)
            raise

    def created(self, wpath):
        if wpath == '.hgignore':
            self.update_hgignore()
        try:
            st = self.stat(wpath)
            if stat.S_ISREG(st[0]):
                self.updatestatus(wpath, st)
        except OSError, err:
            pass

    def modified(self, wpath):
        if wpath == '.hgignore':
            self.update_hgignore()
        try:
            st = self.stat(wpath)
            if stat.S_ISREG(st[0]):
                if self.repo.dirstate[wpath] in 'lmn':
                    self.updatestatus(wpath, st)
        except OSError:
            pass

    def deleted(self, wpath):
        if wpath == '.hgignore':
            self.update_hgignore()
        elif wpath.startswith('.hg/'):
            if wpath == '.hg/wlock':
                self.check_dirstate()
            return

        self.updatestatus(wpath, None)

    def schedule_work(self, wpath, evt):
        self.eventq.setdefault(wpath, [])
        prev = self.eventq[wpath]
        try:
            if prev and evt == 'm' and prev[-1] in 'cm':
                return
            self.eventq[wpath].append(evt)
        finally:
            self.deferred += 1
            self.timeout = 250

    def deferred_event(self, wpath, evt):
        if evt == 'c':
            self.created(wpath)
        elif evt == 'm':
            self.modified(wpath)
        elif evt == 'd':
            self.deleted(wpath)

    def process_create(self, wpath, evt):
        if self.ui.debugflag:
            self.ui.note(_('%s event: created %s\n') %
                         (self.event_time(), wpath))

        if evt.mask & inotify.IN_ISDIR:
            self.scan(wpath)
        else:
            self.schedule_work(wpath, 'c')

    def process_delete(self, wpath, evt):
        if self.ui.debugflag:
            self.ui.note(('%s event: deleted %s\n') %
                         (self.event_time(), wpath))

        if evt.mask & inotify.IN_ISDIR:
            self.scan(wpath)
        else:
            self.schedule_work(wpath, 'd')

    def process_modify(self, wpath, evt):
        if self.ui.debugflag:
            self.ui.note(_('%s event: modified %s\n') %
                         (self.event_time(), wpath))

        if not (evt.mask & inotify.IN_ISDIR):
            self.schedule_work(wpath, 'm')

    def process_unmount(self, evt):
        self.ui.warn(_('filesystem containing %s was unmounted\n') %
                     evt.fullpath)
        sys.exit(0)

    def handle_event(self, fd, event):
        if self.ui.debugflag:
            self.ui.note('%s readable: %d bytes\n' %
                         (self.event_time(), self.threshold.readable()))
        if not self.threshold():
            if self.registered:
                if self.ui.debugflag:
                    self.ui.note('%s below threshold - unhooking\n' %
                                 (self.event_time()))
                self.master.poll.unregister(fd)
                self.registered = False
                self.timeout = 250
        else:
            self.read_events()

    def read_events(self, bufsize=None):
        events = self.watcher.read(bufsize)
        if self.ui.debugflag:
            self.ui.note('%s reading %d events\n' %
                         (self.event_time(), len(events)))
        for evt in events:
            wpath = self.wpath(evt)
            if evt.mask & inotify.IN_UNMOUNT:
                self.process_unmount(wpath, evt)
            elif evt.mask & (inotify.IN_MODIFY | inotify.IN_ATTRIB):
                self.process_modify(wpath, evt)
            elif evt.mask & (inotify.IN_DELETE | inotify.IN_DELETE_SELF |
                             inotify.IN_MOVED_FROM):
                self.process_delete(wpath, evt)
            elif evt.mask & (inotify.IN_CREATE | inotify.IN_MOVED_TO):
                self.process_create(wpath, evt)

    def handle_timeout(self):
        if not self.registered:
            if self.ui.debugflag:
                self.ui.note('%s hooking back up with %d bytes readable\n' %
                             (self.event_time(), self.threshold.readable()))
            self.read_events(0)
            self.master.poll.register(self, select.POLLIN)
            self.registered = True

        if self.eventq:
            if self.ui.debugflag:
                self.ui.note('%s processing %d deferred events as %d\n' %
                             (self.event_time(), self.deferred,
                              len(self.eventq)))
            eventq = self.eventq.items()
            eventq.sort()
            for wpath, evts in eventq:
                for evt in evts:
                    self.deferred_event(wpath, evt)
            self.eventq.clear()
            self.deferred = 0
        self.timeout = None

    def shutdown(self):
        self.watcher.close()

class Server(object):
    poll_events = select.POLLIN

    def __init__(self, ui, repo, watcher, timeout):
        self.ui = ui
        self.repo = repo
        self.watcher = watcher
        self.timeout = timeout
        self.sock = socket.socket(socket.AF_UNIX)
        self.sockpath = self.repo.join('inotify.sock')
        try:
            self.sock.bind(self.sockpath)
        except socket.error, err:
            if err[0] == errno.EADDRINUSE:
                raise AlreadyStartedException(_('could not start server: %s') \
                                              % err[1])
            raise
        self.sock.listen(5)
        self.fileno = self.sock.fileno

    def handle_timeout(self):
        pass

    def handle_event(self, fd, event):
        sock, addr = self.sock.accept()

        cs = common.recvcs(sock)
        version = ord(cs.read(1))

        sock.sendall(chr(common.version))

        if version != common.version:
            self.ui.warn(_('received query from incompatible client '
                           'version %d\n') % version)
            return

        names = cs.read().split('\0')

        states = names.pop()

        self.ui.note(_('answering query for %r\n') % states)

        if self.watcher.timeout:
            # We got a query while a rescan is pending.  Make sure we
            # rescan before responding, or we could give back a wrong
            # answer.
            self.watcher.handle_timeout()

        if not names:
            def genresult(states, tree):
                for fn, state in self.watcher.walk(states, tree):
                    yield fn
        else:
            def genresult(states, tree):
                for fn in names:
                    l = self.watcher.lookup(fn, tree)
                    try:
                        if l in states:
                            yield fn
                    except TypeError:
                        for f, s in self.watcher.walk(states, l, fn):
                            yield f

        results = ['\0'.join(r) for r in [
            genresult('l', self.watcher.statustrees['l']),
            genresult('m', self.watcher.statustrees['m']),
            genresult('a', self.watcher.statustrees['a']),
            genresult('r', self.watcher.statustrees['r']),
            genresult('!', self.watcher.statustrees['!']),
            '?' in states and genresult('?', self.watcher.statustrees['?']) or [],
            [],
            'c' in states and genresult('n', self.watcher.tree) or [],
            ]]

        try:
            try:
                sock.sendall(struct.pack(common.resphdrfmt,
                                         *map(len, results)))
                sock.sendall(''.join(results))
            finally:
                sock.shutdown(socket.SHUT_WR)
        except socket.error, err:
            if err[0] != errno.EPIPE:
                raise

    def shutdown(self):
        self.sock.close()
        try:
            os.unlink(self.sockpath)
        except OSError, err:
            if err.errno != errno.ENOENT:
                raise

class Master(object):
    def __init__(self, ui, repo, timeout=None):
        self.ui = ui
        self.repo = repo
        self.poll = select.poll()
        self.watcher = Watcher(ui, repo, self)
        self.server = Server(ui, repo, self.watcher, timeout)
        self.table = {}
        for obj in (self.watcher, self.server):
            fd = obj.fileno()
            self.table[fd] = obj
            self.poll.register(fd, obj.poll_events)

    def register(self, fd, mask):
        self.poll.register(fd, mask)

    def shutdown(self):
        for obj in self.table.itervalues():
            obj.shutdown()

    def run(self):
        self.watcher.setup()
        self.ui.note(_('finished setup\n'))
        if os.getenv('TIME_STARTUP'):
            sys.exit(0)
        while True:
            timeout = None
            timeobj = None
            for obj in self.table.itervalues():
                if obj.timeout is not None and (timeout is None or obj.timeout < timeout):
                    timeout, timeobj = obj.timeout, obj
            try:
                if self.ui.debugflag:
                    if timeout is None:
                        self.ui.note('polling: no timeout\n')
                    else:
                        self.ui.note('polling: %sms timeout\n' % timeout)
                events = self.poll.poll(timeout)
            except select.error, err:
                if err[0] == errno.EINTR:
                    continue
                raise
            if events:
                for fd, event in events:
                    self.table[fd].handle_event(fd, event)
            elif timeobj:
                timeobj.handle_timeout()

def start(ui, repo):
    m = Master(ui, repo)
    sys.stdout.flush()
    sys.stderr.flush()

    pid = os.fork()
    if pid:
        return pid

    os.setsid()

    fd = os.open('/dev/null', os.O_RDONLY)
    os.dup2(fd, 0)
    if fd > 0:
        os.close(fd)

    fd = os.open(ui.config('inotify', 'log', '/dev/null'),
                 os.O_RDWR | os.O_CREAT | os.O_TRUNC)
    os.dup2(fd, 1)
    os.dup2(fd, 2)
    if fd > 2:
        os.close(fd)

    try:
        m.run()
    finally:
        m.shutdown()
        os._exit(0)
