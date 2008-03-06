# GNU Arch support for the convert extension

from common import NoRepo, commandline, commit, converter_source
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
            raise NoRepo(_("%s does not look like a GNU Arch repo" % path))

        # Could use checktool, but we want to check for baz or tla.
        self.execmd = None
        if util.find_exe('baz'):
            self.execmd = 'baz'
        else:
            if util.find_exe('tla'):
                self.execmd = 'tla'
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
            self._obtainrevision(rev)
        else:
            self.ui.debug(_('applying revision %s...\n' % rev))
            revision = '%s--%s' % (self.treeversion, rev)
            changeset, status = self.runlines('replay', '-d', self.tmppath,
                                              revision)
            if status:
                # Something went wrong while merging (baz or tla
                # issue?), get latest revision and try from there
                shutil.rmtree(self.tmppath, ignore_errors=True)
                self._obtainrevision(rev)
            else:
                old_rev = self.parents[rev][0]
                self.ui.debug(_('computing changeset between %s and %s...\n' \
                                    % (old_rev, rev)))
                rev_a = '%s--%s' % (self.treeversion, old_rev)
                rev_b = '%s--%s' % (self.treeversion, rev)
                self._parsechangeset(changeset, rev)

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
            # os.walk could be used, but here we avoid internal GNU
            # Arch files and directories, thus saving a lot time.
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

    def _obtainrevision(self, rev):
        self.ui.debug(_('obtaining revision %s...\n' % rev))
        revision = '%s--%s' % (self.treeversion, rev)
        output = self._execute('get', revision, self.tmppath)
        self.checkexit(output)
        self.ui.debug(_('analysing revision %s...\n' % rev))
        files = self._readcontents(self.tmppath)
        self.changes[rev].add_files += files

    def _stripbasepath(self, path):
        if path.startswith('./'):
            return path[2:]
        return path

    def _parsecatlog(self, data, rev):
        summary = []
        for l in data:
            l = l.strip()
            if summary:
                summary.append(l)
            elif l.startswith('Summary:'):
                summary.append(l[len('Summary: '):])
            elif l.startswith('Standard-date:'):
                date = l[len('Standard-date: '):]
                strdate = util.strdate(date, '%Y-%m-%d %H:%M:%S')
                self.changes[rev].date = util.datestr(strdate)
            elif l.startswith('Creator:'):
                self.changes[rev].author = l[len('Creator: '):]
        self.changes[rev].summary = '\n'.join(summary)

    def _parsechangeset(self, data, rev):
        for l in data:
            l = l.strip()
            # Added file (ignore added directory)
            if l.startswith('A') and not l.startswith('A/'):
                file = self._stripbasepath(l[1:].strip())
                if not self._exclude(file):
                    self.changes[rev].add_files.append(file)
            # Deleted file (ignore deleted directory)
            elif l.startswith('D') and not l.startswith('D/'):
                file = self._stripbasepath(l[1:].strip())
                if not self._exclude(file):
                    self.changes[rev].del_files.append(file)
            # Modified binary file
            elif l.startswith('Mb'):
                file = self._stripbasepath(l[2:].strip())
                if not self._exclude(file):
                    self.changes[rev].mod_files.append(file)
            # Modified link
            elif l.startswith('M->'):
                file = self._stripbasepath(l[3:].strip())
                if not self._exclude(file):
                    self.changes[rev].mod_files.append(file)
            # Modified file
            elif l.startswith('M'):
                file = self._stripbasepath(l[1:].strip())
                if not self._exclude(file):
                    self.changes[rev].mod_files.append(file)
            # Renamed file (or link)
            elif l.startswith('=>'):
                files = l[2:].strip().split(' ')
                if len(files) == 1:
                    files = l[2:].strip().split('\t')
                src = self._stripbasepath(files[0])
                dst = self._stripbasepath(files[1])
                if not self._exclude(src) and not self._exclude(dst):
                    self.changes[rev].ren_files[src] = dst
            # Conversion from file to link or from link to file (modified)
            elif l.startswith('ch'):
                file = self._stripbasepath(l[2:].strip())
                if not self._exclude(file):
                    self.changes[rev].mod_files.append(file)
            # Renamed directory
            elif l.startswith('/>'):
                dirs = l[2:].strip().split(' ')
                if len(dirs) == 1:
                    dirs = l[2:].strip().split('\t')
                src = self._stripbasepath(dirs[0])
                dst = self._stripbasepath(dirs[1])
                if not self._exclude(src) and not self._exclude(dst):
                    self.changes[rev].ren_dirs[src] = dst
