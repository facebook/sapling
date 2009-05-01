import cStringIO

import os, re, shutil, stat, subprocess
from mercurial import util as hgutil
from mercurial.i18n import _

class externalsfile(dict):
    """Map svn directories to lists of externals entries.
    """
    def __init__(self):
        super(externalsfile, self).__init__()
        self.encoding = 'utf-8'

    def __setitem__(self, key, value):
        if value is None:
            value = []
        elif isinstance(value, basestring):
            value = value.splitlines()
        if key == '.':
            key = ''
        if not value:
            if key in self:
                del self[key]
        else:
            super(externalsfile, self).__setitem__(key, value)

    def write(self):
        fp = cStringIO.StringIO()
        for target in sorted(self):
            lines = self[target]
            if not lines:
                continue
            if not target:
                target = '.'
            fp.write('[%s]\n' % target)
            for l in lines:
                l = ' ' + l + '\n'
                fp.write(l)
        return fp.getvalue()

    def read(self, data):
        self.clear()
        fp = cStringIO.StringIO(data)
        dirs = {}
        target = None
        for line in fp.readlines():
            if not line.strip():
                continue
            if line.startswith('['):
                line = line.strip()
                if line[-1] != ']':
                    raise hgutil.Abort('invalid externals section name: %s' % line)
                target = line[1:-1]
                if target == '.':
                    target = ''
            elif line.startswith(' '):
                line = line.rstrip('\n')
                if target is None or not line:
                    continue
                self.setdefault(target, []).append(line[1:])

def diff(ext1, ext2):
    """Compare 2 externalsfile and yield tuples like (dir, value1, value2)
    where value1 is the external value in ext1 for dir or None, and
    value2 the same in ext2.
    """
    for d in ext1:
        if d not in ext2:
            yield d, '\n'.join(ext1[d]), None
        elif ext1[d] != ext2[d]:
            yield d, '\n'.join(ext1[d]), '\n'.join(ext2[d])
    for d in ext2:
        if d not in ext1:
            yield d, None, '\n'.join(ext2[d])

class BadDefinition(Exception):
    pass

re_defold = re.compile(r'^(.*?)\s+(?:-r\s*(\d+)\s+)?([a-zA-Z]+://.*)$')
re_defnew = re.compile(r'^(?:-r\s*(\d+)\s+)?((?:[a-zA-Z]+://|\^/).*)\s+(.*)$')
re_pegrev = re.compile(r'^(.*)@(\d+)$')
re_scheme = re.compile(r'^[a-zA-Z]+://')

def parsedefinition(line):
    """Parse an external definition line, return a tuple (path, rev, source)
    or raise BadDefinition.
    """
    # The parsing is probably not correct wrt path with whitespaces or
    # potential quotes. svn documentation is not really talkative about
    # these either.
    line = line.strip()
    m = re_defnew.search(line)
    if m:
        rev, source, path = m.group(1, 2, 3)
    else:
        m = re_defold.search(line)
        if not m:
            raise BadDefinition()
        path, rev, source = m.group(1, 2, 3)
    # Look for peg revisions
    m = re_pegrev.search(source)
    if m:
        source, rev = m.group(1, 2)
    return (path, rev, source)

def parsedefinitions(ui, repo, svnroot, exts):
    """Return (targetdir, revision, source) tuples. Fail if nested
    targetdirs are detected. source is an svn project URL.
    """
    defs = []
    for base in sorted(exts):
        for line in exts[base]:
            try:
                path, rev, source = parsedefinition(line)
            except BadDefinition:
                ui.warn(_('ignoring invalid external definition: %r' % line))
                continue
            if re_scheme.search(source):
                pass
            elif source.startswith('^/'):
                source = svnroot + source[1:]
            else:
                ui.warn(_('ignoring unsupported non-fully qualified external: %r' % source))
                continue
            wpath = hgutil.pconvert(os.path.join(base, path))
            wpath = hgutil.canonpath(repo.root, '', wpath)
            defs.append((wpath, rev, source))
    # Check target dirs are not nested
    defs.sort()
    for i, d in enumerate(defs):
        for d2 in defs[i+1:]:
            if d2[0].startswith(d[0] + '/'):
                raise hgutil.Abort(_('external directories cannot nest:\n%s\n%s')
                                   % (d[0], d2[0]))
    return defs

def computeactions(ui, repo, svnroot, ext1, ext2):

    def listdefs(data):
        defs = {}
        exts = externalsfile()
        exts.read(data)
        for d in parsedefinitions(ui, repo, svnroot, exts):
            defs[d[0]] = d
        return defs

    ext1 = listdefs(ext1)
    ext2 = listdefs(ext2)
    for wp1 in ext1:
        if wp1 in ext2:
            yield 'u', ext2[wp1]
        else:
            yield 'd', ext1[wp1]
    for wp2 in ext2:
        if wp2 not in ext1:
            yield 'u', ext2[wp2]

def getsvninfo(svnurl):
    """Return a tuple (url, root) for supplied svn URL or working
    directory path.
    """
    # Yes, this is ugly, but good enough for now
    args = ['svn', 'info', '--xml', svnurl]
    shell = os.name == 'nt'
    p = subprocess.Popen(args, stdout=subprocess.PIPE, shell=shell)
    stdout = p.communicate()[0]
    if p.returncode:
        raise hgutil.Abort(_('cannot get information about %s')
                         % svnurl)
    m = re.search(r'<root>(.*)</root>', stdout, re.S)
    if not m:
        raise hgutil.Abort(_('cannot find SVN repository root from %s')
                           % svnurl)
    root = m.group(1).rstrip('/')

    m = re.search(r'<url>(.*)</url>', stdout, re.S)
    if not m:
        raise hgutil.Abort(_('cannot find SVN repository URL from %s') % svnurl)
    url = m.group(1)

    m = re.search(r'<entry[^>]+revision="([^"]+)"', stdout, re.S)
    if not m:
        raise hgutil.Abort(_('cannot find SVN revision from %s') % svnurl)
    rev = m.group(1)
    return url, root, rev

class externalsupdater:
    def __init__(self, ui, repo):
        self.repo = repo
        self.ui = ui

    def update(self, wpath, rev, source):
        path = self.repo.wjoin(wpath)
        revspec = []
        if rev:
            revspec = ['-r', rev]
        if os.path.isdir(path):
            exturl, extroot, extrev = getsvninfo(path)
            if source == exturl:
                if extrev != rev:
                    self.ui.status(_('updating external on %s@%s\n') %
                                   (wpath, rev or 'HEAD'))
                    cwd = os.path.join(self.repo.root, path)
                    self.svn(['update'] + revspec, cwd)
                return
            self.delete(wpath)
        cwd, dest = os.path.split(path)
        cwd = os.path.join(self.repo.root, cwd)
        if not os.path.isdir(cwd):
            os.makedirs(cwd)
        self.ui.status(_('fetching external %s@%s\n') % (wpath, rev or 'HEAD'))
        self.svn(['co'] + revspec + [source, dest], cwd)

    def delete(self, wpath):
        path = self.repo.wjoin(wpath)
        if os.path.isdir(path):
            self.ui.status(_('removing external %s\n') % wpath)

            def onerror(function, path, excinfo):
                if function is not os.remove:
                    raise
                # read-only files cannot be unlinked under Windows
                s = os.stat(path)
                if (s.st_mode & stat.S_IWRITE) != 0:
                    raise
                os.chmod(path, stat.S_IMODE(s.st_mode) | stat.S_IWRITE)
                os.remove(path)

            shutil.rmtree(path, onerror=onerror)
            return 1

    def svn(self, args, cwd):
        args = ['svn'] + args
        self.ui.debug(_('updating externals: %r, cwd=%s\n') % (args, cwd))
        shell = os.name == 'nt'
        subprocess.check_call(args, cwd=cwd, shell=shell)

def updateexternals(ui, args, repo, **opts):
    """update repository externals
    """
    if len(args) > 1:
        raise hgutil.Abort(_('updateexternals expects at most one changeset'))
    node = None
    if args:
        node = args[0]

    try:
        svnurl = file(repo.join('svn/url'), 'rb').read()
    except:
        raise hgutil.Abort(_('failed to retrieve original svn URL'))
    svnroot = getsvninfo(svnurl)[1]

    # Retrieve current externals status
    try:
        oldext = file(repo.join('svn/externals'), 'rb').read()
    except IOError:
        oldext = ''
    newext = ''
    ctx = repo[node]
    if '.hgsvnexternals' in ctx:
        newext = ctx['.hgsvnexternals'].data()

    updater = externalsupdater(ui, repo)
    actions = computeactions(ui, repo, svnroot, oldext, newext)
    for action, ext in actions:
        if action == 'u':
            updater.update(ext[0], ext[1], ext[2])
        elif action == 'd':
            updater.delete(ext[0])
        else:
            raise hgutil.Abort(_('unknown update actions: %r') % action)

    file(repo.join('svn/externals'), 'wb').write(newext)

