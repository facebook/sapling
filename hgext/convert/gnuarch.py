# GNU Arch support for the convert extension

from common import NoRepo, checktool, commandline, commit, converter_source
from mercurial.i18n import _
from mercurial import util
import os, shutil, tempfile, stat

class gnuarch_source(converter_source, commandline):

    class gnuarch_rev:
        def __init__(self, rev):
            self.rev = rev
            self.summary = ''
            self.date = None
            self.author = ''
            self.add_files = []
            self.mod_files = []
            self.del_files = []
            self.ren_files = {}
            self.ren_dirs = {}

    def __init__(self, ui, path, rev=None):
        super(gnuarch_source, self).__init__(ui, path, rev=rev)

        if not os.path.exists(os.path.join(path, '{arch}')):
            raise NoRepo(_("couldn't open GNU Arch repo %s" % path))

        # Could use checktool, but we want to check for baz or tla.
        self.execmd = None
        if util.find_exe('tla'):
            self.execmd = 'tla'
        else:
            if util.find_exe('baz'):
                self.execmd = 'baz'
            else:
                raise util.Abort(_('cannot find a GNU Arch tool'))

        commandline.__init__(self, ui, self.execmd)

        self.path = os.path.realpath(path)
        self.tmppath = None

        self.treeversion = None
        self.lastrev = None
        self.changes = {}
        self.parents = {}
        self.tags = {}
        self.modecache = {}

    def before(self):
        if self.execmd == 'tla':
            output = self.run0('tree-version', self.path)
        else:
            output = self.run0('tree-version', '-d', self.path)
        self.treeversion = output.strip()

        self.ui.status(_('analyzing tree version %s...\n' % self.treeversion))

        # Get name of temporary directory
        version = self.treeversion.split('/')
        self.tmppath = os.path.join(tempfile.gettempdir(),
                                    'hg-%s' % version[1])

        # Generate parents dictionary
        child = []
        output, status = self.runlines('revisions', self.treeversion)
        self.checkexit(status, 'archive registered?')
        for l in output:
            rev = l.strip()
            self.changes[rev] = self.gnuarch_rev(rev)

            # Read author, date and summary
            catlog = self.runlines0('cat-log', '-d', self.path, rev)
            self._parsecatlog(catlog, rev)

            self.parents[rev] = child
            child = [rev]
            if rev == self.rev:
                break
        self.parents[None] = child

    def after(self):
        self.ui.debug(_('cleaning up %s\n' % self.tmppath))
        shutil.rmtree(self.tmppath, ignore_errors=True)

    def getheads(self):
        return self.parents[None]

    def getfile(self, name, rev):
        if rev != self.lastrev:
            raise util.Abort(_('internal calling inconsistency'))

        # Raise IOError if necessary (i.e. deleted files).
        if not os.path.exists(os.path.join(self.tmppath, name)):
            raise IOError

        data, mode = self._getfile(name, rev)
        self.modecache[(name, rev)] = mode

        return data

    def getmode(self, name, rev):
        return self.modecache[(name, rev)]

    def getchanges(self, rev):
        self.modecache = {}
        self._update(rev)
        changes = []
        copies = {}

        for f in self.changes[rev].add_files:
            changes.append((f, rev))

        for f in self.changes[rev].mod_files:
            changes.append((f, rev))

        for f in self.changes[rev].del_files:
            changes.append((f, rev))

        for src in self.changes[rev].ren_files:
            to = self.changes[rev].ren_files[src]
            changes.append((src, rev))
            changes.append((to, rev))
            copies[src] = to

        for src in self.changes[rev].ren_dirs:
            to = self.changes[rev].ren_dirs[src]
            chgs, cps = self._rendirchanges(src, to);
            changes += [(f, rev) for f in chgs]
            for c in cps:
                copies[c] = cps[c]

        changes.sort()
        self.lastrev = rev

        return changes, copies

    def getcommit(self, rev):
        changes = self.changes[rev]
        return commit(author = changes.author, date = changes.date,
                      desc = changes.summary, parents = self.parents[rev])

    def gettags(self):
        return self.tags

    def _execute(self, cmd, *args, **kwargs):
        cmdline = [self.execmd, cmd]
        cmdline += args
        cmdline = [util.shellquote(arg) for arg in cmdline]
        cmdline += ['>', util.nulldev, '2>', util.nulldev]
        cmdline = util.quotecommand(' '.join(cmdline))
        self.ui.debug(cmdline, '\n')
        return os.system(cmdline)

    def _update(self, rev):
        if rev == 'base-0':
            # Initialise 'base-0' revision
            self.ui.debug(_('obtaining revision %s...\n' % rev))
            revision = '%s--%s' % (self.treeversion, rev)
            output = self._execute('get', revision, self.tmppath)
            self.ui.debug(_('analysing revision %s...\n' % rev))
            files = self._readcontents(self.tmppath)
            self.changes[rev].add_files += files
        else:
            self.ui.debug(_('applying revision %s...\n' % rev))
            revision = '%s--%s' % (self.treeversion, rev)
            output = self._execute('replay', '-d', self.tmppath, revision)

            old_rev = self.parents[rev][0]
            self.ui.debug(_('computing changeset between %s and %s...\n' \
                               % (old_rev, rev)))
            rev_a = '%s--%s' % (self.treeversion, old_rev)
            rev_b = '%s--%s' % (self.treeversion, rev)
            delta = self.runlines0('delta', '-n', rev_a, rev_b)
            self._parsedelta(delta, rev)

    def _getfile(self, name, rev):
        mode = os.lstat(os.path.join(self.tmppath, name)).st_mode
        if stat.S_ISLNK(mode):
            data = os.readlink(os.path.join(self.tmppath, name))
            mode = mode and 'l' or ''
        else:
            data = open(os.path.join(self.tmppath, name), 'rb').read()
            mode = (mode & 0111) and 'x' or ''
        return data, mode

    def _exclude(self, name):
        exclude = [ '{arch}', '.arch-ids', '.arch-inventory' ]
        for exc in exclude:
            if name.find(exc) != -1:
                return True
        return False

    def _readcontents(self, path):
        files = []
        contents = os.listdir(path)
        while len(contents) > 0:
            c = contents.pop()
            p = os.path.join(path, c)
            if not self._exclude(p):
                if os.path.isdir(p):
                    contents += [os.path.join(c, f) for f in os.listdir(p)]
                else:
                    files.append(c)
        return files

    def _rendirchanges(self, src, dest):
        changes = []
        copies = {}
        files = self._readcontents(os.path.join(self.tmppath, dest))
        for f in files:
            s = os.path.join(src, f)
            d = os.path.join(dest, f)
            changes.append(s)
            changes.append(d)
            copies[s] = d
        return changes, copies

    def _parsecatlog(self, data, rev):
        for l in data:
            l = l.strip()
            if l.startswith('Summary:'):
                self.changes[rev].summary = l[len('Summary: '):]

            if l.startswith('Standard-date:'):
                date = l[len('Standard-date: '):]
                strdate = util.strdate(date, '%Y-%m-%d %H:%M:%S')
                self.changes[rev].date = util.datestr(strdate)

            if l.startswith('Creator:'):
                self.changes[rev].author = l[len('Creator: '):]

    def _parsedelta(self, data, rev):
        for l in data:
            l = l.strip()
            if l.startswith('A') and not l.startswith('A/'):
                file = l[1:].strip()
                if not self._exclude(file):
                    self.changes[rev].add_files.append(file)
            elif l.startswith('/>'):
                dirs = l[2:].strip().split(' ')
                if len(dirs) == 1:
                    dirs = l[2:].strip().split('\t')
                if not self._exclude(dirs[0]) and not self._exclude(dirs[1]):
                    self.changes[rev].ren_dirs[dirs[0]] = dirs[1]
            elif l.startswith('M'):
                file = l[1:].strip()
                if not self._exclude(file):
                    self.changes[rev].mod_files.append(file)
            elif l.startswith('->'):
                file = l[2:].strip()
                if not self._exclude(file):
                    self.changes[rev].mod_files.append(file)
            elif l.startswith('D') and not l.startswith('D/'):
                file = l[1:].strip()
                if not self._exclude(file):
                    self.changes[rev].del_files.append(file)
            elif l.startswith('=>'):
                files = l[2:].strip().split(' ')
                if len(files) == 1:
                    files = l[2:].strip().split('\t')
                if not self._exclude(files[0]) and not self._exclude(files[1]):
                    self.changes[rev].ren_files[files[0]] = files[1]
