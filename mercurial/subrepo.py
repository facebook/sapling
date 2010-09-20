# subrepo.py - sub-repository handling for Mercurial
#
# Copyright 2009-2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import errno, os, re, xml.dom.minidom, shutil, urlparse, posixpath
from i18n import _
import config, util, node, error, cmdutil
hg = None

nullstate = ('', '', 'empty')

def state(ctx, ui):
    """return a state dict, mapping subrepo paths configured in .hgsub
    to tuple: (source from .hgsub, revision from .hgsubstate, kind
    (key in types dict))
    """
    p = config.config()
    def read(f, sections=None, remap=None):
        if f in ctx:
            p.parse(f, ctx[f].data(), sections, remap, read)
        else:
            raise util.Abort(_("subrepo spec file %s not found") % f)

    if '.hgsub' in ctx:
        read('.hgsub')

    for path, src in ui.configitems('subpaths'):
        p.set('subpaths', path, src, ui.configsource('subpaths', path))

    rev = {}
    if '.hgsubstate' in ctx:
        try:
            for l in ctx['.hgsubstate'].data().splitlines():
                revision, path = l.split(" ", 1)
                rev[path] = revision
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise

    state = {}
    for path, src in p[''].items():
        kind = 'hg'
        if src.startswith('['):
            if ']' not in src:
                raise util.Abort(_('missing ] in subrepo source'))
            kind, src = src.split(']', 1)
            kind = kind[1:]

        for pattern, repl in p.items('subpaths'):
            # Turn r'C:\foo\bar' into r'C:\\foo\\bar' since re.sub
            # does a string decode.
            repl = repl.encode('string-escape')
            # However, we still want to allow back references to go
            # through unharmed, so we turn r'\\1' into r'\1'. Again,
            # extra escapes are needed because re.sub string decodes.
            repl = re.sub(r'\\\\([0-9]+)', r'\\\1', repl)
            try:
                src = re.sub(pattern, repl, src, 1)
            except re.error, e:
                raise util.Abort(_("bad subrepository pattern in %s: %s")
                                 % (p.source('subpaths', pattern), e))

        state[path] = (src.strip(), rev.get(path, ''), kind)

    return state

def writestate(repo, state):
    """rewrite .hgsubstate in (outer) repo with these subrepo states"""
    repo.wwrite('.hgsubstate',
                ''.join(['%s %s\n' % (state[s][1], s)
                         for s in sorted(state)]), '')

def submerge(repo, wctx, mctx, actx):
    """delegated from merge.applyupdates: merging of .hgsubstate file
    in working context, merging context and ancestor context"""
    if mctx == actx: # backwards?
        actx = wctx.p1()
    s1 = wctx.substate
    s2 = mctx.substate
    sa = actx.substate
    sm = {}

    repo.ui.debug("subrepo merge %s %s %s\n" % (wctx, mctx, actx))

    def debug(s, msg, r=""):
        if r:
            r = "%s:%s:%s" % r
        repo.ui.debug("  subrepo %s: %s %s\n" % (s, msg, r))

    for s, l in s1.items():
        a = sa.get(s, nullstate)
        ld = l # local state with possible dirty flag for compares
        if wctx.sub(s).dirty():
            ld = (l[0], l[1] + "+")
        if wctx == actx: # overwrite
            a = ld

        if s in s2:
            r = s2[s]
            if ld == r or r == a: # no change or local is newer
                sm[s] = l
                continue
            elif ld == a: # other side changed
                debug(s, "other changed, get", r)
                wctx.sub(s).get(r)
                sm[s] = r
            elif ld[0] != r[0]: # sources differ
                if repo.ui.promptchoice(
                    _(' subrepository sources for %s differ\n'
                      'use (l)ocal source (%s) or (r)emote source (%s)?')
                      % (s, l[0], r[0]),
                      (_('&Local'), _('&Remote')), 0):
                    debug(s, "prompt changed, get", r)
                    wctx.sub(s).get(r)
                    sm[s] = r
            elif ld[1] == a[1]: # local side is unchanged
                debug(s, "other side changed, get", r)
                wctx.sub(s).get(r)
                sm[s] = r
            else:
                debug(s, "both sides changed, merge with", r)
                wctx.sub(s).merge(r)
                sm[s] = l
        elif ld == a: # remote removed, local unchanged
            debug(s, "remote removed, remove")
            wctx.sub(s).remove()
        else:
            if repo.ui.promptchoice(
                _(' local changed subrepository %s which remote removed\n'
                  'use (c)hanged version or (d)elete?') % s,
                (_('&Changed'), _('&Delete')), 0):
                debug(s, "prompt remove")
                wctx.sub(s).remove()

    for s, r in s2.items():
        if s in s1:
            continue
        elif s not in sa:
            debug(s, "remote added, get", r)
            mctx.sub(s).get(r)
            sm[s] = r
        elif r != sa[s]:
            if repo.ui.promptchoice(
                _(' remote changed subrepository %s which local removed\n'
                  'use (c)hanged version or (d)elete?') % s,
                (_('&Changed'), _('&Delete')), 0) == 0:
                debug(s, "prompt recreate", r)
                wctx.sub(s).get(r)
                sm[s] = r

    # record merged .hgsubstate
    writestate(repo, sm)

def relpath(sub):
    """return path to this subrepo as seen from outermost repo"""
    if not hasattr(sub, '_repo'):
        return sub._path
    parent = sub._repo
    while hasattr(parent, '_subparent'):
        parent = parent._subparent
    return sub._repo.root[len(parent.root)+1:]

def _abssource(repo, push=False):
    """return pull/push path of repo - either based on parent repo
    .hgsub info or on the subrepos own config"""
    if hasattr(repo, '_subparent'):
        source = repo._subsource
        if source.startswith('/') or '://' in source:
            return source
        parent = _abssource(repo._subparent, push)
        if '://' in parent:
            if parent[-1] == '/':
                parent = parent[:-1]
            r = urlparse.urlparse(parent + '/' + source)
            r = urlparse.urlunparse((r[0], r[1],
                                     posixpath.normpath(r[2]),
                                     r[3], r[4], r[5]))
            return r
        return posixpath.normpath(os.path.join(parent, repo._subsource))
    if push and repo.ui.config('paths', 'default-push'):
        return repo.ui.config('paths', 'default-push', repo.root)
    return repo.ui.config('paths', 'default', repo.root)

def itersubrepos(ctx1, ctx2):
    """find subrepos in ctx1 or ctx2"""
    # Create a (subpath, ctx) mapping where we prefer subpaths from
    # ctx1. The subpaths from ctx2 are important when the .hgsub file
    # has been modified (in ctx2) but not yet committed (in ctx1).
    subpaths = dict.fromkeys(ctx2.substate, ctx2)
    subpaths.update(dict.fromkeys(ctx1.substate, ctx1))
    for subpath, ctx in sorted(subpaths.iteritems()):
        yield subpath, ctx.sub(subpath)

def subrepo(ctx, path):
    """return instance of the right subrepo class for subrepo in path"""
    # subrepo inherently violates our import layering rules
    # because it wants to make repo objects from deep inside the stack
    # so we manually delay the circular imports to not break
    # scripts that don't use our demand-loading
    global hg
    import hg as h
    hg = h

    util.path_auditor(ctx._repo.root)(path)
    state = ctx.substate.get(path, nullstate)
    if state[2] not in types:
        raise util.Abort(_('unknown subrepo type %s') % state[2])
    return types[state[2]](ctx, path, state[:2])

# subrepo classes need to implement the following abstract class:

class abstractsubrepo(object):

    def dirty(self):
        """returns true if the dirstate of the subrepo does not match
        current stored state
        """
        raise NotImplementedError

    def checknested(path):
        """check if path is a subrepository within this repository"""
        return False

    def commit(self, text, user, date):
        """commit the current changes to the subrepo with the given
        log message. Use given user and date if possible. Return the
        new state of the subrepo.
        """
        raise NotImplementedError

    def remove(self):
        """remove the subrepo

        (should verify the dirstate is not dirty first)
        """
        raise NotImplementedError

    def get(self, state):
        """run whatever commands are needed to put the subrepo into
        this state
        """
        raise NotImplementedError

    def merge(self, state):
        """merge currently-saved state with the new state."""
        raise NotImplementedError

    def push(self, force):
        """perform whatever action is analogous to 'hg push'

        This may be a no-op on some systems.
        """
        raise NotImplementedError

    def add(self, ui, match, dryrun, prefix):
        return []

    def status(self, rev2, **opts):
        return [], [], [], [], [], [], []

    def diff(self, diffopts, node2, match, prefix, **opts):
        pass

    def outgoing(self, ui, dest, opts):
        return 1

    def incoming(self, ui, source, opts):
        return 1

    def files(self):
        """return filename iterator"""
        raise NotImplementedError

    def filedata(self, name):
        """return file data"""
        raise NotImplementedError

    def fileflags(self, name):
        """return file flags"""
        return ''

    def archive(self, archiver, prefix):
        for name in self.files():
            flags = self.fileflags(name)
            mode = 'x' in flags and 0755 or 0644
            symlink = 'l' in flags
            archiver.addfile(os.path.join(prefix, self._path, name),
                             mode, symlink, self.filedata(name))


class hgsubrepo(abstractsubrepo):
    def __init__(self, ctx, path, state):
        self._path = path
        self._state = state
        r = ctx._repo
        root = r.wjoin(path)
        create = False
        if not os.path.exists(os.path.join(root, '.hg')):
            create = True
            util.makedirs(root)
        self._repo = hg.repository(r.ui, root, create=create)
        self._repo._subparent = r
        self._repo._subsource = state[0]

        if create:
            fp = self._repo.opener("hgrc", "w", text=True)
            fp.write('[paths]\n')

            def addpathconfig(key, value):
                fp.write('%s = %s\n' % (key, value))
                self._repo.ui.setconfig('paths', key, value)

            defpath = _abssource(self._repo)
            defpushpath = _abssource(self._repo, True)
            addpathconfig('default', defpath)
            if defpath != defpushpath:
                addpathconfig('default-push', defpushpath)
            fp.close()

    def add(self, ui, match, dryrun, prefix):
        return cmdutil.add(ui, self._repo, match, dryrun, True,
                           os.path.join(prefix, self._path))

    def status(self, rev2, **opts):
        try:
            rev1 = self._state[1]
            ctx1 = self._repo[rev1]
            ctx2 = self._repo[rev2]
            return self._repo.status(ctx1, ctx2, **opts)
        except error.RepoLookupError, inst:
            self._repo.ui.warn(_("warning: %s in %s\n")
                               % (inst, relpath(self)))
            return [], [], [], [], [], [], []

    def diff(self, diffopts, node2, match, prefix, **opts):
        try:
            node1 = node.bin(self._state[1])
            # We currently expect node2 to come from substate and be
            # in hex format
            if node2 is not None:
                node2 = node.bin(node2)
            cmdutil.diffordiffstat(self._repo.ui, self._repo, diffopts,
                                   node1, node2, match,
                                   prefix=os.path.join(prefix, self._path),
                                   listsubrepos=True, **opts)
        except error.RepoLookupError, inst:
            self._repo.ui.warn(_("warning: %s in %s\n")
                               % (inst, relpath(self)))

    def archive(self, archiver, prefix):
        abstractsubrepo.archive(self, archiver, prefix)

        rev = self._state[1]
        ctx = self._repo[rev]
        for subpath in ctx.substate:
            s = subrepo(ctx, subpath)
            s.archive(archiver, os.path.join(prefix, self._path))

    def dirty(self):
        r = self._state[1]
        if r == '':
            return True
        w = self._repo[None]
        if w.p1() != self._repo[r]: # version checked out change
            return True
        return w.dirty() # working directory changed

    def checknested(self, path):
        return self._repo._checknested(self._repo.wjoin(path))

    def commit(self, text, user, date):
        self._repo.ui.debug("committing subrepo %s\n" % relpath(self))
        n = self._repo.commit(text, user, date)
        if not n:
            return self._repo['.'].hex() # different version checked out
        return node.hex(n)

    def remove(self):
        # we can't fully delete the repository as it may contain
        # local-only history
        self._repo.ui.note(_('removing subrepo %s\n') % relpath(self))
        hg.clean(self._repo, node.nullid, False)

    def _get(self, state):
        source, revision, kind = state
        try:
            self._repo.lookup(revision)
        except error.RepoError:
            self._repo._subsource = source
            srcurl = _abssource(self._repo)
            self._repo.ui.status(_('pulling subrepo %s from %s\n')
                                 % (relpath(self), srcurl))
            other = hg.repository(self._repo.ui, srcurl)
            self._repo.pull(other)

    def get(self, state):
        self._get(state)
        source, revision, kind = state
        self._repo.ui.debug("getting subrepo %s\n" % self._path)
        hg.clean(self._repo, revision, False)

    def merge(self, state):
        self._get(state)
        cur = self._repo['.']
        dst = self._repo[state[1]]
        anc = dst.ancestor(cur)
        if anc == cur:
            self._repo.ui.debug("updating subrepo %s\n" % relpath(self))
            hg.update(self._repo, state[1])
        elif anc == dst:
            self._repo.ui.debug("skipping subrepo %s\n" % relpath(self))
        else:
            self._repo.ui.debug("merging subrepo %s\n" % relpath(self))
            hg.merge(self._repo, state[1], remind=False)

    def push(self, force):
        # push subrepos depth-first for coherent ordering
        c = self._repo['']
        subs = c.substate # only repos that are committed
        for s in sorted(subs):
            if not c.sub(s).push(force):
                return False

        dsturl = _abssource(self._repo, True)
        self._repo.ui.status(_('pushing subrepo %s to %s\n') %
            (relpath(self), dsturl))
        other = hg.repository(self._repo.ui, dsturl)
        return self._repo.push(other, force)

    def outgoing(self, ui, dest, opts):
        return hg.outgoing(ui, self._repo, _abssource(self._repo, True), opts)

    def incoming(self, ui, source, opts):
        return hg.incoming(ui, self._repo, _abssource(self._repo, False), opts)

    def files(self):
        rev = self._state[1]
        ctx = self._repo[rev]
        return ctx.manifest()

    def filedata(self, name):
        rev = self._state[1]
        return self._repo[rev][name].data()

    def fileflags(self, name):
        rev = self._state[1]
        ctx = self._repo[rev]
        return ctx.flags(name)


class svnsubrepo(abstractsubrepo):
    def __init__(self, ctx, path, state):
        self._path = path
        self._state = state
        self._ctx = ctx
        self._ui = ctx._repo.ui

    def _svncommand(self, commands, filename=''):
        path = os.path.join(self._ctx._repo.origroot, self._path, filename)
        cmd = ['svn'] + commands + [path]
        cmd = [util.shellquote(arg) for arg in cmd]
        cmd = util.quotecommand(' '.join(cmd))
        env = dict(os.environ)
        # Avoid localized output, preserve current locale for everything else.
        env['LC_MESSAGES'] = 'C'
        write, read, err = util.popen3(cmd, env=env, newlines=True)
        retdata = read.read()
        err = err.read().strip()
        if err:
            raise util.Abort(err)
        return retdata

    def _wcrev(self):
        output = self._svncommand(['info', '--xml'])
        doc = xml.dom.minidom.parseString(output)
        entries = doc.getElementsByTagName('entry')
        if not entries:
            return 0
        return int(entries[0].getAttribute('revision') or 0)

    def _wcchanged(self):
        """Return (changes, extchanges) where changes is True
        if the working directory was changed, and extchanges is
        True if any of these changes concern an external entry.
        """
        output = self._svncommand(['status', '--xml'])
        externals, changes = [], []
        doc = xml.dom.minidom.parseString(output)
        for e in doc.getElementsByTagName('entry'):
            s = e.getElementsByTagName('wc-status')
            if not s:
                continue
            item = s[0].getAttribute('item')
            props = s[0].getAttribute('props')
            path = e.getAttribute('path')
            if item == 'external':
                externals.append(path)
            if (item not in ('', 'normal', 'unversioned', 'external')
                or props not in ('', 'none')):
                changes.append(path)
        for path in changes:
            for ext in externals:
                if path == ext or path.startswith(ext + os.sep):
                    return True, True
        return bool(changes), False

    def dirty(self):
        if self._wcrev() == self._state[1] and not self._wcchanged()[0]:
            return False
        return True

    def commit(self, text, user, date):
        # user and date are out of our hands since svn is centralized
        changed, extchanged = self._wcchanged()
        if not changed:
            return self._wcrev()
        if extchanged:
            # Do not try to commit externals
            raise util.Abort(_('cannot commit svn externals'))
        commitinfo = self._svncommand(['commit', '-m', text])
        self._ui.status(commitinfo)
        newrev = re.search('Committed revision ([0-9]+).', commitinfo)
        if not newrev:
            raise util.Abort(commitinfo.splitlines()[-1])
        newrev = newrev.groups()[0]
        self._ui.status(self._svncommand(['update', '-r', newrev]))
        return newrev

    def remove(self):
        if self.dirty():
            self._ui.warn(_('not removing repo %s because '
                            'it has changes.\n' % self._path))
            return
        self._ui.note(_('removing subrepo %s\n') % self._path)
        shutil.rmtree(self._ctx.repo.join(self._path))

    def get(self, state):
        status = self._svncommand(['checkout', state[0], '--revision', state[1]])
        if not re.search('Checked out revision [0-9]+.', status):
            raise util.Abort(status.splitlines()[-1])
        self._ui.status(status)

    def merge(self, state):
        old = int(self._state[1])
        new = int(state[1])
        if new > old:
            self.get(state)

    def push(self, force):
        # push is a no-op for SVN
        return True

    def files(self):
        output = self._svncommand(['list'])
        # This works because svn forbids \n in filenames.
        return output.splitlines()

    def filedata(self, name):
        return self._svncommand(['cat'], name)


types = {
    'hg': hgsubrepo,
    'svn': svnsubrepo,
    }
