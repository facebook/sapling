# cmdutil.py - help for command processing in mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import hex, nullid, nullrev, short
from i18n import _
import os, sys, errno, re, glob, tempfile
import util, templater, patch, error, encoding, templatekw
import match as matchmod
import similar, revset, subrepo

revrangesep = ':'

def parsealiases(cmd):
    return cmd.lstrip("^").split("|")

def findpossible(cmd, table, strict=False):
    """
    Return cmd -> (aliases, command table entry)
    for each matching command.
    Return debug commands (or their aliases) only if no normal command matches.
    """
    choice = {}
    debugchoice = {}
    for e in table.keys():
        aliases = parsealiases(e)
        found = None
        if cmd in aliases:
            found = cmd
        elif not strict:
            for a in aliases:
                if a.startswith(cmd):
                    found = a
                    break
        if found is not None:
            if aliases[0].startswith("debug") or found.startswith("debug"):
                debugchoice[found] = (aliases, table[e])
            else:
                choice[found] = (aliases, table[e])

    if not choice and debugchoice:
        choice = debugchoice

    return choice

def findcmd(cmd, table, strict=True):
    """Return (aliases, command table entry) for command string."""
    choice = findpossible(cmd, table, strict)

    if cmd in choice:
        return choice[cmd]

    if len(choice) > 1:
        clist = choice.keys()
        clist.sort()
        raise error.AmbiguousCommand(cmd, clist)

    if choice:
        return choice.values()[0]

    raise error.UnknownCommand(cmd)

def findrepo(p):
    while not os.path.isdir(os.path.join(p, ".hg")):
        oldp, p = p, os.path.dirname(p)
        if p == oldp:
            return None

    return p

def bail_if_changed(repo):
    if repo.dirstate.parents()[1] != nullid:
        raise util.Abort(_('outstanding uncommitted merge'))
    modified, added, removed, deleted = repo.status()[:4]
    if modified or added or removed or deleted:
        raise util.Abort(_("outstanding uncommitted changes"))

def logmessage(opts):
    """ get the log message according to -m and -l option """
    message = opts.get('message')
    logfile = opts.get('logfile')

    if message and logfile:
        raise util.Abort(_('options --message and --logfile are mutually '
                           'exclusive'))
    if not message and logfile:
        try:
            if logfile == '-':
                message = sys.stdin.read()
            else:
                message = open(logfile).read()
        except IOError, inst:
            raise util.Abort(_("can't read commit message '%s': %s") %
                             (logfile, inst.strerror))
    return message

def loglimit(opts):
    """get the log limit according to option -l/--limit"""
    limit = opts.get('limit')
    if limit:
        try:
            limit = int(limit)
        except ValueError:
            raise util.Abort(_('limit must be a positive integer'))
        if limit <= 0:
            raise util.Abort(_('limit must be positive'))
    else:
        limit = None
    return limit

def revsingle(repo, revspec, default='.'):
    if not revspec:
        return repo[default]

    l = revrange(repo, [revspec])
    if len(l) < 1:
        raise util.Abort(_('empty revision set'))
    return repo[l[-1]]

def revpair(repo, revs):
    if not revs:
        return repo.dirstate.parents()[0], None

    l = revrange(repo, revs)

    if len(l) == 0:
        return repo.dirstate.parents()[0], None

    if len(l) == 1:
        return repo.lookup(l[0]), None

    return repo.lookup(l[0]), repo.lookup(l[-1])

def revrange(repo, revs):
    """Yield revision as strings from a list of revision specifications."""

    def revfix(repo, val, defval):
        if not val and val != 0 and defval is not None:
            return defval
        return repo.changelog.rev(repo.lookup(val))

    seen, l = set(), []
    for spec in revs:
        # attempt to parse old-style ranges first to deal with
        # things like old-tag which contain query metacharacters
        try:
            if isinstance(spec, int):
                seen.add(spec)
                l.append(spec)
                continue

            if revrangesep in spec:
                start, end = spec.split(revrangesep, 1)
                start = revfix(repo, start, 0)
                end = revfix(repo, end, len(repo) - 1)
                step = start > end and -1 or 1
                for rev in xrange(start, end + step, step):
                    if rev in seen:
                        continue
                    seen.add(rev)
                    l.append(rev)
                continue
            elif spec and spec in repo: # single unquoted rev
                rev = revfix(repo, spec, None)
                if rev in seen:
                    continue
                seen.add(rev)
                l.append(rev)
                continue
        except error.RepoLookupError:
            pass

        # fall through to new-style queries if old-style fails
        m = revset.match(spec)
        for r in m(repo, range(len(repo))):
            if r not in seen:
                l.append(r)
        seen.update(l)

    return l

def make_filename(repo, pat, node,
                  total=None, seqno=None, revwidth=None, pathname=None):
    node_expander = {
        'H': lambda: hex(node),
        'R': lambda: str(repo.changelog.rev(node)),
        'h': lambda: short(node),
        }
    expander = {
        '%': lambda: '%',
        'b': lambda: os.path.basename(repo.root),
        }

    try:
        if node:
            expander.update(node_expander)
        if node:
            expander['r'] = (lambda:
                    str(repo.changelog.rev(node)).zfill(revwidth or 0))
        if total is not None:
            expander['N'] = lambda: str(total)
        if seqno is not None:
            expander['n'] = lambda: str(seqno)
        if total is not None and seqno is not None:
            expander['n'] = lambda: str(seqno).zfill(len(str(total)))
        if pathname is not None:
            expander['s'] = lambda: os.path.basename(pathname)
            expander['d'] = lambda: os.path.dirname(pathname) or '.'
            expander['p'] = lambda: pathname

        newname = []
        patlen = len(pat)
        i = 0
        while i < patlen:
            c = pat[i]
            if c == '%':
                i += 1
                c = pat[i]
                c = expander[c]()
            newname.append(c)
            i += 1
        return ''.join(newname)
    except KeyError, inst:
        raise util.Abort(_("invalid format spec '%%%s' in output filename") %
                         inst.args[0])

def make_file(repo, pat, node=None,
              total=None, seqno=None, revwidth=None, mode='wb', pathname=None):

    writable = 'w' in mode or 'a' in mode

    if not pat or pat == '-':
        fp = writable and sys.stdout or sys.stdin
        return os.fdopen(os.dup(fp.fileno()), mode)
    if hasattr(pat, 'write') and writable:
        return pat
    if hasattr(pat, 'read') and 'r' in mode:
        return pat
    return open(make_filename(repo, pat, node, total, seqno, revwidth,
                              pathname),
                mode)

def expandpats(pats):
    if not util.expandglobs:
        return list(pats)
    ret = []
    for p in pats:
        kind, name = matchmod._patsplit(p, None)
        if kind is None:
            try:
                globbed = glob.glob(name)
            except re.error:
                globbed = [name]
            if globbed:
                ret.extend(globbed)
                continue
        ret.append(p)
    return ret

def match(repo, pats=[], opts={}, globbed=False, default='relpath'):
    if pats == ("",):
        pats = []
    if not globbed and default == 'relpath':
        pats = expandpats(pats or [])
    m = matchmod.match(repo.root, repo.getcwd(), pats,
                       opts.get('include'), opts.get('exclude'), default,
                       auditor=repo.auditor)
    def badfn(f, msg):
        repo.ui.warn("%s: %s\n" % (m.rel(f), msg))
    m.bad = badfn
    return m

def matchall(repo):
    return matchmod.always(repo.root, repo.getcwd())

def matchfiles(repo, files):
    return matchmod.exact(repo.root, repo.getcwd(), files)

def addremove(repo, pats=[], opts={}, dry_run=None, similarity=None):
    if dry_run is None:
        dry_run = opts.get('dry_run')
    if similarity is None:
        similarity = float(opts.get('similarity') or 0)
    # we'd use status here, except handling of symlinks and ignore is tricky
    added, unknown, deleted, removed = [], [], [], []
    audit_path = util.path_auditor(repo.root)
    m = match(repo, pats, opts)
    for abs in repo.walk(m):
        target = repo.wjoin(abs)
        good = True
        try:
            audit_path(abs)
        except:
            good = False
        rel = m.rel(abs)
        exact = m.exact(abs)
        if good and abs not in repo.dirstate:
            unknown.append(abs)
            if repo.ui.verbose or not exact:
                repo.ui.status(_('adding %s\n') % ((pats and rel) or abs))
        elif repo.dirstate[abs] != 'r' and (not good or not os.path.lexists(target)
            or (os.path.isdir(target) and not os.path.islink(target))):
            deleted.append(abs)
            if repo.ui.verbose or not exact:
                repo.ui.status(_('removing %s\n') % ((pats and rel) or abs))
        # for finding renames
        elif repo.dirstate[abs] == 'r':
            removed.append(abs)
        elif repo.dirstate[abs] == 'a':
            added.append(abs)
    copies = {}
    if similarity > 0:
        for old, new, score in similar.findrenames(repo,
                added + unknown, removed + deleted, similarity):
            if repo.ui.verbose or not m.exact(old) or not m.exact(new):
                repo.ui.status(_('recording removal of %s as rename to %s '
                                 '(%d%% similar)\n') %
                               (m.rel(old), m.rel(new), score * 100))
            copies[new] = old

    if not dry_run:
        wctx = repo[None]
        wlock = repo.wlock()
        try:
            wctx.remove(deleted)
            wctx.add(unknown)
            for new, old in copies.iteritems():
                wctx.copy(old, new)
        finally:
            wlock.release()

def updatedir(ui, repo, patches, similarity=0):
    '''Update dirstate after patch application according to metadata'''
    if not patches:
        return
    copies = []
    removes = set()
    cfiles = patches.keys()
    cwd = repo.getcwd()
    if cwd:
        cfiles = [util.pathto(repo.root, cwd, f) for f in patches.keys()]
    for f in patches:
        gp = patches[f]
        if not gp:
            continue
        if gp.op == 'RENAME':
            copies.append((gp.oldpath, gp.path))
            removes.add(gp.oldpath)
        elif gp.op == 'COPY':
            copies.append((gp.oldpath, gp.path))
        elif gp.op == 'DELETE':
            removes.add(gp.path)

    wctx = repo[None]
    for src, dst in copies:
        dirstatecopy(ui, repo, wctx, src, dst, cwd=cwd)
    if (not similarity) and removes:
        wctx.remove(sorted(removes), True)

    for f in patches:
        gp = patches[f]
        if gp and gp.mode:
            islink, isexec = gp.mode
            dst = repo.wjoin(gp.path)
            # patch won't create empty files
            if gp.op == 'ADD' and not os.path.lexists(dst):
                flags = (isexec and 'x' or '') + (islink and 'l' or '')
                repo.wwrite(gp.path, '', flags)
            util.set_flags(dst, islink, isexec)
    addremove(repo, cfiles, similarity=similarity)
    files = patches.keys()
    files.extend([r for r in removes if r not in files])
    return sorted(files)

def dirstatecopy(ui, repo, wctx, src, dst, dryrun=False, cwd=None):
    """Update the dirstate to reflect the intent of copying src to dst. For
    different reasons it might not end with dst being marked as copied from src.
    """
    origsrc = repo.dirstate.copied(src) or src
    if dst == origsrc: # copying back a copy?
        if repo.dirstate[dst] not in 'mn' and not dryrun:
            repo.dirstate.normallookup(dst)
    else:
        if repo.dirstate[origsrc] == 'a' and origsrc == src:
            if not ui.quiet:
                ui.warn(_("%s has not been committed yet, so no copy "
                          "data will be stored for %s.\n")
                        % (repo.pathto(origsrc, cwd), repo.pathto(dst, cwd)))
            if repo.dirstate[dst] in '?r' and not dryrun:
                wctx.add([dst])
        elif not dryrun:
            wctx.copy(origsrc, dst)

def copy(ui, repo, pats, opts, rename=False):
    # called with the repo lock held
    #
    # hgsep => pathname that uses "/" to separate directories
    # ossep => pathname that uses os.sep to separate directories
    cwd = repo.getcwd()
    targets = {}
    after = opts.get("after")
    dryrun = opts.get("dry_run")
    wctx = repo[None]

    def walkpat(pat):
        srcs = []
        badstates = after and '?' or '?r'
        m = match(repo, [pat], opts, globbed=True)
        for abs in repo.walk(m):
            state = repo.dirstate[abs]
            rel = m.rel(abs)
            exact = m.exact(abs)
            if state in badstates:
                if exact and state == '?':
                    ui.warn(_('%s: not copying - file is not managed\n') % rel)
                if exact and state == 'r':
                    ui.warn(_('%s: not copying - file has been marked for'
                              ' remove\n') % rel)
                continue
            # abs: hgsep
            # rel: ossep
            srcs.append((abs, rel, exact))
        return srcs

    # abssrc: hgsep
    # relsrc: ossep
    # otarget: ossep
    def copyfile(abssrc, relsrc, otarget, exact):
        abstarget = util.canonpath(repo.root, cwd, otarget)
        reltarget = repo.pathto(abstarget, cwd)
        target = repo.wjoin(abstarget)
        src = repo.wjoin(abssrc)
        state = repo.dirstate[abstarget]

        # check for collisions
        prevsrc = targets.get(abstarget)
        if prevsrc is not None:
            ui.warn(_('%s: not overwriting - %s collides with %s\n') %
                    (reltarget, repo.pathto(abssrc, cwd),
                     repo.pathto(prevsrc, cwd)))
            return

        # check for overwrites
        exists = os.path.lexists(target)
        if not after and exists or after and state in 'mn':
            if not opts['force']:
                ui.warn(_('%s: not overwriting - file exists\n') %
                        reltarget)
                return

        if after:
            if not exists:
                if rename:
                    ui.warn(_('%s: not recording move - %s does not exist\n') %
                            (relsrc, reltarget))
                else:
                    ui.warn(_('%s: not recording copy - %s does not exist\n') %
                            (relsrc, reltarget))
                return
        elif not dryrun:
            try:
                if exists:
                    os.unlink(target)
                targetdir = os.path.dirname(target) or '.'
                if not os.path.isdir(targetdir):
                    os.makedirs(targetdir)
                util.copyfile(src, target)
            except IOError, inst:
                if inst.errno == errno.ENOENT:
                    ui.warn(_('%s: deleted in working copy\n') % relsrc)
                else:
                    ui.warn(_('%s: cannot copy - %s\n') %
                            (relsrc, inst.strerror))
                    return True # report a failure

        if ui.verbose or not exact:
            if rename:
                ui.status(_('moving %s to %s\n') % (relsrc, reltarget))
            else:
                ui.status(_('copying %s to %s\n') % (relsrc, reltarget))

        targets[abstarget] = abssrc

        # fix up dirstate
        dirstatecopy(ui, repo, wctx, abssrc, abstarget, dryrun=dryrun, cwd=cwd)
        if rename and not dryrun:
            wctx.remove([abssrc], not after)

    # pat: ossep
    # dest ossep
    # srcs: list of (hgsep, hgsep, ossep, bool)
    # return: function that takes hgsep and returns ossep
    def targetpathfn(pat, dest, srcs):
        if os.path.isdir(pat):
            abspfx = util.canonpath(repo.root, cwd, pat)
            abspfx = util.localpath(abspfx)
            if destdirexists:
                striplen = len(os.path.split(abspfx)[0])
            else:
                striplen = len(abspfx)
            if striplen:
                striplen += len(os.sep)
            res = lambda p: os.path.join(dest, util.localpath(p)[striplen:])
        elif destdirexists:
            res = lambda p: os.path.join(dest,
                                         os.path.basename(util.localpath(p)))
        else:
            res = lambda p: dest
        return res

    # pat: ossep
    # dest ossep
    # srcs: list of (hgsep, hgsep, ossep, bool)
    # return: function that takes hgsep and returns ossep
    def targetpathafterfn(pat, dest, srcs):
        if matchmod.patkind(pat):
            # a mercurial pattern
            res = lambda p: os.path.join(dest,
                                         os.path.basename(util.localpath(p)))
        else:
            abspfx = util.canonpath(repo.root, cwd, pat)
            if len(abspfx) < len(srcs[0][0]):
                # A directory. Either the target path contains the last
                # component of the source path or it does not.
                def evalpath(striplen):
                    score = 0
                    for s in srcs:
                        t = os.path.join(dest, util.localpath(s[0])[striplen:])
                        if os.path.lexists(t):
                            score += 1
                    return score

                abspfx = util.localpath(abspfx)
                striplen = len(abspfx)
                if striplen:
                    striplen += len(os.sep)
                if os.path.isdir(os.path.join(dest, os.path.split(abspfx)[1])):
                    score = evalpath(striplen)
                    striplen1 = len(os.path.split(abspfx)[0])
                    if striplen1:
                        striplen1 += len(os.sep)
                    if evalpath(striplen1) > score:
                        striplen = striplen1
                res = lambda p: os.path.join(dest,
                                             util.localpath(p)[striplen:])
            else:
                # a file
                if destdirexists:
                    res = lambda p: os.path.join(dest,
                                        os.path.basename(util.localpath(p)))
                else:
                    res = lambda p: dest
        return res


    pats = expandpats(pats)
    if not pats:
        raise util.Abort(_('no source or destination specified'))
    if len(pats) == 1:
        raise util.Abort(_('no destination specified'))
    dest = pats.pop()
    destdirexists = os.path.isdir(dest) and not os.path.islink(dest)
    if not destdirexists:
        if len(pats) > 1 or matchmod.patkind(pats[0]):
            raise util.Abort(_('with multiple sources, destination must be an '
                               'existing directory'))
        if util.endswithsep(dest):
            raise util.Abort(_('destination %s is not a directory') % dest)

    tfn = targetpathfn
    if after:
        tfn = targetpathafterfn
    copylist = []
    for pat in pats:
        srcs = walkpat(pat)
        if not srcs:
            continue
        copylist.append((tfn(pat, dest, srcs), srcs))
    if not copylist:
        raise util.Abort(_('no files to copy'))

    errors = 0
    for targetpath, srcs in copylist:
        for abssrc, relsrc, exact in srcs:
            if copyfile(abssrc, relsrc, targetpath(abssrc), exact):
                errors += 1

    if errors:
        ui.warn(_('(consider using --after)\n'))

    return errors != 0

def service(opts, parentfn=None, initfn=None, runfn=None, logfile=None,
    runargs=None, appendpid=False):
    '''Run a command as a service.'''

    if opts['daemon'] and not opts['daemon_pipefds']:
        # Signal child process startup with file removal
        lockfd, lockpath = tempfile.mkstemp(prefix='hg-service-')
        os.close(lockfd)
        try:
            if not runargs:
                runargs = util.hgcmd() + sys.argv[1:]
            runargs.append('--daemon-pipefds=%s' % lockpath)
            # Don't pass --cwd to the child process, because we've already
            # changed directory.
            for i in xrange(1, len(runargs)):
                if runargs[i].startswith('--cwd='):
                    del runargs[i]
                    break
                elif runargs[i].startswith('--cwd'):
                    del runargs[i:i + 2]
                    break
            def condfn():
                return not os.path.exists(lockpath)
            pid = util.rundetached(runargs, condfn)
            if pid < 0:
                raise util.Abort(_('child process failed to start'))
        finally:
            try:
                os.unlink(lockpath)
            except OSError, e:
                if e.errno != errno.ENOENT:
                    raise
        if parentfn:
            return parentfn(pid)
        else:
            return

    if initfn:
        initfn()

    if opts['pid_file']:
        mode = appendpid and 'a' or 'w'
        fp = open(opts['pid_file'], mode)
        fp.write(str(os.getpid()) + '\n')
        fp.close()

    if opts['daemon_pipefds']:
        lockpath = opts['daemon_pipefds']
        try:
            os.setsid()
        except AttributeError:
            pass
        os.unlink(lockpath)
        util.hidewindow()
        sys.stdout.flush()
        sys.stderr.flush()

        nullfd = os.open(util.nulldev, os.O_RDWR)
        logfilefd = nullfd
        if logfile:
            logfilefd = os.open(logfile, os.O_RDWR | os.O_CREAT | os.O_APPEND)
        os.dup2(nullfd, 0)
        os.dup2(logfilefd, 1)
        os.dup2(logfilefd, 2)
        if nullfd not in (0, 1, 2):
            os.close(nullfd)
        if logfile and logfilefd not in (0, 1, 2):
            os.close(logfilefd)

    if runfn:
        return runfn()

def export(repo, revs, template='hg-%h.patch', fp=None, switch_parent=False,
           opts=None):
    '''export changesets as hg patches.'''

    total = len(revs)
    revwidth = max([len(str(rev)) for rev in revs])

    def single(rev, seqno, fp):
        ctx = repo[rev]
        node = ctx.node()
        parents = [p.node() for p in ctx.parents() if p]
        branch = ctx.branch()
        if switch_parent:
            parents.reverse()
        prev = (parents and parents[0]) or nullid

        shouldclose = False
        if not fp:
            fp = make_file(repo, template, node, total=total, seqno=seqno,
                           revwidth=revwidth, mode='ab')
            if fp != template:
                shouldclose = True
        if fp != sys.stdout and hasattr(fp, 'name'):
            repo.ui.note("%s\n" % fp.name)

        fp.write("# HG changeset patch\n")
        fp.write("# User %s\n" % ctx.user())
        fp.write("# Date %d %d\n" % ctx.date())
        if branch and branch != 'default':
            fp.write("# Branch %s\n" % branch)
        fp.write("# Node ID %s\n" % hex(node))
        fp.write("# Parent  %s\n" % hex(prev))
        if len(parents) > 1:
            fp.write("# Parent  %s\n" % hex(parents[1]))
        fp.write(ctx.description().rstrip())
        fp.write("\n\n")

        for chunk in patch.diff(repo, prev, node, opts=opts):
            fp.write(chunk)

        if shouldclose:
            fp.close()

    for seqno, rev in enumerate(revs):
        single(rev, seqno + 1, fp)

def diffordiffstat(ui, repo, diffopts, node1, node2, match,
                   changes=None, stat=False, fp=None, prefix='',
                   listsubrepos=False):
    '''show diff or diffstat.'''
    if fp is None:
        write = ui.write
    else:
        def write(s, **kw):
            fp.write(s)

    if stat:
        diffopts = diffopts.copy(context=0)
        width = 80
        if not ui.plain():
            width = ui.termwidth()
        chunks = patch.diff(repo, node1, node2, match, changes, diffopts,
                            prefix=prefix)
        for chunk, label in patch.diffstatui(util.iterlines(chunks),
                                             width=width,
                                             git=diffopts.git):
            write(chunk, label=label)
    else:
        for chunk, label in patch.diffui(repo, node1, node2, match,
                                         changes, diffopts, prefix=prefix):
            write(chunk, label=label)

    if listsubrepos:
        ctx1 = repo[node1]
        ctx2 = repo[node2]
        for subpath, sub in subrepo.itersubrepos(ctx1, ctx2):
            if node2 is not None:
                node2 = ctx2.substate[subpath][1]
            submatch = matchmod.narrowmatcher(subpath, match)
            sub.diff(diffopts, node2, submatch, changes=changes,
                     stat=stat, fp=fp, prefix=prefix)

class changeset_printer(object):
    '''show changeset information when templating not requested.'''

    def __init__(self, ui, repo, patch, diffopts, buffered):
        self.ui = ui
        self.repo = repo
        self.buffered = buffered
        self.patch = patch
        self.diffopts = diffopts
        self.header = {}
        self.hunk = {}
        self.lastheader = None
        self.footer = None

    def flush(self, rev):
        if rev in self.header:
            h = self.header[rev]
            if h != self.lastheader:
                self.lastheader = h
                self.ui.write(h)
            del self.header[rev]
        if rev in self.hunk:
            self.ui.write(self.hunk[rev])
            del self.hunk[rev]
            return 1
        return 0

    def close(self):
        if self.footer:
            self.ui.write(self.footer)

    def show(self, ctx, copies=None, matchfn=None, **props):
        if self.buffered:
            self.ui.pushbuffer()
            self._show(ctx, copies, matchfn, props)
            self.hunk[ctx.rev()] = self.ui.popbuffer(labeled=True)
        else:
            self._show(ctx, copies, matchfn, props)

    def _show(self, ctx, copies, matchfn, props):
        '''show a single changeset or file revision'''
        changenode = ctx.node()
        rev = ctx.rev()

        if self.ui.quiet:
            self.ui.write("%d:%s\n" % (rev, short(changenode)),
                          label='log.node')
            return

        log = self.repo.changelog
        date = util.datestr(ctx.date())

        hexfunc = self.ui.debugflag and hex or short

        parents = [(p, hexfunc(log.node(p)))
                   for p in self._meaningful_parentrevs(log, rev)]

        self.ui.write(_("changeset:   %d:%s\n") % (rev, hexfunc(changenode)),
                      label='log.changeset')

        branch = ctx.branch()
        # don't show the default branch name
        if branch != 'default':
            self.ui.write(_("branch:      %s\n") % branch,
                          label='log.branch')
        for bookmark in self.repo.nodebookmarks(changenode):
            self.ui.write(_("bookmark:    %s\n") % bookmark,
                    label='log.bookmark')
        for tag in self.repo.nodetags(changenode):
            self.ui.write(_("tag:         %s\n") % tag,
                          label='log.tag')
        for parent in parents:
            self.ui.write(_("parent:      %d:%s\n") % parent,
                          label='log.parent')

        if self.ui.debugflag:
            mnode = ctx.manifestnode()
            self.ui.write(_("manifest:    %d:%s\n") %
                          (self.repo.manifest.rev(mnode), hex(mnode)),
                          label='ui.debug log.manifest')
        self.ui.write(_("user:        %s\n") % ctx.user(),
                      label='log.user')
        self.ui.write(_("date:        %s\n") % date,
                      label='log.date')

        if self.ui.debugflag:
            files = self.repo.status(log.parents(changenode)[0], changenode)[:3]
            for key, value in zip([_("files:"), _("files+:"), _("files-:")],
                                  files):
                if value:
                    self.ui.write("%-12s %s\n" % (key, " ".join(value)),
                                  label='ui.debug log.files')
        elif ctx.files() and self.ui.verbose:
            self.ui.write(_("files:       %s\n") % " ".join(ctx.files()),
                          label='ui.note log.files')
        if copies and self.ui.verbose:
            copies = ['%s (%s)' % c for c in copies]
            self.ui.write(_("copies:      %s\n") % ' '.join(copies),
                          label='ui.note log.copies')

        extra = ctx.extra()
        if extra and self.ui.debugflag:
            for key, value in sorted(extra.items()):
                self.ui.write(_("extra:       %s=%s\n")
                              % (key, value.encode('string_escape')),
                              label='ui.debug log.extra')

        description = ctx.description().strip()
        if description:
            if self.ui.verbose:
                self.ui.write(_("description:\n"),
                              label='ui.note log.description')
                self.ui.write(description,
                              label='ui.note log.description')
                self.ui.write("\n\n")
            else:
                self.ui.write(_("summary:     %s\n") %
                              description.splitlines()[0],
                              label='log.summary')
        self.ui.write("\n")

        self.showpatch(changenode, matchfn)

    def showpatch(self, node, matchfn):
        if not matchfn:
            matchfn = self.patch
        if matchfn:
            stat = self.diffopts.get('stat')
            diff = self.diffopts.get('patch')
            diffopts = patch.diffopts(self.ui, self.diffopts)
            prev = self.repo.changelog.parents(node)[0]
            if stat:
                diffordiffstat(self.ui, self.repo, diffopts, prev, node,
                               match=matchfn, stat=True)
            if diff:
                if stat:
                    self.ui.write("\n")
                diffordiffstat(self.ui, self.repo, diffopts, prev, node,
                               match=matchfn, stat=False)
            self.ui.write("\n")

    def _meaningful_parentrevs(self, log, rev):
        """Return list of meaningful (or all if debug) parentrevs for rev.

        For merges (two non-nullrev revisions) both parents are meaningful.
        Otherwise the first parent revision is considered meaningful if it
        is not the preceding revision.
        """
        parents = log.parentrevs(rev)
        if not self.ui.debugflag and parents[1] == nullrev:
            if parents[0] >= rev - 1:
                parents = []
            else:
                parents = [parents[0]]
        return parents


class changeset_templater(changeset_printer):
    '''format changeset information.'''

    def __init__(self, ui, repo, patch, diffopts, mapfile, buffered):
        changeset_printer.__init__(self, ui, repo, patch, diffopts, buffered)
        formatnode = ui.debugflag and (lambda x: x) or (lambda x: x[:12])
        defaulttempl = {
            'parent': '{rev}:{node|formatnode} ',
            'manifest': '{rev}:{node|formatnode}',
            'file_copy': '{name} ({source})',
            'extra': '{key}={value|stringescape}'
            }
        # filecopy is preserved for compatibility reasons
        defaulttempl['filecopy'] = defaulttempl['file_copy']
        self.t = templater.templater(mapfile, {'formatnode': formatnode},
                                     cache=defaulttempl)
        self.cache = {}

    def use_template(self, t):
        '''set template string to use'''
        self.t.cache['changeset'] = t

    def _meaningful_parentrevs(self, ctx):
        """Return list of meaningful (or all if debug) parentrevs for rev.
        """
        parents = ctx.parents()
        if len(parents) > 1:
            return parents
        if self.ui.debugflag:
            return [parents[0], self.repo['null']]
        if parents[0].rev() >= ctx.rev() - 1:
            return []
        return parents

    def _show(self, ctx, copies, matchfn, props):
        '''show a single changeset or file revision'''

        showlist = templatekw.showlist

        # showparents() behaviour depends on ui trace level which
        # causes unexpected behaviours at templating level and makes
        # it harder to extract it in a standalone function. Its
        # behaviour cannot be changed so leave it here for now.
        def showparents(**args):
            ctx = args['ctx']
            parents = [[('rev', p.rev()), ('node', p.hex())]
                       for p in self._meaningful_parentrevs(ctx)]
            return showlist('parent', parents, **args)

        props = props.copy()
        props.update(templatekw.keywords)
        props['parents'] = showparents
        props['templ'] = self.t
        props['ctx'] = ctx
        props['repo'] = self.repo
        props['revcache'] = {'copies': copies}
        props['cache'] = self.cache

        # find correct templates for current mode

        tmplmodes = [
            (True, None),
            (self.ui.verbose, 'verbose'),
            (self.ui.quiet, 'quiet'),
            (self.ui.debugflag, 'debug'),
        ]

        types = {'header': '', 'footer':'', 'changeset': 'changeset'}
        for mode, postfix  in tmplmodes:
            for type in types:
                cur = postfix and ('%s_%s' % (type, postfix)) or type
                if mode and cur in self.t:
                    types[type] = cur

        try:

            # write header
            if types['header']:
                h = templater.stringify(self.t(types['header'], **props))
                if self.buffered:
                    self.header[ctx.rev()] = h
                else:
                    if self.lastheader != h:
                        self.lastheader = h
                        self.ui.write(h)

            # write changeset metadata, then patch if requested
            key = types['changeset']
            self.ui.write(templater.stringify(self.t(key, **props)))
            self.showpatch(ctx.node(), matchfn)

            if types['footer']:
                if not self.footer:
                    self.footer = templater.stringify(self.t(types['footer'],
                                                      **props))

        except KeyError, inst:
            msg = _("%s: no key named '%s'")
            raise util.Abort(msg % (self.t.mapfile, inst.args[0]))
        except SyntaxError, inst:
            raise util.Abort('%s: %s' % (self.t.mapfile, inst.args[0]))

def show_changeset(ui, repo, opts, buffered=False):
    """show one changeset using template or regular display.

    Display format will be the first non-empty hit of:
    1. option 'template'
    2. option 'style'
    3. [ui] setting 'logtemplate'
    4. [ui] setting 'style'
    If all of these values are either the unset or the empty string,
    regular display via changeset_printer() is done.
    """
    # options
    patch = False
    if opts.get('patch') or opts.get('stat'):
        patch = matchall(repo)

    tmpl = opts.get('template')
    style = None
    if tmpl:
        tmpl = templater.parsestring(tmpl, quoted=False)
    else:
        style = opts.get('style')

    # ui settings
    if not (tmpl or style):
        tmpl = ui.config('ui', 'logtemplate')
        if tmpl:
            tmpl = templater.parsestring(tmpl)
        else:
            style = util.expandpath(ui.config('ui', 'style', ''))

    if not (tmpl or style):
        return changeset_printer(ui, repo, patch, opts, buffered)

    mapfile = None
    if style and not tmpl:
        mapfile = style
        if not os.path.split(mapfile)[0]:
            mapname = (templater.templatepath('map-cmdline.' + mapfile)
                       or templater.templatepath(mapfile))
            if mapname:
                mapfile = mapname

    try:
        t = changeset_templater(ui, repo, patch, opts, mapfile, buffered)
    except SyntaxError, inst:
        raise util.Abort(inst.args[0])
    if tmpl:
        t.use_template(tmpl)
    return t

def finddate(ui, repo, date):
    """Find the tipmost changeset that matches the given date spec"""

    df = util.matchdate(date)
    m = matchall(repo)
    results = {}

    def prep(ctx, fns):
        d = ctx.date()
        if df(d[0]):
            results[ctx.rev()] = d

    for ctx in walkchangerevs(repo, m, {'rev': None}, prep):
        rev = ctx.rev()
        if rev in results:
            ui.status(_("Found revision %s from %s\n") %
                      (rev, util.datestr(results[rev])))
            return str(rev)

    raise util.Abort(_("revision matching date not found"))

def walkchangerevs(repo, match, opts, prepare):
    '''Iterate over files and the revs in which they changed.

    Callers most commonly need to iterate backwards over the history
    in which they are interested. Doing so has awful (quadratic-looking)
    performance, so we use iterators in a "windowed" way.

    We walk a window of revisions in the desired order.  Within the
    window, we first walk forwards to gather data, then in the desired
    order (usually backwards) to display it.

    This function returns an iterator yielding contexts. Before
    yielding each context, the iterator will first call the prepare
    function on each context in the window in forward order.'''

    def increasing_windows(start, end, windowsize=8, sizelimit=512):
        if start < end:
            while start < end:
                yield start, min(windowsize, end - start)
                start += windowsize
                if windowsize < sizelimit:
                    windowsize *= 2
        else:
            while start > end:
                yield start, min(windowsize, start - end - 1)
                start -= windowsize
                if windowsize < sizelimit:
                    windowsize *= 2

    follow = opts.get('follow') or opts.get('follow_first')

    if not len(repo):
        return []

    if follow:
        defrange = '%s:0' % repo['.'].rev()
    else:
        defrange = '-1:0'
    revs = revrange(repo, opts['rev'] or [defrange])
    if not revs:
        return []
    wanted = set()
    slowpath = match.anypats() or (match.files() and opts.get('removed'))
    fncache = {}
    change = util.cachefunc(repo.changectx)

    # First step is to fill wanted, the set of revisions that we want to yield.
    # When it does not induce extra cost, we also fill fncache for revisions in
    # wanted: a cache of filenames that were changed (ctx.files()) and that
    # match the file filtering conditions.

    if not slowpath and not match.files():
        # No files, no patterns.  Display all revs.
        wanted = set(revs)
    copies = []

    if not slowpath:
        # We only have to read through the filelog to find wanted revisions

        minrev, maxrev = min(revs), max(revs)
        def filerevgen(filelog, last):
            """
            Only files, no patterns.  Check the history of each file.

            Examines filelog entries within minrev, maxrev linkrev range
            Returns an iterator yielding (linkrev, parentlinkrevs, copied)
            tuples in backwards order
            """
            cl_count = len(repo)
            revs = []
            for j in xrange(0, last + 1):
                linkrev = filelog.linkrev(j)
                if linkrev < minrev:
                    continue
                # only yield rev for which we have the changelog, it can
                # happen while doing "hg log" during a pull or commit
                if linkrev >= cl_count:
                    break

                parentlinkrevs = []
                for p in filelog.parentrevs(j):
                    if p != nullrev:
                        parentlinkrevs.append(filelog.linkrev(p))
                n = filelog.node(j)
                revs.append((linkrev, parentlinkrevs,
                             follow and filelog.renamed(n)))

            return reversed(revs)
        def iterfiles():
            for filename in match.files():
                yield filename, None
            for filename_node in copies:
                yield filename_node
        for file_, node in iterfiles():
            filelog = repo.file(file_)
            if not len(filelog):
                if node is None:
                    # A zero count may be a directory or deleted file, so
                    # try to find matching entries on the slow path.
                    if follow:
                        raise util.Abort(
                            _('cannot follow nonexistent file: "%s"') % file_)
                    slowpath = True
                    break
                else:
                    continue

            if node is None:
                last = len(filelog) - 1
            else:
                last = filelog.rev(node)


            # keep track of all ancestors of the file
            ancestors = set([filelog.linkrev(last)])

            # iterate from latest to oldest revision
            for rev, flparentlinkrevs, copied in filerevgen(filelog, last):
                if not follow:
                    if rev > maxrev:
                        continue
                else:
                    # Note that last might not be the first interesting
                    # rev to us:
                    # if the file has been changed after maxrev, we'll
                    # have linkrev(last) > maxrev, and we still need
                    # to explore the file graph
                    if rev not in ancestors:
                        continue
                    # XXX insert 1327 fix here
                    if flparentlinkrevs:
                        ancestors.update(flparentlinkrevs)

                fncache.setdefault(rev, []).append(file_)
                wanted.add(rev)
                if copied:
                    copies.append(copied)
    if slowpath:
        # We have to read the changelog to match filenames against
        # changed files

        if follow:
            raise util.Abort(_('can only follow copies/renames for explicit '
                               'filenames'))

        # The slow path checks files modified in every changeset.
        for i in sorted(revs):
            ctx = change(i)
            matches = filter(match, ctx.files())
            if matches:
                fncache[i] = matches
                wanted.add(i)

    class followfilter(object):
        def __init__(self, onlyfirst=False):
            self.startrev = nullrev
            self.roots = set()
            self.onlyfirst = onlyfirst

        def match(self, rev):
            def realparents(rev):
                if self.onlyfirst:
                    return repo.changelog.parentrevs(rev)[0:1]
                else:
                    return filter(lambda x: x != nullrev,
                                  repo.changelog.parentrevs(rev))

            if self.startrev == nullrev:
                self.startrev = rev
                return True

            if rev > self.startrev:
                # forward: all descendants
                if not self.roots:
                    self.roots.add(self.startrev)
                for parent in realparents(rev):
                    if parent in self.roots:
                        self.roots.add(rev)
                        return True
            else:
                # backwards: all parents
                if not self.roots:
                    self.roots.update(realparents(self.startrev))
                if rev in self.roots:
                    self.roots.remove(rev)
                    self.roots.update(realparents(rev))
                    return True

            return False

    # it might be worthwhile to do this in the iterator if the rev range
    # is descending and the prune args are all within that range
    for rev in opts.get('prune', ()):
        rev = repo.changelog.rev(repo.lookup(rev))
        ff = followfilter()
        stop = min(revs[0], revs[-1])
        for x in xrange(rev, stop - 1, -1):
            if ff.match(x):
                wanted.discard(x)

    # Now that wanted is correctly initialized, we can iterate over the
    # revision range, yielding only revisions in wanted.
    def iterate():
        if follow and not match.files():
            ff = followfilter(onlyfirst=opts.get('follow_first'))
            def want(rev):
                return ff.match(rev) and rev in wanted
        else:
            def want(rev):
                return rev in wanted

        for i, window in increasing_windows(0, len(revs)):
            nrevs = [rev for rev in revs[i:i + window] if want(rev)]
            for rev in sorted(nrevs):
                fns = fncache.get(rev)
                ctx = change(rev)
                if not fns:
                    def fns_generator():
                        for f in ctx.files():
                            if match(f):
                                yield f
                    fns = fns_generator()
                prepare(ctx, fns)
            for rev in nrevs:
                yield change(rev)
    return iterate()

def add(ui, repo, match, dryrun, listsubrepos, prefix):
    join = lambda f: os.path.join(prefix, f)
    bad = []
    oldbad = match.bad
    match.bad = lambda x, y: bad.append(x) or oldbad(x, y)
    names = []
    wctx = repo[None]
    for f in repo.walk(match):
        exact = match.exact(f)
        if exact or f not in repo.dirstate:
            names.append(f)
            if ui.verbose or not exact:
                ui.status(_('adding %s\n') % match.rel(join(f)))

    if listsubrepos:
        for subpath in wctx.substate:
            sub = wctx.sub(subpath)
            try:
                submatch = matchmod.narrowmatcher(subpath, match)
                bad.extend(sub.add(ui, submatch, dryrun, prefix))
            except error.LookupError:
                ui.status(_("skipping missing subrepository: %s\n")
                               % join(subpath))

    if not dryrun:
        rejected = wctx.add(names, prefix)
        bad.extend(f for f in rejected if f in match.files())
    return bad

def commit(ui, repo, commitfunc, pats, opts):
    '''commit the specified files or all outstanding changes'''
    date = opts.get('date')
    if date:
        opts['date'] = util.parsedate(date)
    message = logmessage(opts)

    # extract addremove carefully -- this function can be called from a command
    # that doesn't support addremove
    if opts.get('addremove'):
        addremove(repo, pats, opts)

    return commitfunc(ui, repo, message, match(repo, pats, opts), opts)

def commiteditor(repo, ctx, subs):
    if ctx.description():
        return ctx.description()
    return commitforceeditor(repo, ctx, subs)

def commitforceeditor(repo, ctx, subs):
    edittext = []
    modified, added, removed = ctx.modified(), ctx.added(), ctx.removed()
    if ctx.description():
        edittext.append(ctx.description())
    edittext.append("")
    edittext.append("") # Empty line between message and comments.
    edittext.append(_("HG: Enter commit message."
                      "  Lines beginning with 'HG:' are removed."))
    edittext.append(_("HG: Leave message empty to abort commit."))
    edittext.append("HG: --")
    edittext.append(_("HG: user: %s") % ctx.user())
    if ctx.p2():
        edittext.append(_("HG: branch merge"))
    if ctx.branch():
        edittext.append(_("HG: branch '%s'") % ctx.branch())
    edittext.extend([_("HG: subrepo %s") % s for s in subs])
    edittext.extend([_("HG: added %s") % f for f in added])
    edittext.extend([_("HG: changed %s") % f for f in modified])
    edittext.extend([_("HG: removed %s") % f for f in removed])
    if not added and not modified and not removed:
        edittext.append(_("HG: no files changed"))
    edittext.append("")
    # run editor in the repository root
    olddir = os.getcwd()
    os.chdir(repo.root)
    text = repo.ui.edit("\n".join(edittext), ctx.user())
    text = re.sub("(?m)^HG:.*(\n|$)", "", text)
    os.chdir(olddir)

    if not text.strip():
        raise util.Abort(_("empty commit message"))

    return text
