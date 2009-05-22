import os

from hgext import rebase as hgrebase

from mercurial import cmdutil as hgcmdutil
from mercurial import commands
from mercurial import patch
from mercurial import hg
from mercurial import util as hgutil
from mercurial import node
from mercurial import i18n

from svn import core
from svn import delta

import cmdutil
import hg_delta_editor
import stupid as stupidmod
import svnwrap
import util

def parent(orig, ui, repo, *args, **opts):
    """show Mercurial & Subversion parents of the working dir or revision
    """
    if not opts.get('svn', False):
        return orig(ui, repo, *args, **opts)
    hge = hg_delta_editor.HgChangeReceiver(repo=repo)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    ha = cmdutil.parentrev(ui, repo, hge, svn_commit_hashes)
    if ha.node() == node.nullid:
        raise hgutil.Abort('No parent svn revision!')
    displayer = hgcmdutil.show_changeset(ui, repo, opts, buffered=False)
    displayer.show(ha)
    return 0


def outgoing(repo, dest=None, heads=None, force=False):
    """show changesets not found in the Subversion repository
    """
    assert dest.capable('subversion')

    # split off #rev; TODO implement --revision/#rev support
    svnurl, revs, checkout = hg.parseurl(dest.svnurl, heads)
    hge = hg_delta_editor.HgChangeReceiver(repo=repo)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    return util.outgoing_revisions(repo.ui, repo, hge, svn_commit_hashes,
                                   repo.parents()[0].node())


def diff(orig, ui, repo, *args, **opts):
    """show a diff of the most recent revision against its parent from svn
    """
    if not opts.get('svn', False) or opts.get('change', None):
        return orig(ui, repo, *args, **opts)
    svn_commit_hashes = {}
    hge = hg_delta_editor.HgChangeReceiver(repo=repo)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    if not opts.get('rev', None):
        parent = repo.parents()[0]
        o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes,
                                      parent.node())
        if o_r:
            parent = repo[o_r[-1]].parents()[0]
        opts['rev'] = ['%s:.' % node.hex(parent.node()), ]
    node1, node2 = hgcmdutil.revpair(repo, opts['rev'])
    baserev, _junk = svn_commit_hashes.get(node1, (-1, 'junk', ))
    newrev, _junk = svn_commit_hashes.get(node2, (-1, 'junk', ))
    it = patch.diff(repo, node1, node2,
                    opts=patch.diffopts(ui, opts={'git': True,
                                                  'show_function': False,
                                                  'ignore_all_space': False,
                                                  'ignore_space_change': False,
                                                  'ignore_blank_lines': False,
                                                  'unified': True,
                                                  'text': False,
                                                  }))
    ui.write(cmdutil.filterdiff(''.join(it), baserev, newrev))

def push(repo, dest, force, revs):
    """push revisions starting at a specified head back to Subversion.
    """
    assert not revs, 'designated revisions for push remains unimplemented.'
    ui = repo.ui
    svnurl = util.normalize_url(repo.ui.expandpath(dest.svnurl))
    old_encoding = util.swap_out_encoding()
    # split of #rev; TODO: implement --rev/#rev support
    svnurl, revs, checkout = hg.parseurl(svnurl, revs)
    # TODO: do credentials specified in the URL still work?
    user = repo.ui.config('hgsubversion', 'username')
    passwd = repo.ui.config('hgsubversion', 'password')
    svn = svnwrap.SubversionRepo(svnurl, user, passwd)
    hge = hg_delta_editor.HgChangeReceiver(repo=repo, uuid=svn.uuid)

    # Check if we are up-to-date with the Subversion repository.
    if hge.last_known_revision() != svn.last_changed_rev:
        # Messages are based on localrepository.push() in localrepo.py:1559. 
        # TODO: Ideally, we would behave exactly like other repositories:
        #  - push everything by default
        #  - handle additional heads in the same way
        #  - allow pushing single revisions, branches, tags or heads using
        #    the -r/--rev flag.
        if force:
            ui.warn("note: unsynced remote changes!\n")
        else:
            ui.warn("abort: unsynced remote changes!\n")
            return None, 0

    # Strategy:
    # 1. Find all outgoing commits from this head
    if len(repo.parents()) != 1:
        ui.status('Cowardly refusing to push branch merge\n')
        return 1
    workingrev = repo.parents()[0]
    ui.status('searching for changes\n')
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    outgoing = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, workingrev.node())
    if not (outgoing and len(outgoing)):
        ui.status('no changes found\n')
        return 0
    while outgoing:
        oldest = outgoing.pop(-1)
        old_ctx = repo[oldest]
        if len(old_ctx.parents()) != 1:
            ui.status('Found a branch merge, this needs discussion and '
                      'implementation.\n')
            return 1
        base_n = old_ctx.parents()[0].node()
        old_children = repo[base_n].children()
        svnbranch = repo[base_n].branch()
        oldtip = base_n
        samebranchchildren = [c for c in repo[oldtip].children() if c.branch() == svnbranch
                              and c.node() in svn_commit_hashes]
        while samebranchchildren:
            oldtip = samebranchchildren[0].node()
            samebranchchildren = [c for c in repo[oldtip].children() if c.branch() == svnbranch
                                  and c.node() in svn_commit_hashes]
        # 2. Commit oldest revision that needs to be pushed
        base_revision = svn_commit_hashes[base_n][0]
        try:
            cmdutil.commit_from_rev(ui, repo, old_ctx, hge, svnurl,
                                    base_revision, user, passwd)
        except cmdutil.NoFilesException:
            ui.warn("Could not push revision %s because it had no changes in svn.\n" %
                     old_ctx)
            return 1
        # 3. Fetch revisions from svn
        # TODO: this probably should pass in the source explicitly - rev too?
        r = repo.pull(dest, force=force)
        assert not r or r == 0
        # 4. Find the new head of the target branch
        oldtipctx = repo[oldtip]
        replacement = [c for c in oldtipctx.children() if c not in old_children
                       and c.branch() == oldtipctx.branch()]
        assert len(replacement) == 1, 'Replacement node came back as: %r' % replacement
        replacement = replacement[0]
        # 5. Rebase all children of the currently-pushing rev to the new branch
        heads = repo.heads(old_ctx.node())
        for needs_transplant in heads:
            def extrafn(ctx, extra):
                if ctx.node() == oldest:
                    return
                extra['branch'] = ctx.branch()
            # TODO: can we avoid calling our own rebase wrapper here?
            rebase(hgrebase.rebase, ui, repo, svn=True, svnextrafn=extrafn,
                   svnsourcerev=needs_transplant)
            repo = hg.repository(ui, hge.path)
            for child in repo[replacement.node()].children():
                rebasesrc = node.bin(child.extra().get('rebase_source', node.hex(node.nullid)))
                if rebasesrc in outgoing:
                    while rebasesrc in outgoing:
                        rebsrcindex = outgoing.index(rebasesrc)
                        outgoing = (outgoing[0:rebsrcindex] +
                                    [child.node(), ] + outgoing[rebsrcindex+1:])
                        children = [c for c in child.children() if c.branch() == child.branch()]
                        if children:
                            child = children[0]
                        rebasesrc = node.bin(child.extra().get('rebase_source', node.hex(node.nullid)))
        # TODO: stop constantly creating the HgChangeReceiver instances.
        hge = hg_delta_editor.HgChangeReceiver(hge.repo, ui_=ui, uuid=svn.uuid)
        svn_commit_hashes = dict(zip(hge.revmap.itervalues(), hge.revmap.iterkeys()))
    util.swap_out_encoding(old_encoding)
    return 0


def pull(repo, source, heads=[], force=False):
    """pull new revisions from Subversion"""
    assert source.capable('subversion')
    svn_url = source.svnurl

    # Split off #rev
    svn_url, heads, checkout = hg.parseurl(svn_url, heads)
    old_encoding = util.swap_out_encoding()

    # TODO implement skipto support
    skipto_rev = 0
    try:
        stopat_rev = int(checkout or 0)
    except ValueError:
        raise hgutil.Abort('unrecognised Subversion revision %s: '
                           'only numbers work.' % checkout)

    have_replay = not repo.ui.configbool('hgsubversion', 'stupid')
    if have_replay and not callable(
        delta.svn_txdelta_apply(None, None, None)[0]): #pragma: no cover
        repo.ui.status('You are using old Subversion SWIG bindings. Replay '
                       'will not work until you upgrade to 1.5.0 or newer. '
                       'Falling back to a slower method that may be buggier. '
                       'Please upgrade, or contribute a patch to use the '
                       'ctypes bindings instead of SWIG.\n')
        have_replay = False
    elif not have_replay:
        repo.ui.note('fetching stupidly...\n')

    # TODO: do credentials specified in the URL still work?
    user = repo.ui.config('hgsubversion', 'username')
    passwd = repo.ui.config('hgsubversion', 'password')
    svn = svnwrap.SubversionRepo(svn_url, user, passwd)
    hg_editor = hg_delta_editor.HgChangeReceiver(repo=repo, subdir=svn.subdir,
                                                 uuid=svn.uuid)

    start = max(hg_editor.last_known_revision(), skipto_rev)
    initializing_repo = (hg_editor.last_known_revision() <= 0)
    ui = repo.ui

    if initializing_repo and start > 0:
        raise hgutil.Abort('Revision skipping at repository initialization '
                           'remains unimplemented.')

    revisions = 0

    try:
        # start converting revisions
        for r in svn.revisions(start=start, stop=stopat_rev):
            if (r.author is None and 
                r.message == 'This is an empty revision for padding.'):
                continue
            hg_editor.update_branch_tag_map_for_rev(r)
            # got a 502? Try more than once!
            tries = 0
            converted = False
            while not converted:
                try:
                    util.describe_revision(ui, r)
                    if have_replay:
                        try:
                            cmdutil.replay_convert_rev(hg_editor, svn, r)
                        except svnwrap.SubversionRepoCanNotReplay, e: #pragma: no cover
                            ui.status('%s\n' % e.message)
                            stupidmod.print_your_svn_is_old_message(ui)
                            have_replay = False
                            stupidmod.svn_server_pull_rev(ui, svn, hg_editor, r)
                    else:
                        stupidmod.svn_server_pull_rev(ui, svn, hg_editor, r)
                    converted = True
                except core.SubversionException, e: #pragma: no cover
                    if (e.apr_err == core.SVN_ERR_RA_DAV_REQUEST_FAILED
                        and '502' in str(e)
                        and tries < 3):
                        tries += 1
                        ui.status('Got a 502, retrying (%s)\n' % tries)
                    else:
                        raise hgutil.Abort(*e.args)
            revisions += 1
    except KeyboardInterrupt:
        pass
    finally:
        util.swap_out_encoding(old_encoding)

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
    hge = hg_delta_editor.HgChangeReceiver(repo=repo)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, sourcerev=sourcerev)
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
            if (n in svn_commit_hashes and
                svn_commit_hashes[n][1] == svn_commit_hashes[p_n][1]):
                target_rev = c
                exhausted_choices = False
                break
    if parent_rev == target_rev:
        ui.status('Already up to date!\n')
        return 0
    return orig(ui, repo, dest=node.hex(target_rev.node()),
                base=node.hex(sourcerev),
                extrafn=extrafn)
