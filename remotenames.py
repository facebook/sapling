import os
import errno

from mercurial import bookmarks
from mercurial import commands
from mercurial import encoding
from mercurial import error
from mercurial import exchange
from mercurial import extensions
from mercurial import hg
from mercurial import localrepo
from mercurial import namespaces
from mercurial import obsolete
from mercurial import repoview
from mercurial import revset
from mercurial import scmutil
from mercurial import templatekw
from mercurial import url
from mercurial import util
from mercurial.i18n import _
from mercurial.node import hex, short
from hgext import schemes

_remotenames = {
    "bookmarks": {},
    "branches": {},
}

def expush(orig, repo, remote, *args, **kwargs):
    res = orig(repo, remote, *args, **kwargs)
    pullremotenames(repo, remote)
    return res

def expull(orig, repo, remote, *args, **kwargs):
    res = orig(repo, remote, *args, **kwargs)
    pullremotenames(repo, remote)
    return res

def pullremotenames(repo, remote):
    lock = repo.lock()
    try:
        path = activepath(repo.ui, remote)
        if path:
            # on a push, we don't want to keep obsolete heads since
            # they won't show up as heads on the next pull, so we
            # remove them here otherwise we would require the user
            # to issue a pull to refresh .hg/remotenames
            bmap = {}
            repo = repo.unfiltered()
            for branch, nodes in remote.branchmap().iteritems():
                bmap[branch] = [n for n in nodes if not repo[n].obsolete()]
            saveremotenames(repo, path, bmap, remote.listkeys('bookmarks'))
    finally:
        lock.release()

    loadremotenames(repo)
    writedistance(repo)

def blockerhook(orig, repo, *args, **kwargs):
    blockers = orig(repo)

    unblock = util.safehasattr(repo, '_unblockhiddenremotenames')
    if not unblock:
        return blockers

    # add remotenames to blockers by looping over all names in our own cache
    cl = repo.changelog
    for remotename in _remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            blockers.update(cl.rev(node) for node in ns.nodes(repo, name))

    return blockers

def exupdatefromremote(orig, ui, repo, remotemarks, path, trfunc, explicit=()):
    if ui.configbool('remotenames', 'syncbookmarks', False):
        return orig(ui, repo, remotemarks, path, trfunc, explicit)

    ui.debug('remotenames: skipped syncing local bookmarks\n')

def exclone(orig, ui, *args, **opts):
    """
    We may not want local bookmarks on clone... but we always want remotenames!
    """
    srcpeer, dstpeer = orig(ui, *args, **opts)

    pullremotenames(dstpeer.local(), srcpeer)

    if not ui.configbool('remotenames', 'syncbookmarks', False):
        ui.debug('remotenames: removing cloned bookmarks\n')
        repo = dstpeer.local()
        wlock = repo.wlock()
        try:
            try:
                repo.vfs.unlink('bookmarks')
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise
        finally:
            wlock.release()

    return (srcpeer, dstpeer)

def excommit(orig, repo, *args, **opts):
    res = orig(repo, *args, **opts)
    writedistance(repo)
    return res

def exupdate(orig, repo, *args, **opts):
    res = orig(repo, *args, **opts)
    writedistance(repo)
    return res

def exsetcurrent(orig, repo, mark):
    res = orig(repo, mark)
    writedistance(repo)
    return res

def reposetup(ui, repo):
    if not repo.local():
        return

    hoist = ui.config('remotenames', 'hoist')
    if hoist:
        hoist += '/'

    loadremotenames(repo)

    # cache this so we don't iterate over new values
    items = list(repo.names.iteritems())
    for nsname, ns in items:
        d = _remotenames.get(nsname)
        if not d:
            continue

        rname = 'remote' + nsname
        rtmpl = 'remote' + ns.templatename

        if nsname == 'bookmarks' and hoist:
            def names(rp, d=d):
                l = d.keys()
                for name in l:
                    if name.startswith(hoist):
                        l.append(name[len(hoist):])
                return l

            def namemap(rp, name, d=d):
                if name in d:
                    return d[name]
                return d.get(hoist + name)

            # we don't hoist nodemap because we don't want hoisted names
            # to show up in logs, which is the primary use case here
        else:
            names = lambda rp, d=d: d.keys()
            namemap = lambda rp, name, d=d: d.get(name)

        nodemap = lambda rp, node, d=d: [name for name, n in d.iteritems()
                                         for n2 in n if n2 == node]

        n = namespaces.namespace(rname, templatename=rtmpl,
                                 logname=ns.templatename, colorname=rtmpl,
                                 listnames=names, namemap=namemap,
                                 nodemap=nodemap)
        repo.names.addnamespace(n)

def _tracking(ui):
    # omg default true
    return ui.configbool('remotenames', 'tracking', True)

def setuptracking(ui):
    try:
        rebase = extensions.find('rebase')
        if rebase:
            extensions.wrapcommand(rebase.cmdtable, 'rebase', exrebase)
    except KeyError:
        # rebase isn't on
        pass

def exrebase(orig, ui, repo, **opts):
    dest = opts['dest']
    current = bookmarks.readcurrent(repo)
    if not dest and current:
        tracking = _readtracking(repo)
        if current in tracking:
            opts['dest'] = tracking[current]

    return orig(ui, repo, **opts)

def exstrip(orig, ui, repo, *args, **opts):
    ret = orig(ui, repo, *args, **opts)
    writedistance(repo)
    return ret

def exhistedit(orig, ui, repo, *args, **opts):
    ret = orig(ui, repo, *args, **opts)
    writedistance(repo)
    return ret

def expaths(orig, ui, repo, *args, **opts):
    """allow adding and removing remote paths

    This is very hacky and only exists as an experimentation.

    """
    delete = opts.get('delete')
    add = opts.get('add')
    if delete:
        # find the first section and remote path that matches, and delete that
        foundpaths = False
        oldhgrc = repo.vfs.read('hgrc').splitlines(True)
        f = repo.vfs('hgrc', 'w')
        for line in oldhgrc:
            if '[paths]' in line:
                foundpaths = True
            if not (foundpaths and line.strip().startswith(delete)):
                f.write(line)
        f.close()
        saveremotenames(repo, delete)
        writedistance(repo)
        return

    if add:
        # find the first section that matches, then look for previous value; if
        # not found add a new entry
        foundpaths = False
        oldhgrc = repo.vfs.read('hgrc').splitlines(True)
        f = repo.vfs('hgrc', 'w')
        done = False
        for line in oldhgrc:
            if '[paths]' in line:
                foundpaths = True
            if foundpaths and line.strip().startswith(add):
                done = True
                line = '%s = %s\n' % (add, args[0])
            f.write(line)

        # did we not find an existing path?
        if not done:
            done = True
            f.write("%s = %s\n" % (add, args[0]))

        f.close()
        return

    return orig(ui, repo, *args)

def extsetup(ui):
    extensions.wrapfunction(exchange, 'push', expush)
    extensions.wrapfunction(exchange, 'pull', expull)
    extensions.wrapfunction(repoview, '_getdynamicblockers', blockerhook)
    extensions.wrapfunction(bookmarks, 'updatefromremote', exupdatefromremote)
    extensions.wrapfunction(bookmarks, 'setcurrent', exsetcurrent)
    extensions.wrapfunction(hg, 'clone', exclone)
    extensions.wrapfunction(hg, 'updaterepo', exupdate)
    extensions.wrapfunction(localrepo.localrepository, 'commit', excommit)

    entry = extensions.wrapcommand(commands.table, 'bookmarks', exbookmarks)
    entry[1].append(('a', 'all', None, 'show both remote and local bookmarks'))
    entry[1].append(('', 'remote', None, 'show only remote bookmarks'))

    if _tracking(ui):
        entry[1].append(('t', 'track', '', 'track this bookmark or remote name',
                         'BOOKMARK'))
        entry[1].append(('u', 'untrack', None,
                         'remove tracking for this bookmark',
                         'BOOKMARK'))
        setuptracking(ui)

    entry = extensions.wrapcommand(commands.table, 'branches', exbranches)
    entry[1].append(('a', 'all', None, 'show both remote and local branches'))
    entry[1].append(('', 'remote', None, 'show only remote branches'))

    entry = extensions.wrapcommand(commands.table, 'log', exlog)
    entry[1].append(('', 'remote', None, 'show remote names even if hidden'))

    entry = extensions.wrapcommand(commands.table, 'push', expushcmd)
    entry[1].append(('t', 'to', '', 'push revs to this bookmark', 'BOOKMARK'))
    entry[1].append(('d', 'delete', '', 'delete remote bookmark', 'BOOKMARK'))

    entry = extensions.wrapcommand(commands.table, 'paths', expaths)
    entry[1].append(('d', 'delete', '', 'delete remote path', 'NAME'))
    entry[1].append(('a', 'add', '', 'add remote path', 'NAME PATH'))

    exchange.pushdiscoverymapping['bookmarks'] = expushdiscoverybookmarks

    templatekw.keywords['remotenames'] = remotenameskw

    try:
        strip = extensions.find('strip')
        if strip:
            extensions.wrapcommand(strip.cmdtable, 'strip', exstrip)
    except KeyError:
        # strip isn't on
        pass

    try:
        histedit = extensions.find('histedit')
        if histedit:
            extensions.wrapcommand(histedit.cmdtable, 'histedit', exhistedit)
    except KeyError:
        # histedit isn't on
        pass

def exlog(orig, ui, repo, *args, **opts):
    # hack for logging that turns on the dynamic blockerhook
    if opts.get('remote'):
        repo.__setattr__('_unblockhiddenremotenames', True)
    res = orig(ui, repo, *args, **opts)
    if opts.get('remote'):
        repo.__setattr__('_unblockhiddenremotenames', False)
    return res

_pushto = False
_delete = None

def expushdiscoverybookmarks(pushop):
    repo = pushop.repo.unfiltered()
    remotemarks = pushop.remote.listkeys('bookmarks')
    force = pushop.force

    if _delete:
        if _delete not in remotemarks:
            raise util.Abort(_('remote bookmark %s does not exist'))
        pushop.outbookmarks.append([_delete, remotemarks[_delete], ''])
        return exchange._pushdiscoverybookmarks(pushop)

    if not _pushto:
        ret = exchange._pushdiscoverybookmarks(pushop)
        if not (repo.ui.configbool('remotenames', 'pushanonheads')
                or force):
            # check to make sure we don't push an anonymous head
            if pushop.revs:
                revs = set(pushop.revs)
            else:
                revs = set(repo.lookup(r) for r in repo.revs('head()'))
            revs -= set(pushop.remoteheads)
            # find heads that don't have a bookmark going with them
            for bookmark in pushop.bookmarks:
                rev = repo.lookup(bookmark)
                if rev in revs:
                    revs.remove(rev)
            # remove heads that already have a remote bookmark
            for bookmark, node in remotemarks.iteritems():
                rev = repo.lookup(node)
                if rev in revs:
                    revs.remove(rev)
            # remove heads that already advance bookmarks (old mercurial
            # behavior)
            for bookmark, old, new in pushop.outbookmarks:
                rev = repo.lookup(new)
                if rev in revs:
                    revs.remove(rev)

            revs = [short(r) for r in revs
                    if not repo[r].obsolete()
                    and not repo[r].closesbranch()]
            if revs:
                msg = _("push would create new anonymous heads (%s)")
                hint = _("use --force to override this warning")
                raise util.Abort(msg % ', '.join(sorted(revs)), hint=hint)
        return ret

    bookmark = pushop.bookmarks[0]
    rev = pushop.revs[0]

    # allow new bookmark only if force is True
    old = ''
    if bookmark in remotemarks:
        old = remotemarks[bookmark]
    elif not force:
        msg = _('not creating new bookmark')
        hint = _('use --force to create a new bookmark')
        raise util.Abort(msg, hint=hint)

    # allow non-ff only if force is True
    if not force and old != '':
        if old not in repo:
            msg = _('remote bookmark revision is not in local repo')
            hint = _('pull and merge or rebase or use --force')
            raise util.Abort(msg, hint=hint)
        foreground = obsolete.foreground(repo, [repo.lookup(old)])
        if repo[rev].node() not in foreground:
            msg = _('pushed rev is not in the foreground of remote bookmark')
            hint = _('use --force flag to complete non-fast-forward update')
            raise util.Abort(msg, hint=hint)
        if repo[old] == repo[rev]:
            repo.ui.warn(_('remote bookmark already points at pushed rev\n'))
            return

    pushop.outbookmarks.append((bookmark, old, hex(rev)))

def _pushrevs(repo, ui, rev):
    pushrev = ui.config('remotenames', 'pushrev')
    if pushrev:
        return [repo.lookup(pushrev)]
    if rev:
        return [repo.lookup(rev)]
    return []

def expushcmd(orig, ui, repo, dest=None, **opts):
    # needed for discovery method
    global _pushto, _delete

    _delete = opts.get('delete')
    if _delete:
        flag = None
        for f in ('to', 'bookmark', 'branch', 'rev'):
            if opts.get(f):
                flag = f
                break
        if flag:
            msg = _('do not specify --delete and '
                    '--%s at the same time') % flag
            raise util.Abort(msg)
        # we want to skip pushing any changesets while deleting a remote
        # bookmark, so we send the null revision
        opts['rev'] = ['null']
        return orig(ui, repo, dest, **opts)

    revs = opts.get('rev')
    to = opts.get('to')

    revrenames = dict((v, k) for k, v in _getrenames(ui).iteritems())

    if not dest and not to and not revs and _tracking(ui):
        current = bookmarks.readcurrent(repo)
        tracking = _readtracking(repo)
        # print "tracking on %s %s" % (current, tracking)
        if current and current in tracking:
            track = tracking[current]
            path, book = splitremotename(track)
            # un-rename a path, if needed
            path = revrenames.get(path, path)
            paths = set(path for path, ignore in ui.configitems('paths'))
            if book and path in paths:
                dest = path
                to = book

    # un-rename passed path
    dest = revrenames.get(dest, dest)

    if not to:
        if ui.configbool('remotenames', 'forceto', False):
            msg = _('must specify --to when pushing')
            hint = _('see configuration option %s') % 'remotenames.forceto'
            raise util.Abort(msg, hint=hint)

        if not revs:
            opts['rev'] = _pushrevs(repo, ui, None)

        return orig(ui, repo, dest, **opts)

    if opts.get('bookmark'):
        msg = _('do not specify --to/-t and --bookmark/-B at the same time')
        raise util.Abort(msg)
    if opts.get('branch'):
        msg = _('do not specify --to/-t and --branch/-b at the same time')
        raise util.Abort(msg)

    if revs:
        revs = [repo.lookup(r) for r in scmutil.revrange(repo, revs)]
    else:
        revs = _pushrevs(repo, ui, '.')
    if len(revs) != 1:
        msg = _('--to requires exactly one rev to push')
        hint = _('use --rev BOOKMARK or omit --rev for current commit (.)')
        raise util.Abort(msg, hint=hint)
    rev = revs[0]

    _pushto = True

    # big can o' copypasta from exchange.push
    dest = ui.expandpath(dest or 'default-push', dest or 'default')
    dest, branches = hg.parseurl(dest, opts.get('branch'))
    try:
        other = hg.peer(repo, opts, dest)
    except error.RepoError:
        if dest == "default-push":
            hint = _('see the "path" section in "hg help config"')
            raise util.Abort(_("default repository not configured!"),
                             hint=hint)
        else:
            raise

    # all checks pass, go for it!
    ui.status(_('pushing rev %s to destination %s bookmark %s\n') % (
              short(rev), dest, to))

    # TODO: subrepo stuff

    pushop = exchange.push(repo, other, opts.get('force'), revs=revs,
                           bookmarks=(to,))

    result = not pushop.cgresult
    if pushop.bkresult is not None:
        if pushop.bkresult == 2:
            result = 2
        elif not result and pushop.bkresult:
            result = 2

    _pushto = False
    return result

def exbranches(orig, ui, repo, *args, **opts):
    if not opts.get('remote'):
        orig(ui, repo, *args, **opts)

    if opts.get('all') or opts.get('remote'):
        # exit early if namespace doesn't even exist
        namespace = 'remotebranches'
        if namespace not in repo.names:
            return

        ns = repo.names[namespace]
        label = 'log.' + ns.colorname
        fm = ui.formatter('branches', opts)

        # it seems overkill to hide displaying hidden remote branches
        repo = repo.unfiltered()

        # create a sorted by descending rev list
        revs = set()
        for name in ns.listnames(repo):
            for n in ns.nodes(repo, name):
                revs.add(repo.changelog.rev(n))

        for r in sorted(revs, reverse=True):
            ctx = repo[r]
            for name in ns.names(repo, ctx.node()):
                fm.startitem()
                padsize = max(31 - len(str(r)) - encoding.colwidth(name), 0)

                tmplabel = label
                if ctx.obsolete():
                    tmplabel = tmplabel + ' changeset.obsolete'
                fm.write(ns.colorname, '%s', name, label=label)
                fmt = ' ' * padsize + ' %d:%s'
                fm.condwrite(not ui.quiet, 'rev node', fmt, r,
                             fm.hexfunc(ctx.node()), label=tmplabel)
                fm.plain('\n')
        fm.end()

def _readtracking(repo):
    tracking = {}
    try:
        for line in repo.vfs.read('bookmarks.tracking').strip().split('\n'):
            try:
                book, track = line.strip().split(' ')
                tracking[book] = track
            except ValueError:
                # corrupt file, ignore entry
                pass
    except IOError:
        pass
    return tracking

def _writetracking(repo, tracking):
    data = ''
    for book, track in tracking.iteritems():
        data += '%s %s\n' % (book, track)
    repo.vfs.write('bookmarks.tracking', data)

def _removetracking(repo, bookmarks):
    tracking = _readtracking(repo)
    needwrite = False
    for bmark in bookmarks:
        try:
            del tracking[bmark]
            needwrite = True
        except KeyError:
            pass
    if needwrite:
        _writetracking(repo, tracking)

def exbookmarks(orig, ui, repo, *args, **opts):
    """Bookmark output is sorted by bookmark name.

    This has the side benefit of grouping all remote bookmarks by remote name.

    """
    delete = opts.get('delete')
    rename = opts.get('rename')
    inactive = opts.get('inactive')
    remote = opts.get('remote')
    track = opts.get('track')
    untrack = opts.get('untrack')

    disallowed = set(ui.configlist('remotenames', 'disallowedbookmarks'))
    for name in args:
        if name in disallowed:
            raise util.Abort(_(" bookmark '%s' not allowed by configuration")
                             % name)

    if untrack:
        if track:
            msg = _('do not specify --untrack and --track at the same time')
            raise util.Abort(msg)
        _removetracking(repo, args)
        return

    if delete or rename or args or inactive:
        ret = orig(ui, repo, *args, **opts)
        if track:
            tracking = _readtracking(repo)
            for arg in args:
                tracking[arg] = track
            _writetracking(repo, tracking)
            # update the cache
            writedistance(repo)

        # also remove tracking for a deleted bookmark, if it exists
        if delete:
            _removetracking(repo, args)

        return ret

    # copy pasta from commands.py; need to patch core
    if not remote:
        fm = ui.formatter('bookmarks', opts)
        hexfn = fm.hexfunc
        marks = repo._bookmarks
        if len(marks) == 0 and not fm:
            ui.status(_("no bookmarks set\n"))
        for bmark, n in sorted(marks.iteritems()):
            current = repo._bookmarkcurrent
            if bmark == current:
                prefix, label = '*', 'bookmarks.current'
            else:
                prefix, label = ' ', ''

            fm.startitem()
            if not ui.quiet:
                fm.plain(' %s ' % prefix, label=label)
            fm.write('bookmark', '%s', bmark, label=label)
            pad = " " * (25 - encoding.colwidth(bmark))
            rev = repo.changelog.rev(n)
            h = hexfn(n)
            fm.condwrite(not ui.quiet, 'rev node', pad + ' %d:%s', rev, h,
                         label=label)
            rname, distance = distancefromtracked(repo, bmark)
            if distance != (0, 0) and ui.verbose:
                pad = " " * (25 - encoding.colwidth(str(rev)) -
                             encoding.colwidth(str(h)))
                fm.write('bookmark', pad + ' [%s: %s ahead, %s behind]', rname,
                         *distance, label=label)
            fm.data(active=(bmark == current))
            fm.plain('\n')
        fm.end()

    if remote or opts.get('all'):
        n = 'remotebookmarks'
        if n not in repo.names:
            return
        ns = repo.names[n]
        color = ns.colorname
        label = 'log.' + color

        fm = ui.formatter('bookmarks', opts)

        # it seems overkill to hide displaying hidden remote bookmarks
        repo = repo.unfiltered()

        for name in sorted(ns.listnames(repo)):
            node = ns.nodes(repo, name)[0]
            ctx = repo[node]
            fm.startitem()

            if not ui.quiet:
                fm.plain('   ')

            padsize = max(25 - encoding.colwidth(name), 0)
            fmt = ' ' * padsize + ' %d:%s'

            tmplabel = label
            if ctx.obsolete():
                tmplabel = tmplabel + ' changeset.obsolete'
            fm.write(color, '%s', name, label=label)
            fm.condwrite(not ui.quiet, 'rev node', fmt, ctx.rev(),
                         fm.hexfunc(node), label=tmplabel)
            fm.plain('\n')
        fm.end()

def activepath(ui, remote):
    realpath = ''
    local = None
    try:
        local = remote.local()
    except AttributeError:
        pass

    # determine the remote path from the repo, if possible; else just
    # use the string given to us
    rpath = remote
    if local:
        rpath = getattr(remote, 'root', None)
        if rpath is None:
            # Maybe a localpeer? (hg@1ac628cd7113, 2.3)
            rpath = getattr(getattr(remote, '_repo', None),
                            'root', None)
    elif not isinstance(remote, str):
        try:
            rpath = remote._url
        except AttributeError:
            rpath = remote.url

    for path, uri in ui.configitems('paths'):
        uri = ui.expandpath(expandscheme(ui, uri))
        if local:
            uri = os.path.realpath(uri)
        else:
            if uri.startswith('http'):
                try:
                    uri = url.url(uri).authinfo()[0]
                except AttributeError:
                    try:
                        uri = util.url(uri).authinfo()[0]
                    except AttributeError:
                        uri = url.getauthinfo(uri)[0]
        uri = uri.rstrip('/')
        rpath = rpath.rstrip('/')
        if uri == rpath:
            realpath = path
            # prefer a non-default name to default
            if path != 'default' and path != 'default-push':
                break

    renames = _getrenames(ui)
    realpath = renames.get(realpath, realpath)
    return realpath

# memoization
_renames = None
def _getrenames(ui):
    global _renames
    if _renames is None:
        _renames = {}
        for k, v in ui.configitems('remotenames'):
            if k.startswith('rename.'):
                _renames[k[7:]] = v
    return _renames

def expandscheme(ui, uri):
    '''For a given uri, expand the scheme for it'''
    urischemes = [s for s in schemes.schemes.iterkeys()
                  if uri.startswith('%s://' % s)]
    for s in urischemes:
        # TODO: refactor schemes so we don't
        # duplicate this logic
        ui.note(_('performing schemes expansion with '
                  'scheme %s\n') % s)
        scheme = hg.schemes[s]
        parts = uri.split('://', 1)[1].split('/', scheme.parts)
        if len(parts) > scheme.parts:
            tail = parts[-1]
            parts = parts[:-1]
        else:
            tail = ''
        ctx = dict((str(i + 1), v) for i, v in enumerate(parts))
        uri = ''.join(scheme.templater.process(scheme.url, ctx)) + tail
    return uri

def splitremotename(remote):
    name = ''
    if '/' in remote:
        remote, name = remote.split('/', 1)
    return remote, name

def joinremotename(remote, ref):
    if ref:
        remote += '/' + ref
    return remote

def readremotenames(repo):
    rfile = repo.join('remotenames')
    # exit early if there is nothing to do
    if not os.path.exists(rfile):
        return

    # needed to heuristically determine if a file is in the old format
    branches = repo.names['branches'].listnames(repo)
    bookmarks = repo.names['bookmarks'].listnames(repo)

    f = open(rfile)
    for line in f:
        nametype = None
        line = line.strip()
        if not line:
            continue
        nametype = None
        remote, rname = None, None

        node, name = line.split(' ', 1)

        # check for nametype being written into the file format
        if ' ' in name:
            nametype, name = name.split(' ', 1)

        remote, rname = splitremotename(name)

        # skip old data that didn't write the name (only wrote the alias)
        if not rname:
            continue

        # old format didn't save the nametype, so check for the name in
        # branches and bookmarks
        if nametype is None:
            if rname in branches:
                nametype = 'branches'
            elif rname in bookmarks:
                nametype = 'bookmarks'

        yield node, nametype, remote, rname

    f.close()

def loadremotenames(repo):
    alias_default = repo.ui.configbool('remotenames', 'alias.default')

    for node, nametype, remote, rname in readremotenames(repo):
        # handle alias_default here
        if remote != "default" and rname == "default" and alias_default:
            name = remote
        else:
            name = joinremotename(remote, rname)

        # if the node doesn't exist, skip it
        try:
            ctx = repo[node]
        except error.RepoLookupError:
            continue

        # only mark as remote if the head changeset isn't marked closed
        if not ctx.extra().get('close'):
            nodes = _remotenames[nametype].get(name, [])
            nodes.append(ctx.node())
            _remotenames[nametype][name] = nodes

def transition(repo, ui):
    """
    Help with transitioning to using a remotenames workflow.

    Allows deleting matching local bookmarks defined in a config file:

    [remotenames]
    transitionbookmarks = master, stable
    """
    transmarks = ui.configlist('remotenames', 'transitionbookmarks')
    localmarks = repo._bookmarks
    for mark in transmarks:
        if mark in localmarks:
            del localmarks[mark]
    localmarks.write()

def saveremotenames(repo, remote, branches={}, bookmarks={}):
    # delete old files
    try:
        repo.vfs.unlink('remotedistance')
    except OSError, inst:
        if inst.errno != errno.ENOENT:
            raise

    if not repo.vfs.exists('remotenames'):
        transition(repo, repo.ui)

    # while we're removing old paths, also update _remotenames
    for btype, rmap in _remotenames.iteritems():
        for rname in rmap.copy():
            if remote == splitremotename(rname)[0]:
                del _remotenames[btype][rname]

    # read in all data first before opening file to write
    olddata = set(readremotenames(repo))

    f = repo.vfs('remotenames', 'w')

    # only update the given 'remote', so iterate over old data and re-save it
    for node, nametype, oldremote, rname in olddata:
        if oldremote != remote:
            n = joinremotename(oldremote, rname)
            f.write('%s %s %s\n' % (node, nametype, n))

    for branch, nodes in branches.iteritems():
        for n in nodes:
            rname = joinremotename(remote, branch)
            f.write('%s branches %s\n' % (hex(n), rname))
    for bookmark, n in bookmarks.iteritems():
        f.write('%s bookmarks %s\n' % (n, joinremotename(remote, bookmark)))
    f.close()

def distancefromtracked(repo, bookmark):
    """return the (ahead, behind) distance between the tracked names"""

    tracking = _readtracking(repo)
    remotename = ''
    distance = (0, 0)

    if bookmark and bookmark in tracking:
        remotename = tracking[bookmark]

    if not remotename:
        return (remotename, distance)

    # load the cache
    try:
        distance = repo.vfs.read('cache/tracking.%s' % bookmark).strip()
        return (remotename, [int(d) for d in distance.split(' ')])
    except IOError:
        pass

    if remotename in repo:
        rev1 = repo[bookmark].rev()
        rev2 = repo[remotename].rev()
        distance = (str(len(repo.revs('only(%d, %d)' % (rev1, rev2)))),
                    str(len(repo.revs('only(%d, %d)' % (rev2, rev1)))))
        # save in a cache
        repo.vfs.write('cache/tracking.%s' % bookmark, ' '.join(distance))
    return (remotename, distance)

def writedistance(repo):
    wlock = repo.wlock()
    try:
        for bmark, remotename in _readtracking(repo).iteritems():
            # delete the cache if it exists
            try:
                repo.vfs.unlink('cache/tracking.%s' % bmark)
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise

            try:
                distancefromtracked(repo, bmark)
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise

        # are we on a 'branch' but not at the head, i.e. is there a bookmark
        # that we are heading towards?
        try:
            repo.vfs.unlink('cache/tracking.current')
        except OSError, inst:
            if inst.errno != errno.ENOENT:
                raise

        try:
            revs = list(repo.revs('limit(.:: and bookmark() - ., 1)'))
            if revs:
                # if we are here then we have one or more bookmarks and we'll
                # pick the first one for now
                bmark = repo[revs[0]].bookmarks()[0]
                d = len(repo.revs('only(%d, .)' % revs[0]))
                repo.vfs.write('cache/tracking.current', '%s %d' % (bmark, d))
        except OSError, inst:
            if inst.errno != errno.ENOENT:
                raise

    finally:
        wlock.release()

#########
# revsets
#########

def upstream_revs(filt, repo, subset, x):
    upstream_tips = set()
    for remotename in _remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            if filt(splitremotename(name)[0]):
                upstream_tips.update(ns.nodes(repo, name))

    if not upstream_tips:
        return revset.baseset([])

    tipancestors = repo.revs('::%ln', upstream_tips)
    return revset.filteredset(subset, lambda n: n in tipancestors)

def upstream(repo, subset, x):
    '''``upstream()``
    Select changesets in an upstream repository according to remotenames.
    '''
    repo = repo.unfiltered()
    upstream_names = repo.ui.configlist('remotenames', 'upstream')
    # override default args from hgrc with args passed in on the command line
    if x:
        upstream_names = [revset.getstring(symbol,
                                           "remote path must be a string")
                          for symbol in revset.getlist(x)]
    filt = lambda x: True
    default_path = dict(repo.ui.configitems('paths')).get('default')
    if not upstream_names and default_path:
        default_path = expandscheme(repo.ui, default_path)
        upstream_names = [activepath(repo.ui, default_path)]
    if upstream_names:
        filt = lambda name: name in upstream_names
    return upstream_revs(filt, repo, subset, x)

def pushed(repo, subset, x):
    '''``pushed()``
    Select changesets in any remote repository according to remotenames.
    '''
    revset.getargs(x, 0, 0, "pushed takes no arguments")
    return upstream_revs(lambda x: True, repo, subset, x)

def remotenamesrevset(repo, subset, x):
    """``remotenames()``
    All remote branches heads.
    """
    revset.getargs(x, 0, 0, "remotenames takes no arguments")
    remoterevs = set()
    cl = repo.changelog
    for remotename in _remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            remoterevs.update(ns.nodes(repo, name))

    return revset.baseset(sorted(cl.rev(n) for n in remoterevs))

revset.symbols.update({'upstream': upstream,
                       'pushed': pushed,
                       'remotenames': remotenamesrevset})

###########
# templates
###########

def remotenameskw(**args):
    """:remotenames: List of strings. List of remote names associated with the
    changeset. If remotenames.suppressbranches is True then branch names will
    be hidden if there is a bookmark at the same changeset.

    """
    repo, ctx = args['repo'], args['ctx']

    remotenames = []
    if 'remotebookmarks' in repo.names:
        remotenames = repo.names['remotebookmarks'].names(repo, ctx.node())

    suppress = repo.ui.configbool('remotenames', 'suppressbranches', False)
    if (not remotenames or not suppress) and 'remotebranches' in repo.names:
        remotenames += repo.names['remotebranches'].names(repo, ctx.node())

    return templatekw.showlist('remotename', remotenames,
                               plural='remotenames', **args)
