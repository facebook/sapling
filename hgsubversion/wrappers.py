from hgext import rebase as hgrebase

from mercurial import cmdutil
from mercurial import patch
from mercurial import hg
from mercurial import util as hgutil
from mercurial import node
from mercurial import i18n

import replay
import pushmod
import stupid as stupidmod
import svnwrap
import svnrepo
import util

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


def incoming(orig, ui, repo, source='default', **opts):
    """show incoming revisions from Subversion
    """

    source, revs, checkout = util.parseurl(ui.expandpath(source))
    other = hg.repository(ui, source)
    if 'subversion' not in other.capabilities:
        return orig(ui, repo, source, **opts)

    meta = repo.svnmeta()

    ui.status('incoming changes from %s\n' % other.svnurl)
    for r in other.svn.revisions(start=meta.revmap.youngest):
        ui.status('\n')
        for label, attr in revmeta:
            l1 = label + ':'
            val = str(getattr(r, attr)).strip()
            if not ui.verbose:
                val = val.split('\n')[0]
            ui.status('%s%s\n' % (l1.ljust(13), val))


def outgoing(repo, dest=None, heads=None, force=False):
    """show changesets not found in the Subversion repository
    """
    assert dest.capable('subversion')

    # split off #rev; TODO implement --revision/#rev support
    svnurl, revs, checkout = util.parseurl(dest.svnurl, heads)
    meta = repo.svnmeta()
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
    node1, node2 = cmdutil.revpair(repo, opts['rev'])
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
    cmdutil.bail_if_changed(repo)
    ui = repo.ui
    old_encoding = util.swap_out_encoding()
    # TODO: implement --rev/#rev support
    # TODO: do credentials specified in the URL still work?
    svnurl = repo.ui.expandpath(dest.svnurl)
    svn = dest.svn
    meta = repo.svnmeta(svn.uuid)

    # Strategy:
    # 1. Find all outgoing commits from this head
    if len(repo.parents()) != 1:
        ui.status('Cowardly refusing to push branch merge\n')
        return 0 # results in nonzero exit status, see hg's commands.py
    workingrev = repo.parents()[0]
    ui.status('searching for changes\n')
    hashes = meta.revmap.hashes()
    outgoing = util.outgoing_revisions(repo, hashes, workingrev.node())
    if not (outgoing and len(outgoing)):
        ui.status('no changes found\n')
        return 1 # so we get a sane exit status, see hg's commands.push
    while outgoing:

        # 2. Commit oldest revision that needs to be pushed
        oldest = outgoing.pop(-1)
        old_ctx = repo[oldest]
        old_pars = old_ctx.parents()
        if len(old_pars) != 1:
            ui.status('Found a branch merge, this needs discussion and '
                      'implementation.\n')
            return 0 # results in nonzero exit status, see hg's commands.py
        # We will commit to svn against this node's parent rev. Any file-level
        # conflicts here will result in an error reported by svn.
        base_ctx = old_pars[0]
        base_revision = hashes[base_ctx.node()][0]
        svnbranch = base_ctx.branch()
        # Find most recent svn commit we have on this branch.
        # This node will become the nearest known ancestor of the pushed rev.
        oldtipctx = base_ctx
        old_children = oldtipctx.descendants()
        seen = set(c.node() for c in old_children)
        samebranchchildren = [c for c in old_children if c.branch() == svnbranch
                              and c.node() in hashes]
        if samebranchchildren:
            # The following relies on descendants being sorted by rev.
            oldtipctx = samebranchchildren[-1]
        # All set, so commit now.
        try:
            pushmod.commit(ui, repo, old_ctx, meta, base_revision, svn)
        except pushmod.NoFilesException:
            ui.warn("Could not push revision %s because it had no changes in svn.\n" %
                     old_ctx)
            return 1

        # 3. Fetch revisions from svn
        # TODO: this probably should pass in the source explicitly - rev too?
        r = repo.pull(dest, force=force)
        assert not r or r == 0

        # 4. Find the new head of the target branch
        # We expect to get our own new commit back, but we might also get other
        # commits that happened since our last pull, or even right after our own
        # commit (race).
        for c in oldtipctx.descendants():
            if c.node() not in seen and c.branch() == svnbranch:
                newtipctx = c

        # 5. Rebase all children of the currently-pushing rev to the new head
        heads = repo.heads(old_ctx.node())
        for needs_transplant in heads:
            def extrafn(ctx, extra):
                if ctx.node() == oldest:
                    return
                extra['branch'] = ctx.branch()
            # TODO: can we avoid calling our own rebase wrapper here?
            rebase(hgrebase.rebase, ui, repo, svn=True, svnextrafn=extrafn,
                   svnsourcerev=needs_transplant)
            # Reload the repo after the rebase. Do not reuse contexts across this.
            newtip = newtipctx.node()
            repo = hg.repository(ui, meta.path)
            newtipctx = repo[newtip]
            # Rewrite the node ids in outgoing to their rebased versions.
            rebasemap = dict()
            for child in newtipctx.descendants():
                rebasesrc = child.extra().get('rebase_source')
                if rebasesrc:
                    rebasemap[node.bin(rebasesrc)] = child.node()
            outgoing = [rebasemap.get(n) or n for n in outgoing]
        # TODO: stop constantly creating the SVNMeta instances.
        meta = repo.svnmeta(svn.uuid)
        hashes = meta.revmap.hashes()
    util.swap_out_encoding(old_encoding)
    return 1 # so we get a sane exit status, see hg's commands.push


def pull(repo, source, heads=[], force=False):
    """pull new revisions from Subversion"""
    assert source.capable('subversion')
    svn_url = source.svnurl

    # Split off #rev
    svn_url, heads, checkout = util.parseurl(svn_url, heads)
    old_encoding = util.swap_out_encoding()

    try:
        stopat_rev = int(checkout or 0)
    except ValueError:
        raise hgutil.Abort('unrecognised Subversion revision %s: '
                           'only numbers work.' % checkout)

    have_replay = not repo.ui.configbool('hgsubversion', 'stupid')
    if not have_replay:
        repo.ui.note('fetching stupidly...\n')

    svn = source.svn
    meta = repo.svnmeta(svn.uuid, svn.subdir)

    layout = repo.ui.config('hgsubversion', 'layout', 'auto')
    if layout == 'auto':
        rootlist = svn.list_dir('', revision=(stopat_rev or None))
        if sum(map(lambda x: x in rootlist, ('branches', 'tags', 'trunk'))):
            layout = 'standard'
        else:
            layout = 'single'
        repo.ui.setconfig('hgsubversion', 'layout', layout)
        repo.ui.note('using %s layout\n' % layout)

    branch = repo.ui.config('hgsubversion', 'branch')
    if branch:
        if layout != 'single':
            msg = ('branch cannot be specified for Subversion clones using '
                   'standard directory layout')
            raise hgutil.Abort(msg)

        meta.branchmap['default'] = branch

    ui = repo.ui
    start = meta.revmap.youngest
    origrevcount = len(meta.revmap)

    if start <= 0:
        # we are initializing a new repository
        start = repo.ui.config('hgsubversion', 'startrev', 0)
        if isinstance(start, str) and start.upper() == 'HEAD':
            start = svn.last_changed_rev
        else:
            start = int(start)

        if start > 0:
            if layout == 'standard':
                raise hgutil.Abort('non-zero start revisions are only '
                                   'supported for single-directory clones.')
            ui.note('starting at revision %d; any prior will be ignored\n'
                    % start)
            # fetch all revisions *including* the one specified...
            start -= 1

        # anything less than zero makes no sense
        if start < 0:
            start = 0

    oldrevisions = len(meta.revmap)
    if stopat_rev:
        total = stopat_rev - start
    else:
        total = svn.HEAD - start
    try:
        try:
            # start converting revisions
            for r in svn.revisions(start=start, stop=stopat_rev):
                if (r.author is None and
                    r.message == 'This is an empty revision for padding.'):
                    continue
                tbdelta = meta.update_branch_tag_map_for_rev(r)
                # got a 502? Try more than once!
                tries = 0
                converted = False
                while not converted:
                    try:
                        msg = ''
                        if r.message:
                            msg = r.message.strip()
                        if not msg:
                            msg = util.default_commit_msg
                        else:
                            msg = [s.strip() for s in msg.splitlines() if s][0]
                        w = hgutil.termwidth()
                        bits = (r.revnum, r.author, msg)
                        ui.status(('[r%d] %s: %s' % bits)[:w] + '\n')
                        util.progress(ui, 'pull', r.revnum - start, total=total)

                        meta.save_tbdelta(tbdelta)
                        close = pullfuns[have_replay](ui, meta, svn, r, tbdelta)
                        meta.committags(r, close)
                        for branch, parent in close.iteritems():
                            if parent in (None, node.nullid):
                                continue
                            meta.delbranch(branch, parent, r)

                        meta.save()
                        converted = True

                    except svnwrap.SubversionRepoCanNotReplay, e: #pragma: no cover
                        ui.status('%s\n' % e.message)
                        stupidmod.print_your_svn_is_old_message(ui)
                        have_replay = False
                    except svnwrap.SubversionException, e: #pragma: no cover
                        if (e.args[1] == svnwrap.ERR_RA_DAV_REQUEST_FAILED
                            and '502' in str(e)
                            and tries < 3):
                            tries += 1
                            ui.status('Got a 502, retrying (%s)\n' % tries)
                        else:
                            raise hgutil.Abort(*e.args)
        except KeyboardInterrupt:
            pass
    finally:
        util.progress(ui, 'pull', None, total=total)
        util.swap_out_encoding(old_encoding)

    revisions = len(meta.revmap) - oldrevisions

    if revisions == 0:
        ui.status(i18n._("no changes found\n"))
        return 0
    else:
        ui.status("pulled %d revisions\n" % revisions)

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
        ui.status('Nothing to rebase!\n')
        return 0
    if len(repo[sourcerev].children()):
        ui.status('Refusing to rebase non-head commit like a coward\n')
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
        ui.status('Already up to date!\n')
        return 0
    return orig(ui, repo, dest=node.hex(target_rev.node()),
                base=node.hex(sourcerev),
                extrafn=extrafn)


optionmap = {
    'tagpaths': ('hgsubversion', 'tagpaths'),
    'authors': ('hgsubversion', 'authormap'),
    'filemap': ('hgsubversion', 'filemap'),
    'branchmap': ('hgsubversion', 'branchmap'),
    'stupid': ('hgsubversion', 'stupid'),
    'defaulthost': ('hgsubversion', 'defaulthost'),
    'defaultauthors': ('hgsubversion', 'defaultauthors'),
    'usebranchnames': ('hgsubversion', 'usebranchnames'),
    'layout': ('hgsubversion', 'layout'),
    'startrev': ('hgsubversion', 'startrev'),
}

dontretain = { 'hgsubversion': set(['authormap', 'filemap', 'layout', ]) }

def clone(orig, ui, source, dest=None, **opts):
    """
    Some of the options listed below only apply to Subversion
    %(target)s. See 'hg help %(extension)s' for more information on
    them as well as other ways of customising the conversion process.
    """

    branch = opts.get('branch', None)
    if branch:
        ui.setconfig('hgsubversion', 'branch', branch[-1])

    for opt, (section, name) in optionmap.iteritems():
        if opt in opts and opts[opt]:
            ui.setconfig(section, name, str(opts.pop(opt)))

    # this must be kept in sync with mercurial/commands.py
    srcrepo, dstrepo = hg.clone(util.remoteui(ui, opts), source, dest,
                                pull=opts.get('pull'),
                                stream=opts.get('uncompressed'),
                                rev=opts.get('rev'),
                                update=not opts.get('noupdate'))

    if dstrepo.local() and srcrepo.capable('subversion'):
        fd = dstrepo.opener("hgrc", "a", text=True)
        for section in set(s for s, v in optionmap.itervalues()):
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
