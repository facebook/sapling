import os

from hgext import rebase as hgrebase

from mercurial import cmdutil as hgcmdutil
from mercurial import commands
from mercurial import patch
from mercurial import hg
from mercurial import util as hgutil
from mercurial import node

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


def outgoing(orig, ui, repo, dest=None, *args, **opts):
    """show changesets not found in the Subversion repository
    """
    svnurl = repo.ui.expandpath(dest or 'default-push', dest or 'default')
    if not (cmdutil.issvnurl(svnurl) or opts.get('svn', False)):
        return orig(ui, repo, dest, *args, **opts)

    hge = hg_delta_editor.HgChangeReceiver(repo=repo)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes,
                                  repo.parents()[0].node())
    if not (o_r and len(o_r)):
        ui.status('no changes found\n')
        return 0
    displayer = hgcmdutil.show_changeset(ui, repo, opts, buffered=False)
    for node in reversed(o_r):
        displayer.show(repo[node])


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


def push(orig, ui, repo, dest=None, *args, **opts):
    """push revisions starting at a specified head back to Subversion.
    """
    opts.pop('svn', None) # unused in this case
    svnurl = repo.ui.expandpath(dest or 'default-push', dest or 'default')
    if not cmdutil.issvnurl(svnurl):
        return orig(ui, repo, dest=dest, *args, **opts)
    old_encoding = util.swap_out_encoding()
    hge = hg_delta_editor.HgChangeReceiver(repo=repo)
    svnurl = util.normalize_url(svnurl)
    if svnurl != hge.url:
        raise hgutil.Abort('wrong subversion url!')
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    user, passwd = util.getuserpass(opts)
    # Strategy:
    # 1. Find all outgoing commits from this head
    if len(repo.parents()) != 1:
        ui.status('Cowardly refusing to push branch merge\n')
        return 1
    workingrev = repo.parents()[0]
    ui.status('searching for changes\n')
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
        # TODO this probably should pass in the source explicitly
        r = pull(None, ui, repo, svn=True, stupid=opts.get('svn_stupid', False),
                 username=user, password=passwd)
        assert not r or r == 0
        # 4. Find the new head of the target branch
        repo = hg.repository(ui, hge.path)
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
            rebase(hgrebase.rebase, ui, repo, svn=True, svnextrafn=extrafn,
                   svnsourcerev=needs_transplant, **opts)
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
        hge = hg_delta_editor.HgChangeReceiver(hge.path, ui_=ui)
        svn_commit_hashes = dict(zip(hge.revmap.itervalues(), hge.revmap.iterkeys()))
    util.swap_out_encoding(old_encoding)
    return 0


def clone(orig, ui, source, dest=None, *args, **opts):
    '''clone Subversion repository to a local Mercurial repository.

    If no destination directory name is specified, it defaults to the
    basename of the source plus "-hg".

    You can specify multiple paths for the location of tags using comma
    separated values.
    '''
    svnurl = ui.expandpath(source)
    if not cmdutil.issvnurl(svnurl):
        return orig(ui, source=source, dest=dest, *args, **opts)

    if not dest:
        dest = hg.defaultdest(source) + '-hg'
        ui.status("Assuming destination %s\n" % dest)

    if os.path.exists(dest):
        raise hgutil.Abort("destination '%s' already exists" % dest)
    url = util.normalize_url(svnurl)
    res = -1
    try:
        try:
            res = pull(None, ui, None, source=url, svn=None,
                       svn_stupid=opts.pop('svn_stupid', False),
                       create_new_dest=dest, **opts)
        except core.SubversionException, e:
            if e.apr_err == core.SVN_ERR_RA_SERF_SSL_CERT_UNTRUSTED:
                raise hgutil.Abort('It appears svn does not trust the ssl cert for this site.\n'
                         'Please try running svn ls on that url first.')
            raise
    finally:
        if os.path.exists(dest):
            repo = hg.repository(ui, dest)
            fp = repo.opener("hgrc", "w", text=True)
            fp.write("[paths]\n")
            # percent needs to be escaped for ConfigParser
            fp.write("default = %(url)s\nsvn = %(url)s\n" % {'url': svnurl})
            fp.close()
            if (res is None or res == 0) and not opts.get('noupdate', False):
                commands.update(ui, repo, repo['tip'].node())

    return res


def pull(orig, ui, repo, source="default", *args, **opts):
    """pull new revisions from Subversion

    Also takes svn, svn_stupid, and create_new_dest kwargs.
    """
    svn = opts.pop('svn', None)
    svn_stupid = opts.pop('svn_stupid', False)
    create_new_dest = opts.pop('create_new_dest', False)
    url = ((repo and repo.ui) or ui).expandpath(source)
    if not (cmdutil.issvnurl(url) or svn or create_new_dest):
        return orig(ui, repo, source=source, *args, **opts)
    svn_url = url
    svn_url = util.normalize_url(svn_url)
    old_encoding = util.swap_out_encoding()
    # TODO implement skipto support
    skipto_rev = 0
    have_replay = not svn_stupid
    if have_replay and not callable(
        delta.svn_txdelta_apply(None, None, None)[0]): #pragma: no cover
        ui.status('You are using old Subversion SWIG bindings. Replay will not'
                  ' work until you upgrade to 1.5.0 or newer. Falling back to'
                  ' a slower method that may be buggier. Please upgrade, or'
                  ' contribute a patch to use the ctypes bindings instead'
                  ' of SWIG.\n')
        have_replay = False
    initializing_repo = False
    user, passwd = util.getuserpass(opts)
    svn = svnwrap.SubversionRepo(svn_url, user, passwd)
    author_host = "@%s" % svn.uuid
    tag_locations = ['tags', ]
    authors = opts.pop('svn_authors', None)
    filemap = opts.pop('svn_filemap', None)
    if repo:
        hg_editor = hg_delta_editor.HgChangeReceiver(repo=repo,
                                                     subdir=svn.subdir,
                                                     author_host=author_host,
                                                     tag_locations=tag_locations,
                                                     authors=authors,
                                                     filemap=filemap)
    else:
        hg_editor = hg_delta_editor.HgChangeReceiver(ui_=ui,
                                                     path=create_new_dest,
                                                     subdir=svn.subdir,
                                                     author_host=author_host,
                                                     tag_locations=tag_locations,
                                                     authors=authors,
                                                     filemap=filemap)
    if os.path.exists(hg_editor.uuid_file):
        uuid = open(hg_editor.uuid_file).read()
        assert uuid == svn.uuid
        start = hg_editor.last_known_revision()
    else:
        open(hg_editor.uuid_file, 'w').write(svn.uuid)
        open(hg_editor.svn_url_file, 'w').write(svn_url)
        initializing_repo = True
        start = skipto_rev

    if initializing_repo and start > 0:
        raise hgutil.Abort('Revision skipping at repository initialization '
                           'remains unimplemented.')

    # start converting revisions
    for r in svn.revisions(start=start):
        valid = True
        hg_editor.update_branch_tag_map_for_rev(r)
        for p in r.paths:
            if hg_editor._is_path_valid(p):
                valid = True
                break
        if valid:
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
    util.swap_out_encoding(old_encoding)


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
