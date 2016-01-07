from hgext import rebase as hgrebase

from mercurial import cmdutil
from mercurial import discovery
try:
    from mercurial import exchange
    exchange.push  # existed in first iteration of this file
except ImportError:
    # We only *use* the exchange module in hg 3.2+, so this is safe
    pass
from mercurial import patch
from mercurial import hg
from mercurial import util as hgutil
from mercurial import node
from mercurial import i18n
from mercurial import extensions
from mercurial import repair
from mercurial import revset
from mercurial import scmutil

import layouts
import os
import replay
import pushmod
import stupid as stupidmod
import svnwrap
import svnrepo
import util

try:
    from mercurial import obsolete
except ImportError:
    obsolete = None

pullfuns = {
    True: replay.convert_rev,
    False: stupidmod.convert_rev,
}

revmeta = [
    ('revision', 'revnum'),
    ('user', 'author'),
    ('date', 'date'),
    ('message', 'message'),
]


def version(orig, ui, *args, **opts):
    svn = opts.pop('svn', None)
    orig(ui, *args, **opts)
    if svn:
        svnversion, bindings = svnwrap.version()
        ui.status('\n')
        ui.status('hgsubversion: %s\n' % util.version(ui))
        ui.status('Subversion: %s\n' % svnversion)
        ui.status('bindings: %s\n' % bindings)


def parents(orig, ui, repo, *args, **opts):
    """show Mercurial & Subversion parents of the working dir or revision
    """
    if not opts.get('svn', False):
        return orig(ui, repo, *args, **opts)
    meta = repo.svnmeta()
    hashes = meta.revmap.hashes()
    ha = util.parentrev(ui, repo, meta, hashes)
    if ha.node() == node.nullid:
        raise hgutil.Abort('No parent svn revision!')
    displayer = cmdutil.show_changeset(ui, repo, opts, buffered=False)
    displayer.show(ha)
    return 0


def getpeer(ui, opts, source):
    # Since 2.3 (1ac628cd7113)
    peer = getattr(hg, 'peer', None)
    if peer:
        return peer(ui, opts, source)
    return hg.repository(ui, source)

def getlocalpeer(ui, opts, source):
    peer = getpeer(ui, opts, source)
    repo = getattr(peer, 'local', lambda: peer)()
    if isinstance(repo, bool):
        repo = peer
    return repo

def getcaps(other):
    return (getattr(other, 'caps', None) or
            getattr(other, 'capabilities', None) or set())


def incoming(orig, ui, repo, origsource='default', **opts):
    """show incoming revisions from Subversion
    """

    source, revs, checkout = util.parseurl(ui.expandpath(origsource))
    other = getpeer(ui, opts, source)
    if 'subversion' not in getcaps(other):
        return orig(ui, repo, origsource, **opts)

    svn = other.svn
    meta = repo.svnmeta(svn.uuid, svn.subdir)

    ui.status('incoming changes from %s\n' % other.svnurl)
    svnrevisions = list(svn.revisions(start=meta.lastpulled))
    if opts.get('newest_first'):
        svnrevisions.reverse()
    # Returns 0 if there are incoming changes, 1 otherwise.
    if len(svnrevisions) > 0:
        ret = 0
    else:
        ret = 1
    for r in svnrevisions:
        ui.status('\n')
        for label, attr in revmeta:
            l1 = label + ':'
            val = str(getattr(r, attr)).strip()
            if not ui.verbose:
                val = val.split('\n')[0]
            ui.status('%s%s\n' % (l1.ljust(13), val))
    return ret


def findcommonoutgoing(repo, other, onlyheads=None, force=False,
                       commoninc=None, portable=False):
    assert other.capable('subversion')
    # split off #rev; TODO implement --revision/#rev support
    svn = other.svn
    meta = repo.svnmeta(svn.uuid, svn.subdir)
    parent = repo.parents()[0].node()
    hashes = meta.revmap.hashes()
    common, heads = util.outgoing_common_and_heads(repo, hashes, parent)
    outobj = getattr(discovery, 'outgoing', None)
    if outobj is not None:
        # Mercurial 2.1 and later
        return outobj(repo.changelog, common, heads)
    # Mercurial 2.0 and earlier
    return common, heads


def findoutgoing(repo, dest=None, heads=None, force=False):
    """show changesets not found in the Subversion repository
    """
    assert dest.capable('subversion')
    # split off #rev; TODO implement --revision/#rev support
    # svnurl, revs, checkout = util.parseurl(dest.svnurl, heads)
    svn = dest.svn
    meta = repo.svnmeta(svn.uuid, svn.subdir)
    parent = repo.parents()[0].node()
    hashes = meta.revmap.hashes()
    return util.outgoing_revisions(repo, hashes, parent)


def diff(orig, ui, repo, *args, **opts):
    """show a diff of the most recent revision against its parent from svn
    """
    if not opts.get('svn', False) or opts.get('change', None):
        return orig(ui, repo, *args, **opts)
    meta = repo.svnmeta()
    hashes = meta.revmap.hashes()
    if not opts.get('rev', None):
        parent = repo.parents()[0]
        o_r = util.outgoing_revisions(repo, hashes, parent.node())
        if o_r:
            parent = repo[o_r[-1]].parents()[0]
        opts['rev'] = ['%s:.' % node.hex(parent.node()), ]
    node1, node2 = scmutil.revpair(repo, opts['rev'])
    baserev, _junk = hashes.get(node1, (-1, 'junk'))
    newrev, _junk = hashes.get(node2, (-1, 'junk'))
    it = patch.diff(repo, node1, node2,
                    opts=patch.diffopts(ui, opts={'git': True,
                                                  'show_function': False,
                                                  'ignore_all_space': False,
                                                  'ignore_space_change': False,
                                                  'ignore_blank_lines': False,
                                                  'unified': True,
                                                  'text': False,
                                                  }))
    ui.write(util.filterdiff(''.join(it), baserev, newrev))

def push(repo, dest, force, revs):
    """push revisions starting at a specified head back to Subversion.
    """
    assert not revs, 'designated revisions for push remains unimplemented.'
    cmdutil.bailifchanged(repo)
    checkpush = getattr(repo, 'checkpush', None)
    if checkpush:
        try:
            # The checkpush function changed as of e10000369b47 (first
            # in 3.0) in mercurial
            from mercurial.exchange import pushoperation
            pushop = pushoperation(repo, dest, force, revs, False)
            checkpush(pushop)
        except (ImportError, TypeError):
            checkpush(force, revs)

    ui = repo.ui
    old_encoding = util.swap_out_encoding()

    try:
        hasobsolete = obsolete._enabled
    except:
        hasobsolete = False

    temporary_commits = []
    try:
        # TODO: implement --rev/#rev support
        # TODO: do credentials specified in the URL still work?
        svn = dest.svn
        meta = repo.svnmeta(svn.uuid, svn.subdir)

        # Strategy:
        # 1. Find all outgoing commits from this head
        if len(repo.parents()) != 1:
            ui.status('Cowardly refusing to push branch merge\n')
            return 0 # results in nonzero exit status, see hg's commands.py
        workingrev = repo[None].parents()[0]
        workingbranch = workingrev.branch()
        ui.status('searching for changes\n')
        hashes = meta.revmap.hashes()
        outgoing = util.outgoing_revisions(repo, hashes, workingrev.node())
        if not (outgoing and len(outgoing)):
            ui.status('no changes found\n')
            return 1 # so we get a sane exit status, see hg's commands.push

        tip_ctx = repo[outgoing[-1]].p1()
        svnbranch = tip_ctx.branch()
        modified_files = {}
        for i in range(len(outgoing) - 1, -1, -1):
            # 2. Pick the oldest changeset that needs to be pushed
            current_ctx = repo[outgoing[i]]
            original_ctx = current_ctx

            if len(current_ctx.parents()) != 1:
                ui.status('Found a branch merge, this needs discussion and '
                          'implementation.\n')
                # results in nonzero exit status, see hg's commands.py
                return 0

            # 3. Move the changeset to the tip of the branch if necessary
            conflicts = False
            for file in current_ctx.files():
                if file in modified_files:
                    conflicts = True
                    break

            if conflicts or current_ctx.branch() != svnbranch:
                util.swap_out_encoding(old_encoding)
                try:
                    def extrafn(ctx, extra):
                        extra['branch'] = ctx.branch()

                    ui.note('rebasing %s onto %s \n' % (current_ctx, tip_ctx))
                    hgrebase.rebase(ui, repo,
                                    dest=node.hex(tip_ctx.node()),
                                    rev=[node.hex(current_ctx.node())],
                                    extrafn=extrafn, keep=True)
                finally:
                    util.swap_out_encoding()

                # Don't trust the pre-rebase repo and context.
                repo = getlocalpeer(ui, {}, meta.path)
                meta = repo.svnmeta(svn.uuid, svn.subdir)
                hashes = meta.revmap.hashes()
                tip_ctx = repo[tip_ctx.node()]
                for c in tip_ctx.descendants():
                    rebasesrc = c.extra().get('rebase_source')
                    if rebasesrc and node.bin(rebasesrc) == current_ctx.node():
                        current_ctx = c
                        temporary_commits.append(c.node())
                        break

            # 4. Push the changeset to subversion
            tip_hash = hashes[tip_ctx.node()][0]
            try:
                ui.status('committing %s\n' % current_ctx)
                pushedrev = pushmod.commit(ui, repo, current_ctx, meta,
                                           tip_hash, svn)
            except pushmod.NoFilesException:
                ui.warn("Could not push revision %s because it had no changes "
                        "in svn.\n" % current_ctx)
                return

            # This hook is here purely for testing.  It allows us to
            # onsistently trigger hit the race condition between
            # pushing and pulling here.  In particular, we use it to
            # trigger another revision landing between the time we
            # push a revision and pull it back.
            repo.hook('debug-hgsubversion-between-push-and-pull-for-tests')

            # 5. Pull the latest changesets from subversion, which will
            # include the one we just committed (and possibly others).
            r = pull(repo, dest, force=force, meta=meta)
            assert not r or r == 0

            # 6. Move our tip to the latest pulled tip
            for c in tip_ctx.descendants():
                if c.node() in hashes and c.branch() == svnbranch:
                    if meta.get_source_rev(ctx=c)[0] == pushedrev.revnum:
                        # This is corresponds to the changeset we just pushed
                        if hasobsolete:
                            ui.note('marking %s as obsoleted by %s\n' %
                                    (original_ctx.hex(), c.hex()))
                            obsolete.createmarkers(repo, [(original_ctx, [c])])

                    tip_ctx = c

                    # Remember what files have been modified since the
                    # whole push started.
                    for file in c.files():
                        modified_files[file] = True

            # 7. Rebase any children of the commit we just pushed
            # that are not in the outgoing set
            for c in original_ctx.children():
                if not c.node() in hashes and not c.node() in outgoing:
                    util.swap_out_encoding(old_encoding)
                    try:
                        # Path changed as subdirectories were getting
                        # deleted during push.
                        saved_path = os.getcwd()
                        os.chdir(repo.root)

                        def extrafn(ctx, extra):
                            extra['branch'] = ctx.branch()

                        ui.status('rebasing non-outgoing %s onto %s\n' % (c, tip_ctx))
                        needs_rebase_set = "%s::" % node.hex(c.node())
                        hgrebase.rebase(ui, repo,
                                        dest=node.hex(tip_ctx.node()),
                                        rev=[needs_rebase_set],
                                        extrafn=extrafn,
                                        keep=not hasobsolete)
                    finally:
                        os.chdir(saved_path)
                        util.swap_out_encoding()


        util.swap_out_encoding(old_encoding)
        try:
            hg.update(repo, repo.branchtip(workingbranch))
        finally:
            util.swap_out_encoding()

        if not hasobsolete:
            # strip the original changesets since the push was
            # successful and changeset obsolescence is unavailable
            util.strip(ui, repo, outgoing, "all")
    finally:
        try:
            # It's always safe to delete the temporary commits.
            # The originals are not deleted unless the push
            # completely succeeded.
            if temporary_commits:
                # If the repo is on a temporary commit, get off before
                # the strip.
                parent = repo[None].p1()
                if parent.node() in temporary_commits:
                    hg.update(repo, parent.p1().node())
                if hasobsolete:
                    relations = ((repo[n], ()) for n in temporary_commits)
                    obsolete.createmarkers(repo, relations)
                else:
                    util.strip(ui, repo, temporary_commits, backup=None)

        finally:
            util.swap_out_encoding(old_encoding)
    return 1 # so we get a sane exit status, see hg's commands.push

def exchangepush(orig, repo, remote, force=False, revs=None, newbranch=False,
                 bookmarks=(), **kwargs):
    capable = getattr(remote, 'capable', lambda x: False)
    if capable('subversion'):
        pushop = exchange.pushoperation(repo, remote, force, revs, newbranch,
                                        bookmarks=bookmarks)
        pushop.cgresult = push(repo, remote, force, revs)
        return pushop
    else:
        return orig(repo, remote, force, revs, newbranch, bookmarks=bookmarks,
                    **kwargs)

def pull(repo, source, heads=[], force=False, meta=None):
    """pull new revisions from Subversion"""
    assert source.capable('subversion')
    svn_url = source.svnurl

    # Split off #rev
    svn_url, heads, checkout = util.parseurl(svn_url, heads)
    old_encoding = util.swap_out_encoding()
    total = None
    try:
        have_replay = not repo.ui.configbool('hgsubversion', 'stupid')
        if not have_replay:
            repo.ui.note('fetching stupidly...\n')

        svn = source.svn
        if meta is None:
            meta = repo.svnmeta(svn.uuid, svn.subdir)

        stopat_rev = util.parse_revnum(svn, checkout)

        if meta.layout == 'auto':
            meta.layout = meta.layout_from_subversion(svn, (stopat_rev or None))
            repo.ui.note('using %s layout\n' % meta.layout)

        if meta.branch:
            if meta.layout != 'single':
                msg = ('branch cannot be specified for Subversion clones using '
                       'standard directory layout')
                raise hgutil.Abort(msg)

            meta.branchmap['default'] = meta.branch

        ui = repo.ui
        start = meta.lastpulled
        origrevcount = len(meta.revmap)

        if start <= 0:
            # we are initializing a new repository
            start = util.parse_revnum(svn, repo.ui.config('hgsubversion',
                                                          'startrev', 0))

            if start > 0:
                if meta.layout == 'standard':
                    raise hgutil.Abort('non-zero start revisions are only '
                                       'supported for single-directory clones.')
                ui.note('starting at revision %d; any prior will be ignored\n'
                        % start)
                # fetch all revisions *including* the one specified...
                start -= 1

            # anything less than zero makes no sense
            if start < 0:
                start = 0

        skiprevs = repo.ui.configlist('hgsubversion', 'unsafeskip', '')
        skiprevs = set(util.parse_revnum(svn, r) for r in skiprevs)

        oldrevisions = len(meta.revmap)
        if stopat_rev:
            total = stopat_rev - start
        else:
            total = svn.HEAD - start
        lastpulled = None

        try:
            # start converting revisions
            firstrun = True
            for r in svn.revisions(start=start, stop=stopat_rev):
                if (r.revnum in skiprevs or
                    (r.author is None and
                     r.message == 'This is an empty revision for padding.')):
                    lastpulled = r.revnum
                    continue
                tbdelta = meta.update_branch_tag_map_for_rev(r)
                # got a 502? Try more than once!
                tries = 0
                converted = False
                while not converted:
                    try:
                        msg = meta.getmessage(r).strip()
                        if msg:
                            msg = [s.strip() for s in msg.splitlines() if s][0]
                        if getattr(ui, 'termwidth', False):
                            w = ui.termwidth()
                        else:
                            w = hgutil.termwidth()
                        bits = (r.revnum, r.author, msg)
                        ui.status(('[r%d] %s: %s' % bits)[:w] + '\n')
                        ui.progress('pull', r.revnum - start, total=total)

                        meta.save_tbdelta(tbdelta)
                        close = pullfuns[have_replay](ui, meta, svn, r, tbdelta,
                                                      firstrun)
                        meta.committags(r, close)
                        for branch, parent in close.iteritems():
                            if parent in (None, node.nullid):
                                continue
                            meta.delbranch(branch, parent, r)

                        meta.save()
                        converted = True
                        firstrun = False

                    except svnwrap.SubversionRepoCanNotReplay, e: # pragma: no cover
                        ui.status('%s\n' % e.message)
                        stupidmod.print_your_svn_is_old_message(ui)
                        have_replay = False
                    except svnwrap.SubversionException, e: # pragma: no cover
                        if (e.args[1] == svnwrap.ERR_RA_DAV_REQUEST_FAILED
                            and '502' in str(e)
                            and tries < 3):
                            tries += 1
                            ui.status('Got a 502, retrying (%s)\n' % tries)
                        else:
                            ui.traceback()
                            raise hgutil.Abort(*e.args)

                lastpulled = r.revnum

        except KeyboardInterrupt:
            ui.traceback()
    finally:
        if total is not None:
            ui.progress('pull', None, total=total)
        util.swap_out_encoding(old_encoding)

    if lastpulled is not None:
        meta.lastpulled = lastpulled
    revisions = len(meta.revmap) - oldrevisions

    if revisions == 0:
        ui.status(i18n._("no changes found\n"))
        return 0
    else:
        ui.status("pulled %d revisions\n" % revisions)

def exchangepull(orig, repo, remote, heads=None, force=False, bookmarks=(),
                 **kwargs):
    capable = getattr(remote, 'capable', lambda x: False)
    if capable('subversion'):
        # transaction manager is present in Mercurial >= 3.3
        try:
            trmanager = getattr(exchange, 'transactionmanager')
        except AttributeError:
            trmanager = None
        pullop = exchange.pulloperation(repo, remote, heads, force,
                                        bookmarks=bookmarks)
        if trmanager:
            pullop.trmanager = trmanager(repo, 'pull', remote.url())
        try:
            pullop.cgresult = pull(repo, remote, heads, force)
            return pullop
        finally:
            if trmanager:
                pullop.trmanager.release()
            else:
                pullop.releasetransaction()
    else:
        return orig(repo, remote, heads, force, bookmarks=bookmarks, **kwargs)

def rebase(orig, ui, repo, **opts):
    """rebase current unpushed revisions onto the Subversion head

    This moves a line of development from making its own head to the top of
    Subversion development, linearizing the changes. In order to make sure you
    rebase on top of the current top of Subversion work, you should probably run
    'hg svn pull' before running this.

    Also looks for svnextrafn and svnsourcerev in **opts.
    """
    if not opts.get('svn', False):
        return orig(ui, repo, **opts)
    def extrafn2(ctx, extra):
        """defined here so we can add things easily.
        """
        extra['branch'] = ctx.branch()
    extrafn = opts.get('svnextrafn', extrafn2)
    sourcerev = opts.get('svnsourcerev', repo.parents()[0].node())
    meta = repo.svnmeta()
    hashes = meta.revmap.hashes()
    o_r = util.outgoing_revisions(repo, hashes, sourcerev=sourcerev)
    if not o_r:
        ui.note('nothing to rebase\n')
        return 0
    if len(repo[sourcerev].children()):
        ui.status('refusing to rebase non-head commit like a coward\n')
        return 0
    parent_rev = repo[o_r[-1]].parents()[0]
    target_rev = parent_rev
    p_n = parent_rev.node()
    exhausted_choices = False
    while target_rev.children() and not exhausted_choices:
        for c in target_rev.children():
            exhausted_choices = True
            n = c.node()
            if (n in hashes and hashes[n][1] == hashes[p_n][1]):
                target_rev = c
                exhausted_choices = False
                break
    if parent_rev == target_rev:
        ui.status('already up to date!\n')
        return 0
    return orig(ui, repo, dest=node.hex(target_rev.node()),
                base=node.hex(sourcerev),
                extrafn=extrafn)


optionmap = {
    'tagpaths': ('hgsubversion', 'tagpaths'),
    'authors': ('hgsubversion', 'authormap'),
    'branchdir': ('hgsubversion', 'branchdir'),
    'trunkdir': ('hgsubversion', 'trunkdir'),
    'infix': ('hgsubversion', 'infix'),
    'filemap': ('hgsubversion', 'filemap'),
    'branchmap': ('hgsubversion', 'branchmap'),
    'tagmap': ('hgsubversion', 'tagmap'),
    'stupid': ('hgsubversion', 'stupid'),
    'defaulthost': ('hgsubversion', 'defaulthost'),
    'defaultauthors': ('hgsubversion', 'defaultauthors'),
    'usebranchnames': ('hgsubversion', 'usebranchnames'),
    'layout': ('hgsubversion', 'layout'),
    'startrev': ('hgsubversion', 'startrev'),
}

extrasections = set(['hgsubversionbranch'])


dontretain = {
    'hgsubversion': set(['authormap', 'filemap', 'layout', ]),
    'hgsubversionbranch': set(),
    }

def clone(orig, ui, source, dest=None, **opts):
    """
    Some of the options listed below only apply to Subversion
    %(target)s. See 'hg help %(extension)s' for more information on
    them as well as other ways of customising the conversion process.
    """

    data = {}
    def hgclonewrapper(orig, ui, *args, **opts):
        origsource = args[1]

        if isinstance(origsource, str):
            source, branch, checkout = util.parseurl(ui.expandpath(origsource),
                                         opts.get('branch'))
            srcrepo = getpeer(ui, opts, source)
        else:
            srcrepo = origsource

        if srcrepo.capable('subversion'):
            branches = opts.pop('branch', None)
            if branches:
                data['branches'] = branches
                ui.setconfig('hgsubversion', 'branch', branches[-1])

        data['srcrepo'], data['dstrepo'] = orig(ui, *args, **opts)

        return data['srcrepo'], data['dstrepo']

    for opt, (section, name) in optionmap.iteritems():
        if opt in opts and opts[opt]:
            ui.setconfig(section, name, str(opts.pop(opt)))

    # calling hg.clone directoly to get the repository instances it returns,
    # breaks in subtle ways, so we double-wrap
    orighgclone = extensions.wrapfunction(hg, 'clone', hgclonewrapper)
    orig(ui, source, dest, **opts)
    hg.clone = orighgclone

    # do this again; the ui instance isn't shared between the wrappers
    if data.get('branches'):
        ui.setconfig('hgsubversion', 'branch', data['branches'][-1])

    dstrepo = data.get('dstrepo')
    srcrepo = data.get('srcrepo')
    dst = dstrepo.local()

    if dstrepo.local() and srcrepo.capable('subversion'):
        dst = dstrepo.local()
        fd = dst.opener("hgrc", "a", text=True)
        preservesections = set(s for s, v in optionmap.itervalues())
        preservesections |= extrasections
        for section in preservesections:
            config = dict(ui.configitems(section))
            for name in dontretain[section]:
                config.pop(name, None)

            if config:
                fd.write('\n[%s]\n' % section)
                map(fd.write, ('%s = %s\n' % p for p in config.iteritems()))


def generic(orig, ui, repo, *args, **opts):
    """
    Subversion %(target)s can be used for %(command)s. See 'hg help
    %(extension)s' for more on the conversion process.
    """

    branch = opts.get('branch', None)
    if branch:
        ui.setconfig('hgsubversion', 'branch', branch[-1])

    for opt, (section, name) in optionmap.iteritems():
        if opt in opts and opts[opt]:
            if isinstance(repo, str):
                ui.setconfig(section, name, opts.pop(opt))
            else:
                repo.ui.setconfig(section, name, opts.pop(opt))
    return orig(ui, repo, *args, **opts)
