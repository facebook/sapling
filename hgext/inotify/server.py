# server.py - inotify status server
#
# Copyright 2006, 2007, 2008 Bryan O'Sullivan <bos@serpentine.com>
# Copyright 2007, 2008 Brendan Cully <brendan@kublai.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from mercurial.i18n import _
from mercurial import osutil, util
import common
import errno, os, select, socket, stat, struct, sys, tempfile, time

try:
    import linux as inotify
    from linux import watcher
except ImportError:
    raise

class AlreadyStartedException(Exception): pass

def join(a, b):
    if a:
        if a[-1] == '/':
            return a + b
        return a + '/' + b
    return b

def split(path):
    c = path.rfind('/')
    if c == -1:
        return '', path
    return path[:c], path[c+1:]

walk_ignored_errors = (errno.ENOENT, errno.ENAMETOOLONG)

def walkrepodirs(dirstate, absroot):
    '''Iterate over all subdirectories of this repo.
    Exclude the .hg directory, any nested repos, and ignored dirs.'''
    def walkit(dirname, top):
        fullpath = join(absroot, dirname)
        try:
            for name, kind in osutil.listdir(fullpath):
                if kind == stat.S_IFDIR:
                    if name == '.hg':
                        if not top:
                            return
                    else:
                        d = join(dirname, name)
                        if dirstate._ignore(d):
                            continue
                        for subdir in walkit(d, False):
                            yield subdir
        except OSError, err:
            if err.errno not in walk_ignored_errors:
                raise
        yield fullpath

    return walkit('', True)

def walk(dirstate, absroot, root):
    '''Like os.walk, but only yields regular files.'''

    # This function is critical to performance during startup.

    def walkit(root, reporoot):
        files, dirs = [], []

        try:
            fullpath = join(absroot, root)
            for name, kind in osutil.listdir(fullpath):
                if kind == stat.S_IFDIR:
                    if name == '.hg':
                        if not reporoot:
                            return
                    else:
                        dirs.append(name)
                        path = join(root, name)
                        if dirstate._ignore(path):
                            continue
                        for result in walkit(path, False):
                            yield result
                elif kind in (stat.S_IFREG, stat.S_IFLNK):
                    files.append(name)
            yield fullpath, dirs, files

        except OSError, err:
            if err.errno == errno.ENOTDIR:
                # fullpath was a directory, but has since been replaced
                # by a file.
                yield fullpath, dirs, files
            elif err.errno not in walk_ignored_errors:
                raise

    return walkit(root, root == '')

def _explain_watch_limit(ui, dirstate, rootabs):
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
    ndirs = len(list(walkrepodirs(dirstate, rootabs)))
    ui.warn(_('found %d\n') % ndirs)
    newlimit = min(limit, 1024)
    while newlimit < ((limit + ndirs) * 1.1):
        newlimit *= 2
    ui.warn(_('*** to raise the limit from %d to %d (run as root):\n') %
            (limit, newlimit))
    ui.warn(_('***  echo %d > %s\n') % (newlimit, path))
    raise util.Abort(_('cannot watch %s until inotify watch limit is raised')
                     % rootabs)

class pollable(object):
    """
    Interface to support polling.
    The file descriptor returned by fileno() is registered to a polling
    object.
    Usage:
        Every tick, check if an event has happened since the last tick:
        * If yes, call handle_events
        * If no, call handle_timeout
    """
    poll_events = select.POLLIN
    instances = {}
    poll = select.poll()

    def fileno(self):
        raise NotImplementedError

    def handle_events(self, events):
        raise NotImplementedError

    def handle_timeout(self):
        raise NotImplementedError

    def shutdown(self):
        raise NotImplementedError

    def register(self, timeout):
        fd = self.fileno()

        pollable.poll.register(fd, pollable.poll_events)
        pollable.instances[fd] = self

        self.registered = True
        self.timeout = timeout

    def unregister(self):
        pollable.poll.unregister(self)
        self.registered = False

    @classmethod
    def run(cls):
        while True:
            timeout = None
            timeobj = None
            for obj in cls.instances.itervalues():
                if obj.timeout is not None and (timeout is None or obj.timeout < timeout):
                    timeout, timeobj = obj.timeout, obj
            try:
                events = cls.poll.poll(timeout)
            except select.error, err:
                if err[0] == errno.EINTR:
                    continue
                raise
            if events:
                by_fd = {}
                for fd, event in events:
                    by_fd.setdefault(fd, []).append(event)

                for fd, events in by_fd.iteritems():
                    cls.instances[fd].handle_pollevents(events)

            elif timeobj:
                timeobj.handle_timeout()

def eventaction(code):
    """
    Decorator to help handle events in repowatcher
    """
    def decorator(f):
        def wrapper(self, wpath):
            if code == 'm' and wpath in self.lastevent and \
                self.lastevent[wpath] in 'cm':
                return
            self.lastevent[wpath] = code
            self.timeout = 250

            f(self, wpath)

        wrapper.func_name = f.func_name
        return wrapper
    return decorator

class directory(object):
    """
    Representing a directory

    * path is the relative path from repo root to this directory
    * files is a dict listing the files in this directory
        - keys are file names
        - values are file status
    * dirs is a dict listing the subdirectories
        - key are subdirectories names
        - values are directory objects
    """
    def __init__(self, relpath=''):
        self.path = relpath
        self.files = {}
        self.dirs = {}

    def dir(self, relpath):
        """
        Returns the directory contained at the relative path relpath.
        Creates the intermediate directories if necessary.
        """
        if not relpath:
            return self
        l = relpath.split('/')
        ret = self
        while l:
            next = l.pop(0)
            try:
                ret = ret.dirs[next]
            except KeyError:
                d = directory(join(ret.path, next))
                ret.dirs[next] = d
                ret = d
        return ret

    def walk(self, states):
        """
        yield (filename, status) pairs for items in the trees
        that have status in states.
        filenames are relative to the repo root
        """
        for file, st in self.files.iteritems():
            if st in states:
                yield join(self.path, file), st
        for dir in self.dirs.itervalues():
            for e in dir.walk(states):
                yield e

    def lookup(self, states, path):
        """
        yield root-relative filenames that match path, and whose
        status are in states:
        * if path is a file, yield path
        * if path is a directory, yield directory files
        * if path is not tracked, yield nothing
        """
        if path[-1] == '/':
            path = path[:-1]

        paths = path.split('/')

        # we need to check separately for last node
        last = paths.pop()

        tree = self
        try:
            for dir in paths:
                tree = tree.dirs[dir]
        except KeyError:
            # path is not tracked
            return

        try:
            # if path is a directory, walk it
            for file, st in tree.dirs[last].walk(states):
                yield file
        except KeyError:
            try:
                if tree.files[last] in states:
                    # path is a file
                    yield path
            except KeyError:
                # path is not tracked
                pass

class repowatcher(pollable):
    """
    Watches inotify events
    """
    statuskeys = 'almr!?'
    mask = (
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

    def __init__(self, ui, dirstate, root):
        self.ui = ui
        self.dirstate = dirstate

        self.wprefix = join(root, '')
        self.prefixlen = len(self.wprefix)
        try:
            self.watcher = watcher.watcher()
        except OSError, err:
            raise util.Abort(_('inotify service not available: %s') %
                             err.strerror)
        self.threshold = watcher.threshold(self.watcher)
        self.fileno = self.watcher.fileno

        self.tree = directory()
        self.statcache = {}
        self.statustrees = dict([(s, directory()) for s in self.statuskeys])

        self.last_event = None

        self.lastevent = {}

        self.register(timeout=None)

        self.ds_info = self.dirstate_info()
        self.handle_timeout()
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
            st = os.lstat(self.wprefix + '.hg/dirstate')
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
                self.ui.note(_('watching %r\n') % path[self.prefixlen:])
            try:
                self.watcher.add(path, mask)
            except OSError, err:
                if err.errno in (errno.ENOENT, errno.ENOTDIR):
                    return
                if err.errno != errno.ENOSPC:
                    raise
                _explain_watch_limit(self.ui, self.dirstate, self.wprefix)

    def setup(self):
        self.ui.note(_('watching directories under %r\n') % self.wprefix)
        self.add_watch(self.wprefix + '.hg', inotify.IN_DELETE)
        self.check_dirstate()

    def filestatus(self, fn, st):
        try:
            type_, mode, size, time = self.dirstate._map[fn][:4]
        except KeyError:
            type_ = '?'
        if type_ == 'n':
            st_mode, st_size, st_mtime = st
            if size == -1:
                return 'l'
            if size and (size != st_size or (mode ^ st_mode) & 0100):
                return 'm'
            if time != int(st_mtime):
                return 'l'
            return 'n'
        if type_ == '?' and self.dirstate._ignore(fn):
            return 'i'
        return type_

    def updatefile(self, wfn, osstat):
        '''
        update the file entry of an existing file.

        osstat: (mode, size, time) tuple, as returned by os.lstat(wfn)
        '''

        self._updatestatus(wfn, self.filestatus(wfn, osstat))

    def deletefile(self, wfn, oldstatus):
        '''
        update the entry of a file which has been deleted.

        oldstatus: char in statuskeys, status of the file before deletion
        '''
        if oldstatus == 'r':
            newstatus = 'r'
        elif oldstatus in 'almn':
            newstatus = '!'
        else:
            newstatus = None

        self.statcache.pop(wfn, None)
        self._updatestatus(wfn, newstatus)

    def _updatestatus(self, wfn, newstatus):
        '''
        Update the stored status of a file.

        newstatus: - char in (statuskeys + 'ni'), new status to apply.
                   - or None, to stop tracking wfn
        '''
        root, fn = split(wfn)
        d = self.tree.dir(root)

        oldstatus = d.files.get(fn)
        # oldstatus can be either:
        # - None : fn is new
        # - a char in statuskeys: fn is a (tracked) file

        if self.ui.debugflag and oldstatus != newstatus:
            self.ui.note(_('status: %r %s -> %s\n') %
                             (wfn, oldstatus, newstatus))

        if oldstatus and oldstatus in self.statuskeys \
            and oldstatus != newstatus:
            del self.statustrees[oldstatus].dir(root).files[fn]

        if newstatus in (None, 'i'):
            d.files.pop(fn, None)
        elif oldstatus != newstatus:
            d.files[fn] = newstatus
            if newstatus != 'n':
                self.statustrees[newstatus].dir(root).files[fn] = newstatus


    def check_deleted(self, key):
        # Files that had been deleted but were present in the dirstate
        # may have vanished from the dirstate; we must clean them up.
        nuke = []
        for wfn, ignore in self.statustrees[key].walk(key):
            if wfn not in self.dirstate:
                nuke.append(wfn)
        for wfn in nuke:
            root, fn = split(wfn)
            del self.statustrees[key].dir(root).files[fn]
            del self.tree.dir(root).files[fn]

    def scan(self, topdir=''):
        ds = self.dirstate._map.copy()
        self.add_watch(join(self.wprefix, topdir), self.mask)
        for root, dirs, files in walk(self.dirstate, self.wprefix, topdir):
            for d in dirs:
                self.add_watch(join(root, d), self.mask)
            wroot = root[self.prefixlen:]
            for fn in files:
                wfn = join(wroot, fn)
                self.updatefile(wfn, self.getstat(wfn))
                ds.pop(wfn, None)
        wtopdir = topdir
        if wtopdir and wtopdir[-1] != '/':
            wtopdir += '/'
        for wfn, state in ds.iteritems():
            if not wfn.startswith(wtopdir):
                continue
            try:
                st = self.stat(wfn)
            except OSError:
                status = state[0]
                self.deletefile(wfn, status)
            else:
                self.updatefile(wfn, st)
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
        self.dirstate.invalidate()
        self.handle_timeout()
        self.scan()
        self.ui.note(_('%s end dirstate reload\n') % self.event_time())

    def update_hgignore(self):
        # An update of the ignore file can potentially change the
        # states of all unknown and ignored files.

        # XXX If the user has other ignore files outside the repo, or
        # changes their list of ignore files at run time, we'll
        # potentially never see changes to them.  We could get the
        # client to report to us what ignore data they're using.
        # But it's easier to do nothing than to open that can of
        # worms.

        if '_ignore' in self.dirstate.__dict__:
            delattr(self.dirstate, '_ignore')
            self.ui.note(_('rescanning due to .hgignore change\n'))
            self.handle_timeout()
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
        except OSError:
            self.statcache.pop(wpath, None)
            raise

    @eventaction('c')
    def created(self, wpath):
        if wpath == '.hgignore':
            self.update_hgignore()
        try:
            st = self.stat(wpath)
            if stat.S_ISREG(st[0]):
                self.updatefile(wpath, st)
        except OSError:
            pass

    @eventaction('m')
    def modified(self, wpath):
        if wpath == '.hgignore':
            self.update_hgignore()
        try:
            st = self.stat(wpath)
            if stat.S_ISREG(st[0]):
                if self.dirstate[wpath] in 'lmn':
                    self.updatefile(wpath, st)
        except OSError:
            pass

    @eventaction('d')
    def deleted(self, wpath):
        if wpath == '.hgignore':
            self.update_hgignore()
        elif wpath.startswith('.hg/'):
            if wpath == '.hg/wlock':
                self.check_dirstate()
            return

        self.deletefile(wpath, self.dirstate[wpath])

    def process_create(self, wpath, evt):
        if self.ui.debugflag:
            self.ui.note(_('%s event: created %s\n') %
                         (self.event_time(), wpath))

        if evt.mask & inotify.IN_ISDIR:
            self.scan(wpath)
        else:
            self.created(wpath)

    def process_delete(self, wpath, evt):
        if self.ui.debugflag:
            self.ui.note(_('%s event: deleted %s\n') %
                         (self.event_time(), wpath))

        if evt.mask & inotify.IN_ISDIR:
            tree = self.tree.dir(wpath)
            todelete = [wfn for wfn, ignore in tree.walk('?')]
            for fn in todelete:
                self.deletefile(fn, '?')
            self.scan(wpath)
        else:
            self.deleted(wpath)

    def process_modify(self, wpath, evt):
        if self.ui.debugflag:
            self.ui.note(_('%s event: modified %s\n') %
                         (self.event_time(), wpath))

        if not (evt.mask & inotify.IN_ISDIR):
            self.modified(wpath)

    def process_unmount(self, evt):
        self.ui.warn(_('filesystem containing %s was unmounted\n') %
                     evt.fullpath)
        sys.exit(0)

    def handle_pollevents(self, events):
        if self.ui.debugflag:
            self.ui.note(_('%s readable: %d bytes\n') %
                         (self.event_time(), self.threshold.readable()))
        if not self.threshold():
            if self.registered:
                if self.ui.debugflag:
                    self.ui.note(_('%s below threshold - unhooking\n') %
                                 (self.event_time()))
                self.unregister()
                self.timeout = 250
        else:
            self.read_events()

    def read_events(self, bufsize=None):
        events = self.watcher.read(bufsize)
        if self.ui.debugflag:
            self.ui.note(_('%s reading %d events\n') %
                         (self.event_time(), len(events)))
        for evt in events:
            assert evt.fullpath.startswith(self.wprefix)
            wpath = evt.fullpath[self.prefixlen:]

            # paths have been normalized, wpath never ends with a '/'

            if wpath.startswith('.hg/') and evt.mask & inotify.IN_ISDIR:
                # ignore subdirectories of .hg/ (merge, patches...)
                continue

            if evt.mask & inotify.IN_UNMOUNT:
                self.process_unmount(wpath, evt)
            elif evt.mask & (inotify.IN_MODIFY | inotify.IN_ATTRIB):
                self.process_modify(wpath, evt)
            elif evt.mask & (inotify.IN_DELETE | inotify.IN_DELETE_SELF |
                             inotify.IN_MOVED_FROM):
                self.process_delete(wpath, evt)
            elif evt.mask & (inotify.IN_CREATE | inotify.IN_MOVED_TO):
                self.process_create(wpath, evt)

        self.lastevent.clear()

    def handle_timeout(self):
        if not self.registered:
            if self.ui.debugflag:
                self.ui.note(_('%s hooking back up with %d bytes readable\n') %
                             (self.event_time(), self.threshold.readable()))
            self.read_events(0)
            self.register(timeout=None)

        self.timeout = None

    def shutdown(self):
        self.watcher.close()

    def debug(self):
        """
        Returns a sorted list of relatives paths currently watched,
        for debugging purposes.
        """
        return sorted(tuple[0][self.prefixlen:] for tuple in self.watcher)

class server(pollable):
    """
    Listens for client queries on unix socket inotify.sock
    """
    def __init__(self, ui, root, repowatcher, timeout):
        self.ui = ui
        self.repowatcher = repowatcher
        self.sock = socket.socket(socket.AF_UNIX)
        self.sockpath = join(root, '.hg/inotify.sock')
        self.realsockpath = None
        try:
            self.sock.bind(self.sockpath)
        except socket.error, err:
            if err[0] == errno.EADDRINUSE:
                raise AlreadyStartedException(_('could not start server: %s')
                                              % err[1])
            if err[0] == "AF_UNIX path too long":
                tempdir = tempfile.mkdtemp(prefix="hg-inotify-")
                self.realsockpath = os.path.join(tempdir, "inotify.sock")
                try:
                    self.sock.bind(self.realsockpath)
                    os.symlink(self.realsockpath, self.sockpath)
                except (OSError, socket.error), inst:
                    try:
                        os.unlink(self.realsockpath)
                    except:
                        pass
                    os.rmdir(tempdir)
                    if inst.errno == errno.EEXIST:
                        raise AlreadyStartedException(_('could not start server: %s')
                                                      % inst.strerror)
                    raise
            else:
                raise
        self.sock.listen(5)
        self.fileno = self.sock.fileno
        self.register(timeout=timeout)

    def handle_timeout(self):
        pass

    def answer_stat_query(self, cs):
        names = cs.read().split('\0')

        states = names.pop()

        self.ui.note(_('answering query for %r\n') % states)

        if self.repowatcher.timeout:
            # We got a query while a rescan is pending.  Make sure we
            # rescan before responding, or we could give back a wrong
            # answer.
            self.repowatcher.handle_timeout()

        if not names:
            def genresult(states, tree):
                for fn, state in tree.walk(states):
                    yield fn
        else:
            def genresult(states, tree):
                for fn in names:
                    for f in tree.lookup(states, fn):
                        yield f

        return ['\0'.join(r) for r in [
            genresult('l', self.repowatcher.statustrees['l']),
            genresult('m', self.repowatcher.statustrees['m']),
            genresult('a', self.repowatcher.statustrees['a']),
            genresult('r', self.repowatcher.statustrees['r']),
            genresult('!', self.repowatcher.statustrees['!']),
            '?' in states
                and genresult('?', self.repowatcher.statustrees['?'])
                or [],
            [],
            'c' in states and genresult('n', self.repowatcher.tree) or [],
            ]]

    def answer_dbug_query(self):
        return ['\0'.join(self.repowatcher.debug())]

    def handle_pollevents(self, events):
        for e in events:
            self.handle_pollevent()

    def handle_pollevent(self):
        sock, addr = self.sock.accept()

        cs = common.recvcs(sock)
        version = ord(cs.read(1))

        if version != common.version:
            self.ui.warn(_('received query from incompatible client '
                           'version %d\n') % version)
            try:
                # try to send back our version to the client
                # this way, the client too is informed of the mismatch
                sock.sendall(chr(common.version))
            except:
                pass
            return

        type = cs.read(4)

        if type == 'STAT':
            results = self.answer_stat_query(cs)
        elif type == 'DBUG':
            results = self.answer_dbug_query()
        else:
            self.ui.warn(_('unrecognized query type: %s\n') % type)
            return

        try:
            try:
                v = chr(common.version)

                sock.sendall(v + type + struct.pack(common.resphdrfmts[type],
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
            if self.realsockpath:
                os.unlink(self.realsockpath)
                os.rmdir(os.path.dirname(self.realsockpath))
        except OSError, err:
            if err.errno != errno.ENOENT:
                raise

class master(object):
    def __init__(self, ui, dirstate, root, timeout=None):
        self.ui = ui
        self.repowatcher = repowatcher(ui, dirstate, root)
        self.server = server(ui, root, self.repowatcher, timeout)

    def shutdown(self):
        for obj in pollable.instances.itervalues():
            obj.shutdown()

    def run(self):
        self.repowatcher.setup()
        self.ui.note(_('finished setup\n'))
        if os.getenv('TIME_STARTUP'):
            sys.exit(0)
        pollable.run()

def start(ui, dirstate, root):
    def closefds(ignore):
        # (from python bug #1177468)
        # close all inherited file descriptors
        # Python 2.4.1 and later use /dev/urandom to seed the random module's RNG
        # a file descriptor is kept internally as os._urandomfd (created on demand
        # the first time os.urandom() is called), and should not be closed
        try:
            os.urandom(4)
            urandom_fd = getattr(os, '_urandomfd', None)
        except AttributeError:
            urandom_fd = None
        ignore.append(urandom_fd)
        for fd in range(3, 256):
            if fd in ignore:
                continue
            try:
                os.close(fd)
            except OSError:
                pass

    m = master(ui, dirstate, root)
    sys.stdout.flush()
    sys.stderr.flush()

    pid = os.fork()
    if pid:
        return pid

    closefds(pollable.instances.keys())
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
