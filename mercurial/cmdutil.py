# cmdutil.py - help for command processing in mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import hex, nullid, nullrev, short
from i18n import _
import os, sys, errno, re, tempfile, cStringIO, shutil
import util, scmutil, templater, patch, error, templatekw, revlog, copies
import match as matchmod
import context, repair, graphmod, revset, phases, obsolete, pathutil
import changelog
import bookmarks
import encoding
import crecord as crecordmod
import lock as lockmod

def parsealiases(cmd):
    return cmd.lstrip("^").split("|")

def setupwrapcolorwrite(ui):
    # wrap ui.write so diff output can be labeled/colorized
    def wrapwrite(orig, *args, **kw):
        label = kw.pop('label', '')
        for chunk, l in patch.difflabel(lambda: args):
            orig(chunk, label=label + l)

    oldwrite = ui.write
    def wrap(*args, **kwargs):
        return wrapwrite(oldwrite, *args, **kwargs)
    setattr(ui, 'write', wrap)
    return oldwrite

def filterchunks(ui, originalhunks, usecurses, testfile):
    if usecurses:
        if testfile:
            recordfn = crecordmod.testdecorator(testfile,
                                                crecordmod.testchunkselector)
        else:
            recordfn = crecordmod.chunkselector

        return crecordmod.filterpatch(ui, originalhunks, recordfn)

    else:
        return patch.filterpatch(ui, originalhunks)

def recordfilter(ui, originalhunks):
    usecurses =  ui.configbool('experimental', 'crecord', False)
    testfile = ui.config('experimental', 'crecordtest', None)
    oldwrite = setupwrapcolorwrite(ui)
    try:
        newchunks = filterchunks(ui, originalhunks, usecurses, testfile)
    finally:
        ui.write = oldwrite
    return newchunks

def dorecord(ui, repo, commitfunc, cmdsuggest, backupall,
            filterfn, *pats, **opts):
    import merge as mergemod
    hunkclasses = (crecordmod.uihunk, patch.recordhunk)
    ishunk = lambda x: isinstance(x, hunkclasses)

    if not ui.interactive():
        raise util.Abort(_('running non-interactively, use %s instead') %
                         cmdsuggest)

    # make sure username is set before going interactive
    if not opts.get('user'):
        ui.username() # raise exception, username not provided

    def recordfunc(ui, repo, message, match, opts):
        """This is generic record driver.

        Its job is to interactively filter local changes, and
        accordingly prepare working directory into a state in which the
        job can be delegated to a non-interactive commit command such as
        'commit' or 'qrefresh'.

        After the actual job is done by non-interactive command, the
        working directory is restored to its original state.

        In the end we'll record interesting changes, and everything else
        will be left in place, so the user can continue working.
        """

        checkunfinished(repo, commit=True)
        merge = len(repo[None].parents()) > 1
        if merge:
            raise util.Abort(_('cannot partially commit a merge '
                               '(use "hg commit" instead)'))

        status = repo.status(match=match)
        diffopts = patch.difffeatureopts(ui, opts=opts, whitespace=True)
        diffopts.nodates = True
        diffopts.git = True
        originaldiff =  patch.diff(repo, changes=status, opts=diffopts)
        originalchunks = patch.parsepatch(originaldiff)

        # 1. filter patch, so we have intending-to apply subset of it
        try:
            chunks = filterfn(ui, originalchunks)
        except patch.PatchError, err:
            raise util.Abort(_('error parsing patch: %s') % err)

        # We need to keep a backup of files that have been newly added and
        # modified during the recording process because there is a previous
        # version without the edit in the workdir
        newlyaddedandmodifiedfiles = set()
        for chunk in chunks:
            if ishunk(chunk) and chunk.header.isnewfile() and chunk not in \
                originalchunks:
                newlyaddedandmodifiedfiles.add(chunk.header.filename())
        contenders = set()
        for h in chunks:
            try:
                contenders.update(set(h.files()))
            except AttributeError:
                pass

        changed = status.modified + status.added + status.removed
        newfiles = [f for f in changed if f in contenders]
        if not newfiles:
            ui.status(_('no changes to record\n'))
            return 0

        modified = set(status.modified)

        # 2. backup changed files, so we can restore them in the end

        if backupall:
            tobackup = changed
        else:
            tobackup = [f for f in newfiles if f in modified or f in \
                    newlyaddedandmodifiedfiles]
        backups = {}
        if tobackup:
            backupdir = repo.join('record-backups')
            try:
                os.mkdir(backupdir)
            except OSError, err:
                if err.errno != errno.EEXIST:
                    raise
        try:
            # backup continues
            for f in tobackup:
                fd, tmpname = tempfile.mkstemp(prefix=f.replace('/', '_')+'.',
                                               dir=backupdir)
                os.close(fd)
                ui.debug('backup %r as %r\n' % (f, tmpname))
                util.copyfile(repo.wjoin(f), tmpname)
                shutil.copystat(repo.wjoin(f), tmpname)
                backups[f] = tmpname

            fp = cStringIO.StringIO()
            for c in chunks:
                fname = c.filename()
                if fname in backups:
                    c.write(fp)
            dopatch = fp.tell()
            fp.seek(0)

            [os.unlink(repo.wjoin(c)) for c in newlyaddedandmodifiedfiles]
            # 3a. apply filtered patch to clean repo  (clean)
            if backups:
                # Equivalent to hg.revert
                choices = lambda key: key in backups
                mergemod.update(repo, repo.dirstate.p1(),
                        False, True, choices)

            # 3b. (apply)
            if dopatch:
                try:
                    ui.debug('applying patch\n')
                    ui.debug(fp.getvalue())
                    patch.internalpatch(ui, repo, fp, 1, eolmode=None)
                except patch.PatchError, err:
                    raise util.Abort(str(err))
            del fp

            # 4. We prepared working directory according to filtered
            #    patch. Now is the time to delegate the job to
            #    commit/qrefresh or the like!

            # Make all of the pathnames absolute.
            newfiles = [repo.wjoin(nf) for nf in newfiles]
            return commitfunc(ui, repo, *newfiles, **opts)
        finally:
            # 5. finally restore backed-up files
            try:
                for realname, tmpname in backups.iteritems():
                    ui.debug('restoring %r to %r\n' % (tmpname, realname))
                    util.copyfile(tmpname, repo.wjoin(realname))
                    # Our calls to copystat() here and above are a
                    # hack to trick any editors that have f open that
                    # we haven't modified them.
                    #
                    # Also note that this racy as an editor could
                    # notice the file's mtime before we've finished
                    # writing it.
                    shutil.copystat(tmpname, repo.wjoin(realname))
                    os.unlink(tmpname)
                if tobackup:
                    os.rmdir(backupdir)
            except OSError:
                pass

    return commit(ui, repo, recordfunc, pats, opts)

def findpossible(cmd, table, strict=False):
    """
    Return cmd -> (aliases, command table entry)
    for each matching command.
    Return debug commands (or their aliases) only if no normal command matches.
    """
    choice = {}
    debugchoice = {}

    if cmd in table:
        # short-circuit exact matches, "log" alias beats "^log|history"
        keys = [cmd]
    else:
        keys = table.keys()

    allcmds = []
    for e in keys:
        aliases = parsealiases(e)
        allcmds.extend(aliases)
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

    return choice, allcmds

def findcmd(cmd, table, strict=True):
    """Return (aliases, command table entry) for command string."""
    choice, allcmds = findpossible(cmd, table, strict)

    if cmd in choice:
        return choice[cmd]

    if len(choice) > 1:
        clist = choice.keys()
        clist.sort()
        raise error.AmbiguousCommand(cmd, clist)

    if choice:
        return choice.values()[0]

    raise error.UnknownCommand(cmd, allcmds)

def findrepo(p):
    while not os.path.isdir(os.path.join(p, ".hg")):
        oldp, p = p, os.path.dirname(p)
        if p == oldp:
            return None

    return p

def bailifchanged(repo, merge=True):
    if merge and repo.dirstate.p2() != nullid:
        raise util.Abort(_('outstanding uncommitted merge'))
    modified, added, removed, deleted = repo.status()[:4]
    if modified or added or removed or deleted:
        raise util.Abort(_('uncommitted changes'))
    ctx = repo[None]
    for s in sorted(ctx.substate):
        ctx.sub(s).bailifchanged()

def logmessage(ui, opts):
    """ get the log message according to -m and -l option """
    message = opts.get('message')
    logfile = opts.get('logfile')

    if message and logfile:
        raise util.Abort(_('options --message and --logfile are mutually '
                           'exclusive'))
    if not message and logfile:
        try:
            if logfile == '-':
                message = ui.fin.read()
            else:
                message = '\n'.join(util.readfile(logfile).splitlines())
        except IOError, inst:
            raise util.Abort(_("can't read commit message '%s': %s") %
                             (logfile, inst.strerror))
    return message

def mergeeditform(ctxorbool, baseformname):
    """return appropriate editform name (referencing a committemplate)

    'ctxorbool' is either a ctx to be committed, or a bool indicating whether
    merging is committed.

    This returns baseformname with '.merge' appended if it is a merge,
    otherwise '.normal' is appended.
    """
    if isinstance(ctxorbool, bool):
        if ctxorbool:
            return baseformname + ".merge"
    elif 1 < len(ctxorbool.parents()):
        return baseformname + ".merge"

    return baseformname + ".normal"

def getcommiteditor(edit=False, finishdesc=None, extramsg=None,
                    editform='', **opts):
    """get appropriate commit message editor according to '--edit' option

    'finishdesc' is a function to be called with edited commit message
    (= 'description' of the new changeset) just after editing, but
    before checking empty-ness. It should return actual text to be
    stored into history. This allows to change description before
    storing.

    'extramsg' is a extra message to be shown in the editor instead of
    'Leave message empty to abort commit' line. 'HG: ' prefix and EOL
    is automatically added.

    'editform' is a dot-separated list of names, to distinguish
    the purpose of commit text editing.

    'getcommiteditor' returns 'commitforceeditor' regardless of
    'edit', if one of 'finishdesc' or 'extramsg' is specified, because
    they are specific for usage in MQ.
    """
    if edit or finishdesc or extramsg:
        return lambda r, c, s: commitforceeditor(r, c, s,
                                                 finishdesc=finishdesc,
                                                 extramsg=extramsg,
                                                 editform=editform)
    elif editform:
        return lambda r, c, s: commiteditor(r, c, s, editform=editform)
    else:
        return commiteditor

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

def makefilename(repo, pat, node, desc=None,
                  total=None, seqno=None, revwidth=None, pathname=None):
    node_expander = {
        'H': lambda: hex(node),
        'R': lambda: str(repo.changelog.rev(node)),
        'h': lambda: short(node),
        'm': lambda: re.sub('[^\w]', '_', str(desc))
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

def makefileobj(repo, pat, node=None, desc=None, total=None,
                seqno=None, revwidth=None, mode='wb', modemap=None,
                pathname=None):

    writable = mode not in ('r', 'rb')

    if not pat or pat == '-':
        if writable:
            fp = repo.ui.fout
        else:
            fp = repo.ui.fin
        if util.safehasattr(fp, 'fileno'):
            return os.fdopen(os.dup(fp.fileno()), mode)
        else:
            # if this fp can't be duped properly, return
            # a dummy object that can be closed
            class wrappedfileobj(object):
                noop = lambda x: None
                def __init__(self, f):
                    self.f = f
                def __getattr__(self, attr):
                    if attr == 'close':
                        return self.noop
                    else:
                        return getattr(self.f, attr)

            return wrappedfileobj(fp)
    if util.safehasattr(pat, 'write') and writable:
        return pat
    if util.safehasattr(pat, 'read') and 'r' in mode:
        return pat
    fn = makefilename(repo, pat, node, desc, total, seqno, revwidth, pathname)
    if modemap is not None:
        mode = modemap.get(fn, mode)
        if mode == 'wb':
            modemap[fn] = 'ab'
    return open(fn, mode)

def openrevlog(repo, cmd, file_, opts):
    """opens the changelog, manifest, a filelog or a given revlog"""
    cl = opts['changelog']
    mf = opts['manifest']
    dir = opts['dir']
    msg = None
    if cl and mf:
        msg = _('cannot specify --changelog and --manifest at the same time')
    elif cl and dir:
        msg = _('cannot specify --changelog and --dir at the same time')
    elif cl or mf:
        if file_:
            msg = _('cannot specify filename with --changelog or --manifest')
        elif not repo:
            msg = _('cannot specify --changelog or --manifest or --dir '
                    'without a repository')
    if msg:
        raise util.Abort(msg)

    r = None
    if repo:
        if cl:
            r = repo.unfiltered().changelog
        elif dir:
            if 'treemanifest' not in repo.requirements:
                raise util.Abort(_("--dir can only be used on repos with "
                                   "treemanifest enabled"))
            dirlog = repo.dirlog(file_)
            if len(dirlog):
                r = dirlog
        elif mf:
            r = repo.manifest
        elif file_:
            filelog = repo.file(file_)
            if len(filelog):
                r = filelog
    if not r:
        if not file_:
            raise error.CommandError(cmd, _('invalid arguments'))
        if not os.path.isfile(file_):
            raise util.Abort(_("revlog '%s' not found") % file_)
        r = revlog.revlog(scmutil.opener(os.getcwd(), audit=False),
                          file_[:-2] + ".i")
    return r

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
        if after:
            badstates = '?'
        else:
            badstates = '?r'
        m = scmutil.match(repo[None], [pat], opts, globbed=True)
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
        abstarget = pathutil.canonpath(repo.root, cwd, otarget)
        if '/' in abstarget:
            # We cannot normalize abstarget itself, this would prevent
            # case only renames, like a => A.
            abspath, absname = abstarget.rsplit('/', 1)
            abstarget = repo.dirstate.normalize(abspath) + '/' + absname
        reltarget = repo.pathto(abstarget, cwd)
        target = repo.wjoin(abstarget)
        src = repo.wjoin(abssrc)
        state = repo.dirstate[abstarget]

        scmutil.checkportable(ui, abstarget)

        # check for collisions
        prevsrc = targets.get(abstarget)
        if prevsrc is not None:
            ui.warn(_('%s: not overwriting - %s collides with %s\n') %
                    (reltarget, repo.pathto(abssrc, cwd),
                     repo.pathto(prevsrc, cwd)))
            return

        # check for overwrites
        exists = os.path.lexists(target)
        samefile = False
        if exists and abssrc != abstarget:
            if (repo.dirstate.normalize(abssrc) ==
                repo.dirstate.normalize(abstarget)):
                if not rename:
                    ui.warn(_("%s: can't copy - same file\n") % reltarget)
                    return
                exists = False
                samefile = True

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
                if samefile:
                    tmp = target + "~hgrename"
                    os.rename(src, tmp)
                    os.rename(tmp, target)
                else:
                    util.copyfile(src, target)
                srcexists = True
            except IOError, inst:
                if inst.errno == errno.ENOENT:
                    ui.warn(_('%s: deleted in working directory\n') % relsrc)
                    srcexists = False
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
        scmutil.dirstatecopy(ui, repo, wctx, abssrc, abstarget,
                             dryrun=dryrun, cwd=cwd)
        if rename and not dryrun:
            if not after and srcexists and not samefile:
                util.unlinkpath(repo.wjoin(abssrc))
            wctx.forget([abssrc])

    # pat: ossep
    # dest ossep
    # srcs: list of (hgsep, hgsep, ossep, bool)
    # return: function that takes hgsep and returns ossep
    def targetpathfn(pat, dest, srcs):
        if os.path.isdir(pat):
            abspfx = pathutil.canonpath(repo.root, cwd, pat)
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
            abspfx = pathutil.canonpath(repo.root, cwd, pat)
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

    pats = scmutil.expandpats(pats)
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

    def writepid(pid):
        if opts['pid_file']:
            if appendpid:
                mode = 'a'
            else:
                mode = 'w'
            fp = open(opts['pid_file'], mode)
            fp.write(str(pid) + '\n')
            fp.close()

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
            writepid(pid)
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

    if not opts['daemon']:
        writepid(os.getpid())

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

        nullfd = os.open(os.devnull, os.O_RDWR)
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

def tryimportone(ui, repo, hunk, parents, opts, msgs, updatefunc):
    """Utility function used by commands.import to import a single patch

    This function is explicitly defined here to help the evolve extension to
    wrap this part of the import logic.

    The API is currently a bit ugly because it a simple code translation from
    the import command. Feel free to make it better.

    :hunk: a patch (as a binary string)
    :parents: nodes that will be parent of the created commit
    :opts: the full dict of option passed to the import command
    :msgs: list to save commit message to.
           (used in case we need to save it when failing)
    :updatefunc: a function that update a repo to a given node
                 updatefunc(<repo>, <node>)
    """
    tmpname, message, user, date, branch, nodeid, p1, p2 = \
        patch.extract(ui, hunk)

    update = not opts.get('bypass')
    strip = opts["strip"]
    prefix = opts["prefix"]
    sim = float(opts.get('similarity') or 0)
    if not tmpname:
        return (None, None, False)
    msg = _('applied to working directory')

    rejects = False
    dsguard = None

    try:
        cmdline_message = logmessage(ui, opts)
        if cmdline_message:
            # pickup the cmdline msg
            message = cmdline_message
        elif message:
            # pickup the patch msg
            message = message.strip()
        else:
            # launch the editor
            message = None
        ui.debug('message:\n%s\n' % message)

        if len(parents) == 1:
            parents.append(repo[nullid])
        if opts.get('exact'):
            if not nodeid or not p1:
                raise util.Abort(_('not a Mercurial patch'))
            p1 = repo[p1]
            p2 = repo[p2 or nullid]
        elif p2:
            try:
                p1 = repo[p1]
                p2 = repo[p2]
                # Without any options, consider p2 only if the
                # patch is being applied on top of the recorded
                # first parent.
                if p1 != parents[0]:
                    p1 = parents[0]
                    p2 = repo[nullid]
            except error.RepoError:
                p1, p2 = parents
            if p2.node() == nullid:
                ui.warn(_("warning: import the patch as a normal revision\n"
                          "(use --exact to import the patch as a merge)\n"))
        else:
            p1, p2 = parents

        n = None
        if update:
            dsguard = dirstateguard(repo, 'tryimportone')
            if p1 != parents[0]:
                updatefunc(repo, p1.node())
            if p2 != parents[1]:
                repo.setparents(p1.node(), p2.node())

            if opts.get('exact') or opts.get('import_branch'):
                repo.dirstate.setbranch(branch or 'default')

            partial = opts.get('partial', False)
            files = set()
            try:
                patch.patch(ui, repo, tmpname, strip=strip, prefix=prefix,
                            files=files, eolmode=None, similarity=sim / 100.0)
            except patch.PatchError, e:
                if not partial:
                    raise util.Abort(str(e))
                if partial:
                    rejects = True

            files = list(files)
            if opts.get('no_commit'):
                if message:
                    msgs.append(message)
            else:
                if opts.get('exact') or p2:
                    # If you got here, you either use --force and know what
                    # you are doing or used --exact or a merge patch while
                    # being updated to its first parent.
                    m = None
                else:
                    m = scmutil.matchfiles(repo, files or [])
                editform = mergeeditform(repo[None], 'import.normal')
                if opts.get('exact'):
                    editor = None
                else:
                    editor = getcommiteditor(editform=editform, **opts)
                allowemptyback = repo.ui.backupconfig('ui', 'allowemptycommit')
                try:
                    if partial:
                        repo.ui.setconfig('ui', 'allowemptycommit', True)
                    n = repo.commit(message, opts.get('user') or user,
                                    opts.get('date') or date, match=m,
                                    editor=editor)
                finally:
                    repo.ui.restoreconfig(allowemptyback)
            dsguard.close()
        else:
            if opts.get('exact') or opts.get('import_branch'):
                branch = branch or 'default'
            else:
                branch = p1.branch()
            store = patch.filestore()
            try:
                files = set()
                try:
                    patch.patchrepo(ui, repo, p1, store, tmpname, strip, prefix,
                                    files, eolmode=None)
                except patch.PatchError, e:
                    raise util.Abort(str(e))
                if opts.get('exact'):
                    editor = None
                else:
                    editor = getcommiteditor(editform='import.bypass')
                memctx = context.makememctx(repo, (p1.node(), p2.node()),
                                            message,
                                            opts.get('user') or user,
                                            opts.get('date') or date,
                                            branch, files, store,
                                            editor=editor)
                n = memctx.commit()
            finally:
                store.close()
        if opts.get('exact') and opts.get('no_commit'):
            # --exact with --no-commit is still useful in that it does merge
            # and branch bits
            ui.warn(_("warning: can't check exact import with --no-commit\n"))
        elif opts.get('exact') and hex(n) != nodeid:
            raise util.Abort(_('patch is damaged or loses information'))
        if n:
            # i18n: refers to a short changeset id
            msg = _('created %s') % short(n)
        return (msg, n, rejects)
    finally:
        lockmod.release(dsguard)
        os.unlink(tmpname)

def export(repo, revs, template='hg-%h.patch', fp=None, switch_parent=False,
           opts=None):
    '''export changesets as hg patches.'''

    total = len(revs)
    revwidth = max([len(str(rev)) for rev in revs])
    filemode = {}

    def single(rev, seqno, fp):
        ctx = repo[rev]
        node = ctx.node()
        parents = [p.node() for p in ctx.parents() if p]
        branch = ctx.branch()
        if switch_parent:
            parents.reverse()

        if parents:
            prev = parents[0]
        else:
            prev = nullid

        shouldclose = False
        if not fp and len(template) > 0:
            desc_lines = ctx.description().rstrip().split('\n')
            desc = desc_lines[0]    #Commit always has a first line.
            fp = makefileobj(repo, template, node, desc=desc, total=total,
                             seqno=seqno, revwidth=revwidth, mode='wb',
                             modemap=filemode)
            if fp != template:
                shouldclose = True
        if fp and fp != sys.stdout and util.safehasattr(fp, 'name'):
            repo.ui.note("%s\n" % fp.name)

        if not fp:
            write = repo.ui.write
        else:
            def write(s, **kw):
                fp.write(s)

        write("# HG changeset patch\n")
        write("# User %s\n" % ctx.user())
        write("# Date %d %d\n" % ctx.date())
        write("#      %s\n" % util.datestr(ctx.date()))
        if branch and branch != 'default':
            write("# Branch %s\n" % branch)
        write("# Node ID %s\n" % hex(node))
        write("# Parent  %s\n" % hex(prev))
        if len(parents) > 1:
            write("# Parent  %s\n" % hex(parents[1]))
        write(ctx.description().rstrip())
        write("\n\n")

        for chunk, label in patch.diffui(repo, prev, node, opts=opts):
            write(chunk, label=label)

        if shouldclose:
            fp.close()

    for seqno, rev in enumerate(revs):
        single(rev, seqno + 1, fp)

def diffordiffstat(ui, repo, diffopts, node1, node2, match,
                   changes=None, stat=False, fp=None, prefix='',
                   root='', listsubrepos=False):
    '''show diff or diffstat.'''
    if fp is None:
        write = ui.write
    else:
        def write(s, **kw):
            fp.write(s)

    if root:
        relroot = pathutil.canonpath(repo.root, repo.getcwd(), root)
    else:
        relroot = ''
    if relroot != '':
        # XXX relative roots currently don't work if the root is within a
        # subrepo
        uirelroot = match.uipath(relroot)
        relroot += '/'
        for matchroot in match.files():
            if not matchroot.startswith(relroot):
                ui.warn(_('warning: %s not inside relative root %s\n') % (
                    match.uipath(matchroot), uirelroot))

    if stat:
        diffopts = diffopts.copy(context=0)
        width = 80
        if not ui.plain():
            width = ui.termwidth()
        chunks = patch.diff(repo, node1, node2, match, changes, diffopts,
                            prefix=prefix, relroot=relroot)
        for chunk, label in patch.diffstatui(util.iterlines(chunks),
                                             width=width,
                                             git=diffopts.git):
            write(chunk, label=label)
    else:
        for chunk, label in patch.diffui(repo, node1, node2, match,
                                         changes, diffopts, prefix=prefix,
                                         relroot=relroot):
            write(chunk, label=label)

    if listsubrepos:
        ctx1 = repo[node1]
        ctx2 = repo[node2]
        for subpath, sub in scmutil.itersubrepos(ctx1, ctx2):
            tempnode2 = node2
            try:
                if node2 is not None:
                    tempnode2 = ctx2.substate[subpath][1]
            except KeyError:
                # A subrepo that existed in node1 was deleted between node1 and
                # node2 (inclusive). Thus, ctx2's substate won't contain that
                # subpath. The best we can do is to ignore it.
                tempnode2 = None
            submatch = matchmod.narrowmatcher(subpath, match)
            sub.diff(ui, diffopts, tempnode2, submatch, changes=changes,
                     stat=stat, fp=fp, prefix=prefix)

class changeset_printer(object):
    '''show changeset information when templating not requested.'''

    def __init__(self, ui, repo, matchfn, diffopts, buffered):
        self.ui = ui
        self.repo = repo
        self.buffered = buffered
        self.matchfn = matchfn
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
        if self.ui.debugflag:
            hexfunc = hex
        else:
            hexfunc = short
        if rev is None:
            pctx = ctx.p1()
            revnode = (pctx.rev(), hexfunc(pctx.node()) + '+')
        else:
            revnode = (rev, hexfunc(changenode))

        if self.ui.quiet:
            self.ui.write("%d:%s\n" % revnode, label='log.node')
            return

        date = util.datestr(ctx.date())

        # i18n: column positioning for "hg log"
        self.ui.write(_("changeset:   %d:%s\n") % revnode,
                      label='log.changeset changeset.%s' % ctx.phasestr())

        # branches are shown first before any other names due to backwards
        # compatibility
        branch = ctx.branch()
        # don't show the default branch name
        if branch != 'default':
            # i18n: column positioning for "hg log"
            self.ui.write(_("branch:      %s\n") % branch,
                          label='log.branch')

        for name, ns in self.repo.names.iteritems():
            # branches has special logic already handled above, so here we just
            # skip it
            if name == 'branches':
                continue
            # we will use the templatename as the color name since those two
            # should be the same
            for name in ns.names(self.repo, changenode):
                self.ui.write(ns.logfmt % name,
                              label='log.%s' % ns.colorname)
        if self.ui.debugflag:
            # i18n: column positioning for "hg log"
            self.ui.write(_("phase:       %s\n") % ctx.phasestr(),
                          label='log.phase')
        for pctx in self._meaningful_parentrevs(ctx):
            label = 'log.parent changeset.%s' % pctx.phasestr()
            # i18n: column positioning for "hg log"
            self.ui.write(_("parent:      %d:%s\n")
                          % (pctx.rev(), hexfunc(pctx.node())),
                          label=label)

        if self.ui.debugflag and rev is not None:
            mnode = ctx.manifestnode()
            # i18n: column positioning for "hg log"
            self.ui.write(_("manifest:    %d:%s\n") %
                          (self.repo.manifest.rev(mnode), hex(mnode)),
                          label='ui.debug log.manifest')
        # i18n: column positioning for "hg log"
        self.ui.write(_("user:        %s\n") % ctx.user(),
                      label='log.user')
        # i18n: column positioning for "hg log"
        self.ui.write(_("date:        %s\n") % date,
                      label='log.date')

        if self.ui.debugflag:
            files = ctx.p1().status(ctx)[:3]
            for key, value in zip([# i18n: column positioning for "hg log"
                                   _("files:"),
                                   # i18n: column positioning for "hg log"
                                   _("files+:"),
                                   # i18n: column positioning for "hg log"
                                   _("files-:")], files):
                if value:
                    self.ui.write("%-12s %s\n" % (key, " ".join(value)),
                                  label='ui.debug log.files')
        elif ctx.files() and self.ui.verbose:
            # i18n: column positioning for "hg log"
            self.ui.write(_("files:       %s\n") % " ".join(ctx.files()),
                          label='ui.note log.files')
        if copies and self.ui.verbose:
            copies = ['%s (%s)' % c for c in copies]
            # i18n: column positioning for "hg log"
            self.ui.write(_("copies:      %s\n") % ' '.join(copies),
                          label='ui.note log.copies')

        extra = ctx.extra()
        if extra and self.ui.debugflag:
            for key, value in sorted(extra.items()):
                # i18n: column positioning for "hg log"
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
                # i18n: column positioning for "hg log"
                self.ui.write(_("summary:     %s\n") %
                              description.splitlines()[0],
                              label='log.summary')
        self.ui.write("\n")

        self.showpatch(changenode, matchfn)

    def showpatch(self, node, matchfn):
        if not matchfn:
            matchfn = self.matchfn
        if matchfn:
            stat = self.diffopts.get('stat')
            diff = self.diffopts.get('patch')
            diffopts = patch.diffallopts(self.ui, self.diffopts)
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

    def _meaningful_parentrevs(self, ctx):
        """Return list of meaningful (or all if debug) parentrevs for rev.

        For merges (two non-nullrev revisions) both parents are meaningful.
        Otherwise the first parent revision is considered meaningful if it
        is not the preceding revision.
        """
        parents = ctx.parents()
        if len(parents) > 1:
            return parents
        if self.ui.debugflag:
            return [parents[0], self.repo['null']]
        if parents[0].rev() >= scmutil.intrev(self.repo, ctx.rev()) - 1:
            return []
        return parents

class jsonchangeset(changeset_printer):
    '''format changeset information.'''

    def __init__(self, ui, repo, matchfn, diffopts, buffered):
        changeset_printer.__init__(self, ui, repo, matchfn, diffopts, buffered)
        self.cache = {}
        self._first = True

    def close(self):
        if not self._first:
            self.ui.write("\n]\n")
        else:
            self.ui.write("[]\n")

    def _show(self, ctx, copies, matchfn, props):
        '''show a single changeset or file revision'''
        rev = ctx.rev()
        if rev is None:
            jrev = jnode = 'null'
        else:
            jrev = str(rev)
            jnode = '"%s"' % hex(ctx.node())
        j = encoding.jsonescape

        if self._first:
            self.ui.write("[\n {")
            self._first = False
        else:
            self.ui.write(",\n {")

        if self.ui.quiet:
            self.ui.write('\n  "rev": %s' % jrev)
            self.ui.write(',\n  "node": %s' % jnode)
            self.ui.write('\n }')
            return

        self.ui.write('\n  "rev": %s' % jrev)
        self.ui.write(',\n  "node": %s' % jnode)
        self.ui.write(',\n  "branch": "%s"' % j(ctx.branch()))
        self.ui.write(',\n  "phase": "%s"' % ctx.phasestr())
        self.ui.write(',\n  "user": "%s"' % j(ctx.user()))
        self.ui.write(',\n  "date": [%d, %d]' % ctx.date())
        self.ui.write(',\n  "desc": "%s"' % j(ctx.description()))

        self.ui.write(',\n  "bookmarks": [%s]' %
                      ", ".join('"%s"' % j(b) for b in ctx.bookmarks()))
        self.ui.write(',\n  "tags": [%s]' %
                      ", ".join('"%s"' % j(t) for t in ctx.tags()))
        self.ui.write(',\n  "parents": [%s]' %
                      ", ".join('"%s"' % c.hex() for c in ctx.parents()))

        if self.ui.debugflag:
            if rev is None:
                jmanifestnode = 'null'
            else:
                jmanifestnode = '"%s"' % hex(ctx.manifestnode())
            self.ui.write(',\n  "manifest": %s' % jmanifestnode)

            self.ui.write(',\n  "extra": {%s}' %
                          ", ".join('"%s": "%s"' % (j(k), j(v))
                                    for k, v in ctx.extra().items()))

            files = ctx.p1().status(ctx)
            self.ui.write(',\n  "modified": [%s]' %
                          ", ".join('"%s"' % j(f) for f in files[0]))
            self.ui.write(',\n  "added": [%s]' %
                          ", ".join('"%s"' % j(f) for f in files[1]))
            self.ui.write(',\n  "removed": [%s]' %
                          ", ".join('"%s"' % j(f) for f in files[2]))

        elif self.ui.verbose:
            self.ui.write(',\n  "files": [%s]' %
                          ", ".join('"%s"' % j(f) for f in ctx.files()))

            if copies:
                self.ui.write(',\n  "copies": {%s}' %
                              ", ".join('"%s": "%s"' % (j(k), j(v))
                                                        for k, v in copies))

        matchfn = self.matchfn
        if matchfn:
            stat = self.diffopts.get('stat')
            diff = self.diffopts.get('patch')
            diffopts = patch.difffeatureopts(self.ui, self.diffopts, git=True)
            node, prev = ctx.node(), ctx.p1().node()
            if stat:
                self.ui.pushbuffer()
                diffordiffstat(self.ui, self.repo, diffopts, prev, node,
                               match=matchfn, stat=True)
                self.ui.write(',\n  "diffstat": "%s"' % j(self.ui.popbuffer()))
            if diff:
                self.ui.pushbuffer()
                diffordiffstat(self.ui, self.repo, diffopts, prev, node,
                               match=matchfn, stat=False)
                self.ui.write(',\n  "diff": "%s"' % j(self.ui.popbuffer()))

        self.ui.write("\n }")

class changeset_templater(changeset_printer):
    '''format changeset information.'''

    def __init__(self, ui, repo, matchfn, diffopts, tmpl, mapfile, buffered):
        changeset_printer.__init__(self, ui, repo, matchfn, diffopts, buffered)
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
        if tmpl:
            self.t.cache['changeset'] = tmpl

        self.cache = {}

    def _show(self, ctx, copies, matchfn, props):
        '''show a single changeset or file revision'''

        showlist = templatekw.showlist

        # showparents() behaviour depends on ui trace level which
        # causes unexpected behaviours at templating level and makes
        # it harder to extract it in a standalone function. Its
        # behaviour cannot be changed so leave it here for now.
        def showparents(**args):
            ctx = args['ctx']
            parents = [[('rev', p.rev()),
                        ('node', p.hex()),
                        ('phase', p.phasestr())]
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

def gettemplate(ui, tmpl, style):
    """
    Find the template matching the given template spec or style.
    """

    # ui settings
    if not tmpl and not style: # template are stronger than style
        tmpl = ui.config('ui', 'logtemplate')
        if tmpl:
            try:
                tmpl = templater.unquotestring(tmpl)
            except SyntaxError:
                pass
            return tmpl, None
        else:
            style = util.expandpath(ui.config('ui', 'style', ''))

    if not tmpl and style:
        mapfile = style
        if not os.path.split(mapfile)[0]:
            mapname = (templater.templatepath('map-cmdline.' + mapfile)
                       or templater.templatepath(mapfile))
            if mapname:
                mapfile = mapname
        return None, mapfile

    if not tmpl:
        return None, None

    # looks like a literal template?
    if '{' in tmpl:
        return tmpl, None

    # perhaps a stock style?
    if not os.path.split(tmpl)[0]:
        mapname = (templater.templatepath('map-cmdline.' + tmpl)
                   or templater.templatepath(tmpl))
        if mapname and os.path.isfile(mapname):
            return None, mapname

    # perhaps it's a reference to [templates]
    t = ui.config('templates', tmpl)
    if t:
        try:
            tmpl = templater.unquotestring(t)
        except SyntaxError:
            tmpl = t
        return tmpl, None

    if tmpl == 'list':
        ui.write(_("available styles: %s\n") % templater.stylelist())
        raise util.Abort(_("specify a template"))

    # perhaps it's a path to a map or a template
    if ('/' in tmpl or '\\' in tmpl) and os.path.isfile(tmpl):
        # is it a mapfile for a style?
        if os.path.basename(tmpl).startswith("map-"):
            return None, os.path.realpath(tmpl)
        tmpl = open(tmpl).read()
        return tmpl, None

    # constant string?
    return tmpl, None

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
    matchfn = None
    if opts.get('patch') or opts.get('stat'):
        matchfn = scmutil.matchall(repo)

    if opts.get('template') == 'json':
        return jsonchangeset(ui, repo, matchfn, opts, buffered)

    tmpl, mapfile = gettemplate(ui, opts.get('template'), opts.get('style'))

    if not tmpl and not mapfile:
        return changeset_printer(ui, repo, matchfn, opts, buffered)

    try:
        t = changeset_templater(ui, repo, matchfn, opts, tmpl, mapfile,
                                buffered)
    except SyntaxError, inst:
        raise util.Abort(inst.args[0])
    return t

def showmarker(ui, marker):
    """utility function to display obsolescence marker in a readable way

    To be used by debug function."""
    ui.write(hex(marker.precnode()))
    for repl in marker.succnodes():
        ui.write(' ')
        ui.write(hex(repl))
    ui.write(' %X ' % marker.flags())
    parents = marker.parentnodes()
    if parents is not None:
        ui.write('{%s} ' % ', '.join(hex(p) for p in parents))
    ui.write('(%s) ' % util.datestr(marker.date()))
    ui.write('{%s}' % (', '.join('%r: %r' % t for t in
                                 sorted(marker.metadata().items())
                                 if t[0] != 'date')))
    ui.write('\n')

def finddate(ui, repo, date):
    """Find the tipmost changeset that matches the given date spec"""

    df = util.matchdate(date)
    m = scmutil.matchall(repo)
    results = {}

    def prep(ctx, fns):
        d = ctx.date()
        if df(d[0]):
            results[ctx.rev()] = d

    for ctx in walkchangerevs(repo, m, {'rev': None}, prep):
        rev = ctx.rev()
        if rev in results:
            ui.status(_("found revision %s from %s\n") %
                      (rev, util.datestr(results[rev])))
            return str(rev)

    raise util.Abort(_("revision matching date not found"))

def increasingwindows(windowsize=8, sizelimit=512):
    while True:
        yield windowsize
        if windowsize < sizelimit:
            windowsize *= 2

class FileWalkError(Exception):
    pass

def walkfilerevs(repo, match, follow, revs, fncache):
    '''Walks the file history for the matched files.

    Returns the changeset revs that are involved in the file history.

    Throws FileWalkError if the file history can't be walked using
    filelogs alone.
    '''
    wanted = set()
    copies = []
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
        pctx = repo['.']
        for filename in match.files():
            if follow:
                if filename not in pctx:
                    raise util.Abort(_('cannot follow file not in parent '
                                       'revision: "%s"') % filename)
                yield filename, pctx[filename].filenode()
            else:
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
                raise FileWalkError("Cannot walk via filelog")
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

    return wanted

class _followfilter(object):
    def __init__(self, repo, onlyfirst=False):
        self.repo = repo
        self.startrev = nullrev
        self.roots = set()
        self.onlyfirst = onlyfirst

    def match(self, rev):
        def realparents(rev):
            if self.onlyfirst:
                return self.repo.changelog.parentrevs(rev)[0:1]
            else:
                return filter(lambda x: x != nullrev,
                              self.repo.changelog.parentrevs(rev))

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

    follow = opts.get('follow') or opts.get('follow_first')
    revs = _logrevs(repo, opts)
    if not revs:
        return []
    wanted = set()
    slowpath = match.anypats() or (match.files() and opts.get('removed'))
    fncache = {}
    change = repo.changectx

    # First step is to fill wanted, the set of revisions that we want to yield.
    # When it does not induce extra cost, we also fill fncache for revisions in
    # wanted: a cache of filenames that were changed (ctx.files()) and that
    # match the file filtering conditions.

    if match.always():
        # No files, no patterns.  Display all revs.
        wanted = revs

    if not slowpath and match.files():
        # We only have to read through the filelog to find wanted revisions

        try:
            wanted = walkfilerevs(repo, match, follow, revs, fncache)
        except FileWalkError:
            slowpath = True

            # We decided to fall back to the slowpath because at least one
            # of the paths was not a file. Check to see if at least one of them
            # existed in history, otherwise simply return
            for path in match.files():
                if path == '.' or path in repo.store:
                    break
            else:
                return []

    if slowpath:
        # We have to read the changelog to match filenames against
        # changed files

        if follow:
            raise util.Abort(_('can only follow copies/renames for explicit '
                               'filenames'))

        # The slow path checks files modified in every changeset.
        # This is really slow on large repos, so compute the set lazily.
        class lazywantedset(object):
            def __init__(self):
                self.set = set()
                self.revs = set(revs)

            # No need to worry about locality here because it will be accessed
            # in the same order as the increasing window below.
            def __contains__(self, value):
                if value in self.set:
                    return True
                elif not value in self.revs:
                    return False
                else:
                    self.revs.discard(value)
                    ctx = change(value)
                    matches = filter(match, ctx.files())
                    if matches:
                        fncache[value] = matches
                        self.set.add(value)
                        return True
                    return False

            def discard(self, value):
                self.revs.discard(value)
                self.set.discard(value)

        wanted = lazywantedset()

    # it might be worthwhile to do this in the iterator if the rev range
    # is descending and the prune args are all within that range
    for rev in opts.get('prune', ()):
        rev = repo[rev].rev()
        ff = _followfilter(repo)
        stop = min(revs[0], revs[-1])
        for x in xrange(rev, stop - 1, -1):
            if ff.match(x):
                wanted = wanted - [x]

    # Now that wanted is correctly initialized, we can iterate over the
    # revision range, yielding only revisions in wanted.
    def iterate():
        if follow and not match.files():
            ff = _followfilter(repo, onlyfirst=opts.get('follow_first'))
            def want(rev):
                return ff.match(rev) and rev in wanted
        else:
            def want(rev):
                return rev in wanted

        it = iter(revs)
        stopiteration = False
        for windowsize in increasingwindows():
            nrevs = []
            for i in xrange(windowsize):
                rev = next(it, None)
                if rev is None:
                    stopiteration = True
                    break
                elif want(rev):
                    nrevs.append(rev)
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

            if stopiteration:
                break

    return iterate()

def _makefollowlogfilematcher(repo, files, followfirst):
    # When displaying a revision with --patch --follow FILE, we have
    # to know which file of the revision must be diffed. With
    # --follow, we want the names of the ancestors of FILE in the
    # revision, stored in "fcache". "fcache" is populated by
    # reproducing the graph traversal already done by --follow revset
    # and relating linkrevs to file names (which is not "correct" but
    # good enough).
    fcache = {}
    fcacheready = [False]
    pctx = repo['.']

    def populate():
        for fn in files:
            for i in ((pctx[fn],), pctx[fn].ancestors(followfirst=followfirst)):
                for c in i:
                    fcache.setdefault(c.linkrev(), set()).add(c.path())

    def filematcher(rev):
        if not fcacheready[0]:
            # Lazy initialization
            fcacheready[0] = True
            populate()
        return scmutil.matchfiles(repo, fcache.get(rev, []))

    return filematcher

def _makenofollowlogfilematcher(repo, pats, opts):
    '''hook for extensions to override the filematcher for non-follow cases'''
    return None

def _makelogrevset(repo, pats, opts, revs):
    """Return (expr, filematcher) where expr is a revset string built
    from log options and file patterns or None. If --stat or --patch
    are not passed filematcher is None. Otherwise it is a callable
    taking a revision number and returning a match objects filtering
    the files to be detailed when displaying the revision.
    """
    opt2revset = {
        'no_merges':        ('not merge()', None),
        'only_merges':      ('merge()', None),
        '_ancestors':       ('ancestors(%(val)s)', None),
        '_fancestors':      ('_firstancestors(%(val)s)', None),
        '_descendants':     ('descendants(%(val)s)', None),
        '_fdescendants':    ('_firstdescendants(%(val)s)', None),
        '_matchfiles':      ('_matchfiles(%(val)s)', None),
        'date':             ('date(%(val)r)', None),
        'branch':           ('branch(%(val)r)', ' or '),
        '_patslog':         ('filelog(%(val)r)', ' or '),
        '_patsfollow':      ('follow(%(val)r)', ' or '),
        '_patsfollowfirst': ('_followfirst(%(val)r)', ' or '),
        'keyword':          ('keyword(%(val)r)', ' or '),
        'prune':            ('not (%(val)r or ancestors(%(val)r))', ' and '),
        'user':             ('user(%(val)r)', ' or '),
        }

    opts = dict(opts)
    # follow or not follow?
    follow = opts.get('follow') or opts.get('follow_first')
    if opts.get('follow_first'):
        followfirst = 1
    else:
        followfirst = 0
    # --follow with FILE behaviour depends on revs...
    it = iter(revs)
    startrev = it.next()
    followdescendants = startrev < next(it, startrev)

    # branch and only_branch are really aliases and must be handled at
    # the same time
    opts['branch'] = opts.get('branch', []) + opts.get('only_branch', [])
    opts['branch'] = [repo.lookupbranch(b) for b in opts['branch']]
    # pats/include/exclude are passed to match.match() directly in
    # _matchfiles() revset but walkchangerevs() builds its matcher with
    # scmutil.match(). The difference is input pats are globbed on
    # platforms without shell expansion (windows).
    wctx = repo[None]
    match, pats = scmutil.matchandpats(wctx, pats, opts)
    slowpath = match.anypats() or (match.files() and opts.get('removed'))
    if not slowpath:
        for f in match.files():
            if follow and f not in wctx:
                # If the file exists, it may be a directory, so let it
                # take the slow path.
                if os.path.exists(repo.wjoin(f)):
                    slowpath = True
                    continue
                else:
                    raise util.Abort(_('cannot follow file not in parent '
                                       'revision: "%s"') % f)
            filelog = repo.file(f)
            if not filelog:
                # A zero count may be a directory or deleted file, so
                # try to find matching entries on the slow path.
                if follow:
                    raise util.Abort(
                        _('cannot follow nonexistent file: "%s"') % f)
                slowpath = True

        # We decided to fall back to the slowpath because at least one
        # of the paths was not a file. Check to see if at least one of them
        # existed in history - in that case, we'll continue down the
        # slowpath; otherwise, we can turn off the slowpath
        if slowpath:
            for path in match.files():
                if path == '.' or path in repo.store:
                    break
            else:
                slowpath = False

    fpats = ('_patsfollow', '_patsfollowfirst')
    fnopats = (('_ancestors', '_fancestors'),
               ('_descendants', '_fdescendants'))
    if slowpath:
        # See walkchangerevs() slow path.
        #
        # pats/include/exclude cannot be represented as separate
        # revset expressions as their filtering logic applies at file
        # level. For instance "-I a -X a" matches a revision touching
        # "a" and "b" while "file(a) and not file(b)" does
        # not. Besides, filesets are evaluated against the working
        # directory.
        matchargs = ['r:', 'd:relpath']
        for p in pats:
            matchargs.append('p:' + p)
        for p in opts.get('include', []):
            matchargs.append('i:' + p)
        for p in opts.get('exclude', []):
            matchargs.append('x:' + p)
        matchargs = ','.join(('%r' % p) for p in matchargs)
        opts['_matchfiles'] = matchargs
        if follow:
            opts[fnopats[0][followfirst]] = '.'
    else:
        if follow:
            if pats:
                # follow() revset interprets its file argument as a
                # manifest entry, so use match.files(), not pats.
                opts[fpats[followfirst]] = list(match.files())
            else:
                op = fnopats[followdescendants][followfirst]
                opts[op] = 'rev(%d)' % startrev
        else:
            opts['_patslog'] = list(pats)

    filematcher = None
    if opts.get('patch') or opts.get('stat'):
        # When following files, track renames via a special matcher.
        # If we're forced to take the slowpath it means we're following
        # at least one pattern/directory, so don't bother with rename tracking.
        if follow and not match.always() and not slowpath:
            # _makefollowlogfilematcher expects its files argument to be
            # relative to the repo root, so use match.files(), not pats.
            filematcher = _makefollowlogfilematcher(repo, match.files(),
                                                    followfirst)
        else:
            filematcher = _makenofollowlogfilematcher(repo, pats, opts)
            if filematcher is None:
                filematcher = lambda rev: match

    expr = []
    for op, val in sorted(opts.iteritems()):
        if not val:
            continue
        if op not in opt2revset:
            continue
        revop, andor = opt2revset[op]
        if '%(val)' not in revop:
            expr.append(revop)
        else:
            if not isinstance(val, list):
                e = revop % {'val': val}
            else:
                e = '(' + andor.join((revop % {'val': v}) for v in val) + ')'
            expr.append(e)

    if expr:
        expr = '(' + ' and '.join(expr) + ')'
    else:
        expr = None
    return expr, filematcher

def _logrevs(repo, opts):
    # Default --rev value depends on --follow but --follow behaviour
    # depends on revisions resolved from --rev...
    follow = opts.get('follow') or opts.get('follow_first')
    if opts.get('rev'):
        revs = scmutil.revrange(repo, opts['rev'])
    elif follow and repo.dirstate.p1() == nullid:
        revs = revset.baseset()
    elif follow:
        revs = repo.revs('reverse(:.)')
    else:
        revs = revset.spanset(repo)
        revs.reverse()
    return revs

def getgraphlogrevs(repo, pats, opts):
    """Return (revs, expr, filematcher) where revs is an iterable of
    revision numbers, expr is a revset string built from log options
    and file patterns or None, and used to filter 'revs'. If --stat or
    --patch are not passed filematcher is None. Otherwise it is a
    callable taking a revision number and returning a match objects
    filtering the files to be detailed when displaying the revision.
    """
    limit = loglimit(opts)
    revs = _logrevs(repo, opts)
    if not revs:
        return revset.baseset(), None, None
    expr, filematcher = _makelogrevset(repo, pats, opts, revs)
    if opts.get('rev'):
        # User-specified revs might be unsorted, but don't sort before
        # _makelogrevset because it might depend on the order of revs
        revs.sort(reverse=True)
    if expr:
        # Revset matchers often operate faster on revisions in changelog
        # order, because most filters deal with the changelog.
        revs.reverse()
        matcher = revset.match(repo.ui, expr)
        # Revset matches can reorder revisions. "A or B" typically returns
        # returns the revision matching A then the revision matching B. Sort
        # again to fix that.
        revs = matcher(repo, revs)
        revs.sort(reverse=True)
    if limit is not None:
        limitedrevs = []
        for idx, rev in enumerate(revs):
            if idx >= limit:
                break
            limitedrevs.append(rev)
        revs = revset.baseset(limitedrevs)

    return revs, expr, filematcher

def getlogrevs(repo, pats, opts):
    """Return (revs, expr, filematcher) where revs is an iterable of
    revision numbers, expr is a revset string built from log options
    and file patterns or None, and used to filter 'revs'. If --stat or
    --patch are not passed filematcher is None. Otherwise it is a
    callable taking a revision number and returning a match objects
    filtering the files to be detailed when displaying the revision.
    """
    limit = loglimit(opts)
    revs = _logrevs(repo, opts)
    if not revs:
        return revset.baseset([]), None, None
    expr, filematcher = _makelogrevset(repo, pats, opts, revs)
    if expr:
        # Revset matchers often operate faster on revisions in changelog
        # order, because most filters deal with the changelog.
        if not opts.get('rev'):
            revs.reverse()
        matcher = revset.match(repo.ui, expr)
        # Revset matches can reorder revisions. "A or B" typically returns
        # returns the revision matching A then the revision matching B. Sort
        # again to fix that.
        revs = matcher(repo, revs)
        if not opts.get('rev'):
            revs.sort(reverse=True)
    if limit is not None:
        limitedrevs = []
        for idx, r in enumerate(revs):
            if limit <= idx:
                break
            limitedrevs.append(r)
        revs = revset.baseset(limitedrevs)

    return revs, expr, filematcher

def displaygraph(ui, dag, displayer, showparents, edgefn, getrenamed=None,
                 filematcher=None):
    seen, state = [], graphmod.asciistate()
    for rev, type, ctx, parents in dag:
        char = 'o'
        if ctx.node() in showparents:
            char = '@'
        elif ctx.obsolete():
            char = 'x'
        elif ctx.closesbranch():
            char = '_'
        copies = None
        if getrenamed and ctx.rev():
            copies = []
            for fn in ctx.files():
                rename = getrenamed(fn, ctx.rev())
                if rename:
                    copies.append((fn, rename[0]))
        revmatchfn = None
        if filematcher is not None:
            revmatchfn = filematcher(ctx.rev())
        displayer.show(ctx, copies=copies, matchfn=revmatchfn)
        lines = displayer.hunk.pop(rev).split('\n')
        if not lines[-1]:
            del lines[-1]
        displayer.flush(rev)
        edges = edgefn(type, char, lines, seen, rev, parents)
        for type, char, lines, coldata in edges:
            graphmod.ascii(ui, state, type, char, lines, coldata)
    displayer.close()

def graphlog(ui, repo, *pats, **opts):
    # Parameters are identical to log command ones
    revs, expr, filematcher = getgraphlogrevs(repo, pats, opts)
    revdag = graphmod.dagwalker(repo, revs)

    getrenamed = None
    if opts.get('copies'):
        endrev = None
        if opts.get('rev'):
            endrev = scmutil.revrange(repo, opts.get('rev')).max() + 1
        getrenamed = templatekw.getrenamedfn(repo, endrev=endrev)
    displayer = show_changeset(ui, repo, opts, buffered=True)
    showparents = [ctx.node() for ctx in repo[None].parents()]
    displaygraph(ui, revdag, displayer, showparents,
                 graphmod.asciiedges, getrenamed, filematcher)

def checkunsupportedgraphflags(pats, opts):
    for op in ["newest_first"]:
        if op in opts and opts[op]:
            raise util.Abort(_("-G/--graph option is incompatible with --%s")
                             % op.replace("_", "-"))

def graphrevs(repo, nodes, opts):
    limit = loglimit(opts)
    nodes.reverse()
    if limit is not None:
        nodes = nodes[:limit]
    return graphmod.nodes(repo, nodes)

def add(ui, repo, match, prefix, explicitonly, **opts):
    join = lambda f: os.path.join(prefix, f)
    bad = []
    oldbad = match.bad
    match.bad = lambda x, y: bad.append(x) or oldbad(x, y)
    names = []
    wctx = repo[None]
    cca = None
    abort, warn = scmutil.checkportabilityalert(ui)
    if abort or warn:
        cca = scmutil.casecollisionauditor(ui, abort, repo.dirstate)
    for f in wctx.walk(match):
        exact = match.exact(f)
        if exact or not explicitonly and f not in wctx and repo.wvfs.lexists(f):
            if cca:
                cca(f)
            names.append(f)
            if ui.verbose or not exact:
                ui.status(_('adding %s\n') % match.rel(f))

    for subpath in sorted(wctx.substate):
        sub = wctx.sub(subpath)
        try:
            submatch = matchmod.narrowmatcher(subpath, match)
            if opts.get('subrepos'):
                bad.extend(sub.add(ui, submatch, prefix, False, **opts))
            else:
                bad.extend(sub.add(ui, submatch, prefix, True, **opts))
        except error.LookupError:
            ui.status(_("skipping missing subrepository: %s\n")
                           % join(subpath))

    if not opts.get('dry_run'):
        rejected = wctx.add(names, prefix)
        bad.extend(f for f in rejected if f in match.files())
    return bad

def forget(ui, repo, match, prefix, explicitonly):
    join = lambda f: os.path.join(prefix, f)
    bad = []
    oldbad = match.bad
    match.bad = lambda x, y: bad.append(x) or oldbad(x, y)
    wctx = repo[None]
    forgot = []
    s = repo.status(match=match, clean=True)
    forget = sorted(s[0] + s[1] + s[3] + s[6])
    if explicitonly:
        forget = [f for f in forget if match.exact(f)]

    for subpath in sorted(wctx.substate):
        sub = wctx.sub(subpath)
        try:
            submatch = matchmod.narrowmatcher(subpath, match)
            subbad, subforgot = sub.forget(submatch, prefix)
            bad.extend([subpath + '/' + f for f in subbad])
            forgot.extend([subpath + '/' + f for f in subforgot])
        except error.LookupError:
            ui.status(_("skipping missing subrepository: %s\n")
                           % join(subpath))

    if not explicitonly:
        for f in match.files():
            if f not in repo.dirstate and not repo.wvfs.isdir(f):
                if f not in forgot:
                    if repo.wvfs.exists(f):
                        # Don't complain if the exact case match wasn't given.
                        # But don't do this until after checking 'forgot', so
                        # that subrepo files aren't normalized, and this op is
                        # purely from data cached by the status walk above.
                        if repo.dirstate.normalize(f) in repo.dirstate:
                            continue
                        ui.warn(_('not removing %s: '
                                  'file is already untracked\n')
                                % match.rel(f))
                    bad.append(f)

    for f in forget:
        if ui.verbose or not match.exact(f):
            ui.status(_('removing %s\n') % match.rel(f))

    rejected = wctx.forget(forget, prefix)
    bad.extend(f for f in rejected if f in match.files())
    forgot.extend(f for f in forget if f not in rejected)
    return bad, forgot

def files(ui, ctx, m, fm, fmt, subrepos):
    rev = ctx.rev()
    ret = 1
    ds = ctx.repo().dirstate

    for f in ctx.matches(m):
        if rev is None and ds[f] == 'r':
            continue
        fm.startitem()
        if ui.verbose:
            fc = ctx[f]
            fm.write('size flags', '% 10d % 1s ', fc.size(), fc.flags())
        fm.data(abspath=f)
        fm.write('path', fmt, m.rel(f))
        ret = 0

    if subrepos:
        for subpath in sorted(ctx.substate):
            sub = ctx.sub(subpath)
            try:
                submatch = matchmod.narrowmatcher(subpath, m)
                if sub.printfiles(ui, submatch, fm, fmt) == 0:
                    ret = 0
            except error.LookupError:
                ui.status(_("skipping missing subrepository: %s\n")
                               % m.abs(subpath))

    return ret

def remove(ui, repo, m, prefix, after, force, subrepos):
    join = lambda f: os.path.join(prefix, f)
    ret = 0
    s = repo.status(match=m, clean=True)
    modified, added, deleted, clean = s[0], s[1], s[3], s[6]

    wctx = repo[None]

    for subpath in sorted(wctx.substate):
        def matchessubrepo(matcher, subpath):
            if matcher.exact(subpath):
                return True
            for f in matcher.files():
                if f.startswith(subpath):
                    return True
            return False

        if subrepos or matchessubrepo(m, subpath):
            sub = wctx.sub(subpath)
            try:
                submatch = matchmod.narrowmatcher(subpath, m)
                if sub.removefiles(submatch, prefix, after, force, subrepos):
                    ret = 1
            except error.LookupError:
                ui.status(_("skipping missing subrepository: %s\n")
                               % join(subpath))

    # warn about failure to delete explicit files/dirs
    deleteddirs = util.dirs(deleted)
    for f in m.files():
        def insubrepo():
            for subpath in wctx.substate:
                if f.startswith(subpath):
                    return True
            return False

        isdir = f in deleteddirs or wctx.hasdir(f)
        if f in repo.dirstate or isdir or f == '.' or insubrepo():
            continue

        if repo.wvfs.exists(f):
            if repo.wvfs.isdir(f):
                ui.warn(_('not removing %s: no tracked files\n')
                        % m.rel(f))
            else:
                ui.warn(_('not removing %s: file is untracked\n')
                        % m.rel(f))
        # missing files will generate a warning elsewhere
        ret = 1

    if force:
        list = modified + deleted + clean + added
    elif after:
        list = deleted
        for f in modified + added + clean:
            ui.warn(_('not removing %s: file still exists\n') % m.rel(f))
            ret = 1
    else:
        list = deleted + clean
        for f in modified:
            ui.warn(_('not removing %s: file is modified (use -f'
                      ' to force removal)\n') % m.rel(f))
            ret = 1
        for f in added:
            ui.warn(_('not removing %s: file has been marked for add'
                      ' (use forget to undo)\n') % m.rel(f))
            ret = 1

    for f in sorted(list):
        if ui.verbose or not m.exact(f):
            ui.status(_('removing %s\n') % m.rel(f))

    wlock = repo.wlock()
    try:
        if not after:
            for f in list:
                if f in added:
                    continue # we never unlink added files on remove
                util.unlinkpath(repo.wjoin(f), ignoremissing=True)
        repo[None].forget(list)
    finally:
        wlock.release()

    return ret

def cat(ui, repo, ctx, matcher, prefix, **opts):
    err = 1

    def write(path):
        fp = makefileobj(repo, opts.get('output'), ctx.node(),
                         pathname=os.path.join(prefix, path))
        data = ctx[path].data()
        if opts.get('decode'):
            data = repo.wwritedata(path, data)
        fp.write(data)
        fp.close()

    # Automation often uses hg cat on single files, so special case it
    # for performance to avoid the cost of parsing the manifest.
    if len(matcher.files()) == 1 and not matcher.anypats():
        file = matcher.files()[0]
        mf = repo.manifest
        mfnode = ctx.manifestnode()
        if mfnode and mf.find(mfnode, file)[0]:
            write(file)
            return 0

    # Don't warn about "missing" files that are really in subrepos
    bad = matcher.bad

    def badfn(path, msg):
        for subpath in ctx.substate:
            if path.startswith(subpath):
                return
        bad(path, msg)

    matcher.bad = badfn

    for abs in ctx.walk(matcher):
        write(abs)
        err = 0

    matcher.bad = bad

    for subpath in sorted(ctx.substate):
        sub = ctx.sub(subpath)
        try:
            submatch = matchmod.narrowmatcher(subpath, matcher)

            if not sub.cat(submatch, os.path.join(prefix, sub._path),
                           **opts):
                err = 0
        except error.RepoLookupError:
            ui.status(_("skipping missing subrepository: %s\n")
                           % os.path.join(prefix, subpath))

    return err

def commit(ui, repo, commitfunc, pats, opts):
    '''commit the specified files or all outstanding changes'''
    date = opts.get('date')
    if date:
        opts['date'] = util.parsedate(date)
    message = logmessage(ui, opts)
    matcher = scmutil.match(repo[None], pats, opts)

    # extract addremove carefully -- this function can be called from a command
    # that doesn't support addremove
    if opts.get('addremove'):
        if scmutil.addremove(repo, matcher, "", opts) != 0:
            raise util.Abort(
                _("failed to mark all new/missing files as added/removed"))

    return commitfunc(ui, repo, message, matcher, opts)

def amend(ui, repo, commitfunc, old, extra, pats, opts):
    # amend will reuse the existing user if not specified, but the obsolete
    # marker creation requires that the current user's name is specified.
    if obsolete.isenabled(repo, obsolete.createmarkersopt):
        ui.username() # raise exception if username not set

    ui.note(_('amending changeset %s\n') % old)
    base = old.p1()

    wlock = dsguard = lock = newid = None
    try:
        wlock = repo.wlock()
        dsguard = dirstateguard(repo, 'amend')
        lock = repo.lock()
        tr = repo.transaction('amend')
        try:
            # See if we got a message from -m or -l, if not, open the editor
            # with the message of the changeset to amend
            message = logmessage(ui, opts)
            # ensure logfile does not conflict with later enforcement of the
            # message. potential logfile content has been processed by
            # `logmessage` anyway.
            opts.pop('logfile')
            # First, do a regular commit to record all changes in the working
            # directory (if there are any)
            ui.callhooks = False
            activebookmark = repo._activebookmark
            try:
                repo._activebookmark = None
                opts['message'] = 'temporary amend commit for %s' % old
                node = commit(ui, repo, commitfunc, pats, opts)
            finally:
                repo._activebookmark = activebookmark
                ui.callhooks = True
            ctx = repo[node]

            # Participating changesets:
            #
            # node/ctx o - new (intermediate) commit that contains changes
            #          |   from working dir to go into amending commit
            #          |   (or a workingctx if there were no changes)
            #          |
            # old      o - changeset to amend
            #          |
            # base     o - parent of amending changeset

            # Update extra dict from amended commit (e.g. to preserve graft
            # source)
            extra.update(old.extra())

            # Also update it from the intermediate commit or from the wctx
            extra.update(ctx.extra())

            if len(old.parents()) > 1:
                # ctx.files() isn't reliable for merges, so fall back to the
                # slower repo.status() method
                files = set([fn for st in repo.status(base, old)[:3]
                             for fn in st])
            else:
                files = set(old.files())

            # Second, we use either the commit we just did, or if there were no
            # changes the parent of the working directory as the version of the
            # files in the final amend commit
            if node:
                ui.note(_('copying changeset %s to %s\n') % (ctx, base))

                user = ctx.user()
                date = ctx.date()
                # Recompute copies (avoid recording a -> b -> a)
                copied = copies.pathcopies(base, ctx)
                if old.p2:
                    copied.update(copies.pathcopies(old.p2(), ctx))

                # Prune files which were reverted by the updates: if old
                # introduced file X and our intermediate commit, node,
                # renamed that file, then those two files are the same and
                # we can discard X from our list of files. Likewise if X
                # was deleted, it's no longer relevant
                files.update(ctx.files())

                def samefile(f):
                    if f in ctx.manifest():
                        a = ctx.filectx(f)
                        if f in base.manifest():
                            b = base.filectx(f)
                            return (not a.cmp(b)
                                    and a.flags() == b.flags())
                        else:
                            return False
                    else:
                        return f not in base.manifest()
                files = [f for f in files if not samefile(f)]

                def filectxfn(repo, ctx_, path):
                    try:
                        fctx = ctx[path]
                        flags = fctx.flags()
                        mctx = context.memfilectx(repo,
                                                  fctx.path(), fctx.data(),
                                                  islink='l' in flags,
                                                  isexec='x' in flags,
                                                  copied=copied.get(path))
                        return mctx
                    except KeyError:
                        return None
            else:
                ui.note(_('copying changeset %s to %s\n') % (old, base))

                # Use version of files as in the old cset
                def filectxfn(repo, ctx_, path):
                    try:
                        return old.filectx(path)
                    except KeyError:
                        return None

                user = opts.get('user') or old.user()
                date = opts.get('date') or old.date()
            editform = mergeeditform(old, 'commit.amend')
            editor = getcommiteditor(editform=editform, **opts)
            if not message:
                editor = getcommiteditor(edit=True, editform=editform)
                message = old.description()

            pureextra = extra.copy()
            extra['amend_source'] = old.hex()

            new = context.memctx(repo,
                                 parents=[base.node(), old.p2().node()],
                                 text=message,
                                 files=files,
                                 filectxfn=filectxfn,
                                 user=user,
                                 date=date,
                                 extra=extra,
                                 editor=editor)

            newdesc =  changelog.stripdesc(new.description())
            if ((not node)
                and newdesc == old.description()
                and user == old.user()
                and date == old.date()
                and pureextra == old.extra()):
                # nothing changed. continuing here would create a new node
                # anyway because of the amend_source noise.
                #
                # This not what we expect from amend.
                return old.node()

            ph = repo.ui.config('phases', 'new-commit', phases.draft)
            try:
                if opts.get('secret'):
                    commitphase = 'secret'
                else:
                    commitphase = old.phase()
                repo.ui.setconfig('phases', 'new-commit', commitphase, 'amend')
                newid = repo.commitctx(new)
            finally:
                repo.ui.setconfig('phases', 'new-commit', ph, 'amend')
            if newid != old.node():
                # Reroute the working copy parent to the new changeset
                repo.setparents(newid, nullid)

                # Move bookmarks from old parent to amend commit
                bms = repo.nodebookmarks(old.node())
                if bms:
                    marks = repo._bookmarks
                    for bm in bms:
                        marks[bm] = newid
                    marks.write()
            #commit the whole amend process
            createmarkers = obsolete.isenabled(repo, obsolete.createmarkersopt)
            if createmarkers and newid != old.node():
                # mark the new changeset as successor of the rewritten one
                new = repo[newid]
                obs = [(old, (new,))]
                if node:
                    obs.append((ctx, ()))

                obsolete.createmarkers(repo, obs)
            tr.close()
        finally:
            tr.release()
        dsguard.close()
        if not createmarkers and newid != old.node():
            # Strip the intermediate commit (if there was one) and the amended
            # commit
            if node:
                ui.note(_('stripping intermediate changeset %s\n') % ctx)
            ui.note(_('stripping amended changeset %s\n') % old)
            repair.strip(ui, repo, old.node(), topic='amend-backup')
    finally:
        lockmod.release(lock, dsguard, wlock)
    return newid

def commiteditor(repo, ctx, subs, editform=''):
    if ctx.description():
        return ctx.description()
    return commitforceeditor(repo, ctx, subs, editform=editform)

def commitforceeditor(repo, ctx, subs, finishdesc=None, extramsg=None,
                      editform=''):
    if not extramsg:
        extramsg = _("Leave message empty to abort commit.")

    forms = [e for e in editform.split('.') if e]
    forms.insert(0, 'changeset')
    while forms:
        tmpl = repo.ui.config('committemplate', '.'.join(forms))
        if tmpl:
            committext = buildcommittemplate(repo, ctx, subs, extramsg, tmpl)
            break
        forms.pop()
    else:
        committext = buildcommittext(repo, ctx, subs, extramsg)

    # run editor in the repository root
    olddir = os.getcwd()
    os.chdir(repo.root)
    text = repo.ui.edit(committext, ctx.user(), ctx.extra(), editform=editform)
    text = re.sub("(?m)^HG:.*(\n|$)", "", text)
    os.chdir(olddir)

    if finishdesc:
        text = finishdesc(text)
    if not text.strip():
        raise util.Abort(_("empty commit message"))

    return text

def buildcommittemplate(repo, ctx, subs, extramsg, tmpl):
    ui = repo.ui
    tmpl, mapfile = gettemplate(ui, tmpl, None)

    try:
        t = changeset_templater(ui, repo, None, {}, tmpl, mapfile, False)
    except SyntaxError, inst:
        raise util.Abort(inst.args[0])

    for k, v in repo.ui.configitems('committemplate'):
        if k != 'changeset':
            t.t.cache[k] = v

    if not extramsg:
        extramsg = '' # ensure that extramsg is string

    ui.pushbuffer()
    t.show(ctx, extramsg=extramsg)
    return ui.popbuffer()

def buildcommittext(repo, ctx, subs, extramsg):
    edittext = []
    modified, added, removed = ctx.modified(), ctx.added(), ctx.removed()
    if ctx.description():
        edittext.append(ctx.description())
    edittext.append("")
    edittext.append("") # Empty line between message and comments.
    edittext.append(_("HG: Enter commit message."
                      "  Lines beginning with 'HG:' are removed."))
    edittext.append("HG: %s" % extramsg)
    edittext.append("HG: --")
    edittext.append(_("HG: user: %s") % ctx.user())
    if ctx.p2():
        edittext.append(_("HG: branch merge"))
    if ctx.branch():
        edittext.append(_("HG: branch '%s'") % ctx.branch())
    if bookmarks.isactivewdirparent(repo):
        edittext.append(_("HG: bookmark '%s'") % repo._activebookmark)
    edittext.extend([_("HG: subrepo %s") % s for s in subs])
    edittext.extend([_("HG: added %s") % f for f in added])
    edittext.extend([_("HG: changed %s") % f for f in modified])
    edittext.extend([_("HG: removed %s") % f for f in removed])
    if not added and not modified and not removed:
        edittext.append(_("HG: no files changed"))
    edittext.append("")

    return "\n".join(edittext)

def commitstatus(repo, node, branch, bheads=None, opts={}):
    ctx = repo[node]
    parents = ctx.parents()

    if (not opts.get('amend') and bheads and node not in bheads and not
        [x for x in parents if x.node() in bheads and x.branch() == branch]):
        repo.ui.status(_('created new head\n'))
        # The message is not printed for initial roots. For the other
        # changesets, it is printed in the following situations:
        #
        # Par column: for the 2 parents with ...
        #   N: null or no parent
        #   B: parent is on another named branch
        #   C: parent is a regular non head changeset
        #   H: parent was a branch head of the current branch
        # Msg column: whether we print "created new head" message
        # In the following, it is assumed that there already exists some
        # initial branch heads of the current branch, otherwise nothing is
        # printed anyway.
        #
        # Par Msg Comment
        # N N  y  additional topo root
        #
        # B N  y  additional branch root
        # C N  y  additional topo head
        # H N  n  usual case
        #
        # B B  y  weird additional branch root
        # C B  y  branch merge
        # H B  n  merge with named branch
        #
        # C C  y  additional head from merge
        # C H  n  merge with a head
        #
        # H H  n  head merge: head count decreases

    if not opts.get('close_branch'):
        for r in parents:
            if r.closesbranch() and r.branch() == branch:
                repo.ui.status(_('reopening closed branch head %d\n') % r)

    if repo.ui.debugflag:
        repo.ui.write(_('committed changeset %d:%s\n') % (int(ctx), ctx.hex()))
    elif repo.ui.verbose:
        repo.ui.write(_('committed changeset %d:%s\n') % (int(ctx), ctx))

def revert(ui, repo, ctx, parents, *pats, **opts):
    parent, p2 = parents
    node = ctx.node()

    mf = ctx.manifest()
    if node == p2:
        parent = p2
    if node == parent:
        pmf = mf
    else:
        pmf = None

    # need all matching names in dirstate and manifest of target rev,
    # so have to walk both. do not print errors if files exist in one
    # but not other. in both cases, filesets should be evaluated against
    # workingctx to get consistent result (issue4497). this means 'set:**'
    # cannot be used to select missing files from target rev.

    # `names` is a mapping for all elements in working copy and target revision
    # The mapping is in the form:
    #   <asb path in repo> -> (<path from CWD>, <exactly specified by matcher?>)
    names = {}

    wlock = repo.wlock()
    try:
        ## filling of the `names` mapping
        # walk dirstate to fill `names`

        interactive = opts.get('interactive', False)
        wctx = repo[None]
        m = scmutil.match(wctx, pats, opts)

        # we'll need this later
        targetsubs = sorted(s for s in wctx.substate if m(s))

        if not m.always():
            m.bad = lambda x, y: False
            for abs in repo.walk(m):
                names[abs] = m.rel(abs), m.exact(abs)

            # walk target manifest to fill `names`

            def badfn(path, msg):
                if path in names:
                    return
                if path in ctx.substate:
                    return
                path_ = path + '/'
                for f in names:
                    if f.startswith(path_):
                        return
                ui.warn("%s: %s\n" % (m.rel(path), msg))

            m.bad = badfn
            for abs in ctx.walk(m):
                if abs not in names:
                    names[abs] = m.rel(abs), m.exact(abs)

            # Find status of all file in `names`.
            m = scmutil.matchfiles(repo, names)

            changes = repo.status(node1=node, match=m,
                                  unknown=True, ignored=True, clean=True)
        else:
            changes = repo.status(node1=node, match=m)
            for kind in changes:
                for abs in kind:
                    names[abs] = m.rel(abs), m.exact(abs)

            m = scmutil.matchfiles(repo, names)

        modified = set(changes.modified)
        added    = set(changes.added)
        removed  = set(changes.removed)
        _deleted = set(changes.deleted)
        unknown  = set(changes.unknown)
        unknown.update(changes.ignored)
        clean    = set(changes.clean)
        modadded = set()

        # split between files known in target manifest and the others
        smf = set(mf)

        # determine the exact nature of the deleted changesets
        deladded = _deleted - smf
        deleted = _deleted - deladded

        # We need to account for the state of the file in the dirstate,
        # even when we revert against something else than parent. This will
        # slightly alter the behavior of revert (doing back up or not, delete
        # or just forget etc).
        if parent == node:
            dsmodified = modified
            dsadded = added
            dsremoved = removed
            # store all local modifications, useful later for rename detection
            localchanges = dsmodified | dsadded
            modified, added, removed = set(), set(), set()
        else:
            changes = repo.status(node1=parent, match=m)
            dsmodified = set(changes.modified)
            dsadded    = set(changes.added)
            dsremoved  = set(changes.removed)
            # store all local modifications, useful later for rename detection
            localchanges = dsmodified | dsadded

            # only take into account for removes between wc and target
            clean |= dsremoved - removed
            dsremoved &= removed
            # distinct between dirstate remove and other
            removed -= dsremoved

            modadded = added & dsmodified
            added -= modadded

            # tell newly modified apart.
            dsmodified &= modified
            dsmodified |= modified & dsadded # dirstate added may needs backup
            modified -= dsmodified

            # We need to wait for some post-processing to update this set
            # before making the distinction. The dirstate will be used for
            # that purpose.
            dsadded = added

        # in case of merge, files that are actually added can be reported as
        # modified, we need to post process the result
        if p2 != nullid:
            if pmf is None:
                # only need parent manifest in the merge case,
                # so do not read by default
                pmf = repo[parent].manifest()
            mergeadd = dsmodified - set(pmf)
            dsadded |= mergeadd
            dsmodified -= mergeadd

        # if f is a rename, update `names` to also revert the source
        cwd = repo.getcwd()
        for f in localchanges:
            src = repo.dirstate.copied(f)
            # XXX should we check for rename down to target node?
            if src and src not in names and repo.dirstate[src] == 'r':
                dsremoved.add(src)
                names[src] = (repo.pathto(src, cwd), True)

        # distinguish between file to forget and the other
        added = set()
        for abs in dsadded:
            if repo.dirstate[abs] != 'a':
                added.add(abs)
        dsadded -= added

        for abs in deladded:
            if repo.dirstate[abs] == 'a':
                dsadded.add(abs)
        deladded -= dsadded

        # For files marked as removed, we check if an unknown file is present at
        # the same path. If a such file exists it may need to be backed up.
        # Making the distinction at this stage helps have simpler backup
        # logic.
        removunk = set()
        for abs in removed:
            target = repo.wjoin(abs)
            if os.path.lexists(target):
                removunk.add(abs)
        removed -= removunk

        dsremovunk = set()
        for abs in dsremoved:
            target = repo.wjoin(abs)
            if os.path.lexists(target):
                dsremovunk.add(abs)
        dsremoved -= dsremovunk

        # action to be actually performed by revert
        # (<list of file>, message>) tuple
        actions = {'revert': ([], _('reverting %s\n')),
                   'add': ([], _('adding %s\n')),
                   'remove': ([], _('removing %s\n')),
                   'drop': ([], _('removing %s\n')),
                   'forget': ([], _('forgetting %s\n')),
                   'undelete': ([], _('undeleting %s\n')),
                   'noop': (None, _('no changes needed to %s\n')),
                   'unknown': (None, _('file not managed: %s\n')),
                  }

        # "constant" that convey the backup strategy.
        # All set to `discard` if `no-backup` is set do avoid checking
        # no_backup lower in the code.
        # These values are ordered for comparison purposes
        backup = 2  # unconditionally do backup
        check = 1   # check if the existing file differs from target
        discard = 0 # never do backup
        if opts.get('no_backup'):
            backup = check = discard

        backupanddel = actions['remove']
        if not opts.get('no_backup'):
            backupanddel = actions['drop']

        disptable = (
            # dispatch table:
            #   file state
            #   action
            #   make backup

            ## Sets that results that will change file on disk
            # Modified compared to target, no local change
            (modified,      actions['revert'],   discard),
            # Modified compared to target, but local file is deleted
            (deleted,       actions['revert'],   discard),
            # Modified compared to target, local change
            (dsmodified,    actions['revert'],   backup),
            # Added since target
            (added,         actions['remove'],   discard),
            # Added in working directory
            (dsadded,       actions['forget'],   discard),
            # Added since target, have local modification
            (modadded,      backupanddel,        backup),
            # Added since target but file is missing in working directory
            (deladded,      actions['drop'],   discard),
            # Removed since  target, before working copy parent
            (removed,       actions['add'],      discard),
            # Same as `removed` but an unknown file exists at the same path
            (removunk,      actions['add'],      check),
            # Removed since targe, marked as such in working copy parent
            (dsremoved,     actions['undelete'], discard),
            # Same as `dsremoved` but an unknown file exists at the same path
            (dsremovunk,    actions['undelete'], check),
            ## the following sets does not result in any file changes
            # File with no modification
            (clean,         actions['noop'],     discard),
            # Existing file, not tracked anywhere
            (unknown,       actions['unknown'],  discard),
            )

        for abs, (rel, exact) in sorted(names.items()):
            # target file to be touch on disk (relative to cwd)
            target = repo.wjoin(abs)
            # search the entry in the dispatch table.
            # if the file is in any of these sets, it was touched in the working
            # directory parent and we are sure it needs to be reverted.
            for table, (xlist, msg), dobackup in disptable:
                if abs not in table:
                    continue
                if xlist is not None:
                    xlist.append(abs)
                    if dobackup and (backup <= dobackup
                                     or wctx[abs].cmp(ctx[abs])):
                            bakname = "%s.orig" % rel
                            ui.note(_('saving current version of %s as %s\n') %
                                    (rel, bakname))
                            if not opts.get('dry_run'):
                                if interactive:
                                    util.copyfile(target, bakname)
                                else:
                                    util.rename(target, bakname)
                    if ui.verbose or not exact:
                        if not isinstance(msg, basestring):
                            msg = msg(abs)
                        ui.status(msg % rel)
                elif exact:
                    ui.warn(msg % rel)
                break

        if not opts.get('dry_run'):
            needdata = ('revert', 'add', 'undelete')
            _revertprefetch(repo, ctx, *[actions[name][0] for name in needdata])
            _performrevert(repo, parents, ctx, actions, interactive)

        if targetsubs:
            # Revert the subrepos on the revert list
            for sub in targetsubs:
                try:
                    wctx.sub(sub).revert(ctx.substate[sub], *pats, **opts)
                except KeyError:
                    raise util.Abort("subrepository '%s' does not exist in %s!"
                                      % (sub, short(ctx.node())))
    finally:
        wlock.release()

def _revertprefetch(repo, ctx, *files):
    """Let extension changing the storage layer prefetch content"""
    pass

def _performrevert(repo, parents, ctx, actions, interactive=False):
    """function that actually perform all the actions computed for revert

    This is an independent function to let extension to plug in and react to
    the imminent revert.

    Make sure you have the working directory locked when calling this function.
    """
    parent, p2 = parents
    node = ctx.node()
    def checkout(f):
        fc = ctx[f]
        return repo.wwrite(f, fc.data(), fc.flags())

    audit_path = pathutil.pathauditor(repo.root)
    for f in actions['forget'][0]:
        repo.dirstate.drop(f)
    for f in actions['remove'][0]:
        audit_path(f)
        try:
            util.unlinkpath(repo.wjoin(f))
        except OSError:
            pass
        repo.dirstate.remove(f)
    for f in actions['drop'][0]:
        audit_path(f)
        repo.dirstate.remove(f)

    normal = None
    if node == parent:
        # We're reverting to our parent. If possible, we'd like status
        # to report the file as clean. We have to use normallookup for
        # merges to avoid losing information about merged/dirty files.
        if p2 != nullid:
            normal = repo.dirstate.normallookup
        else:
            normal = repo.dirstate.normal

    if interactive:
        # Prompt the user for changes to revert
        torevert = [repo.wjoin(f) for f in actions['revert'][0]]
        m = scmutil.match(ctx, torevert, {})
        diff = patch.diff(repo, None, ctx.node(), m)
        originalchunks = patch.parsepatch(diff)
        try:
            chunks = recordfilter(repo.ui, originalchunks)
        except patch.PatchError, err:
            raise util.Abort(_('error parsing patch: %s') % err)

        # Apply changes
        fp = cStringIO.StringIO()
        for c in chunks:
            c.write(fp)
        dopatch = fp.tell()
        fp.seek(0)
        if dopatch:
            try:
                patch.internalpatch(repo.ui, repo, fp, 1, eolmode=None)
            except patch.PatchError, err:
                raise util.Abort(str(err))
        del fp
    else:
        for f in actions['revert'][0]:
            wsize = checkout(f)
            if normal:
                normal(f)
            elif wsize == repo.dirstate._map[f][2]:
                # changes may be overlooked without normallookup,
                # if size isn't changed at reverting
                repo.dirstate.normallookup(f)

    for f in actions['add'][0]:
        checkout(f)
        repo.dirstate.add(f)

    normal = repo.dirstate.normallookup
    if node == parent and p2 == nullid:
        normal = repo.dirstate.normal
    for f in actions['undelete'][0]:
        checkout(f)
        normal(f)

    copied = copies.pathcopies(repo[parent], ctx)

    for f in actions['add'][0] + actions['undelete'][0] + actions['revert'][0]:
        if f in copied:
            repo.dirstate.copy(copied[f], f)

def command(table):
    """Returns a function object to be used as a decorator for making commands.

    This function receives a command table as its argument. The table should
    be a dict.

    The returned function can be used as a decorator for adding commands
    to that command table. This function accepts multiple arguments to define
    a command.

    The first argument is the command name.

    The options argument is an iterable of tuples defining command arguments.
    See ``mercurial.fancyopts.fancyopts()`` for the format of each tuple.

    The synopsis argument defines a short, one line summary of how to use the
    command. This shows up in the help output.

    The norepo argument defines whether the command does not require a
    local repository. Most commands operate against a repository, thus the
    default is False.

    The optionalrepo argument defines whether the command optionally requires
    a local repository.

    The inferrepo argument defines whether to try to find a repository from the
    command line arguments. If True, arguments will be examined for potential
    repository locations. See ``findrepo()``. If a repository is found, it
    will be used.
    """
    def cmd(name, options=(), synopsis=None, norepo=False, optionalrepo=False,
            inferrepo=False):
        def decorator(func):
            if synopsis:
                table[name] = func, list(options), synopsis
            else:
                table[name] = func, list(options)

            if norepo:
                # Avoid import cycle.
                import commands
                commands.norepo += ' %s' % ' '.join(parsealiases(name))

            if optionalrepo:
                import commands
                commands.optionalrepo += ' %s' % ' '.join(parsealiases(name))

            if inferrepo:
                import commands
                commands.inferrepo += ' %s' % ' '.join(parsealiases(name))

            return func
        return decorator

    return cmd

# a list of (ui, repo, otherpeer, opts, missing) functions called by
# commands.outgoing.  "missing" is "missing" of the result of
# "findcommonoutgoing()"
outgoinghooks = util.hooks()

# a list of (ui, repo) functions called by commands.summary
summaryhooks = util.hooks()

# a list of (ui, repo, opts, changes) functions called by commands.summary.
#
# functions should return tuple of booleans below, if 'changes' is None:
#  (whether-incomings-are-needed, whether-outgoings-are-needed)
#
# otherwise, 'changes' is a tuple of tuples below:
#  - (sourceurl, sourcebranch, sourcepeer, incoming)
#  - (desturl,   destbranch,   destpeer,   outgoing)
summaryremotehooks = util.hooks()

# A list of state files kept by multistep operations like graft.
# Since graft cannot be aborted, it is considered 'clearable' by update.
# note: bisect is intentionally excluded
# (state file, clearable, allowcommit, error, hint)
unfinishedstates = [
    ('graftstate', True, False, _('graft in progress'),
     _("use 'hg graft --continue' or 'hg update' to abort")),
    ('updatestate', True, False, _('last update was interrupted'),
     _("use 'hg update' to get a consistent checkout"))
    ]

def checkunfinished(repo, commit=False):
    '''Look for an unfinished multistep operation, like graft, and abort
    if found. It's probably good to check this right before
    bailifchanged().
    '''
    for f, clearable, allowcommit, msg, hint in unfinishedstates:
        if commit and allowcommit:
            continue
        if repo.vfs.exists(f):
            raise util.Abort(msg, hint=hint)

def clearunfinished(repo):
    '''Check for unfinished operations (as above), and clear the ones
    that are clearable.
    '''
    for f, clearable, allowcommit, msg, hint in unfinishedstates:
        if not clearable and repo.vfs.exists(f):
            raise util.Abort(msg, hint=hint)
    for f, clearable, allowcommit, msg, hint in unfinishedstates:
        if clearable and repo.vfs.exists(f):
            util.unlink(repo.join(f))

class dirstateguard(object):
    '''Restore dirstate at unexpected failure.

    At the construction, this class does:

    - write current ``repo.dirstate`` out, and
    - save ``.hg/dirstate`` into the backup file

    This restores ``.hg/dirstate`` from backup file, if ``release()``
    is invoked before ``close()``.

    This just removes the backup file at ``close()`` before ``release()``.
    '''

    def __init__(self, repo, name):
        repo.dirstate.write()
        self._repo = repo
        self._filename = 'dirstate.backup.%s.%d' % (name, id(self))
        repo.vfs.write(self._filename, repo.vfs.tryread('dirstate'))
        self._active = True
        self._closed = False

    def __del__(self):
        if self._active: # still active
            # this may occur, even if this class is used correctly:
            # for example, releasing other resources like transaction
            # may raise exception before ``dirstateguard.release`` in
            # ``release(tr, ....)``.
            self._abort()

    def close(self):
        if not self._active: # already inactivated
            msg = (_("can't close already inactivated backup: %s")
                   % self._filename)
            raise util.Abort(msg)

        self._repo.vfs.unlink(self._filename)
        self._active = False
        self._closed = True

    def _abort(self):
        # this "invalidate()" prevents "wlock.release()" from writing
        # changes of dirstate out after restoring to original status
        self._repo.dirstate.invalidate()

        self._repo.vfs.rename(self._filename, 'dirstate')
        self._active = False

    def release(self):
        if not self._closed:
            if not self._active: # already inactivated
                msg = (_("can't release already inactivated backup: %s")
                       % self._filename)
                raise util.Abort(msg)
            self._abort()
