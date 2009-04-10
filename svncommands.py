import os
import cPickle as pickle

from mercurial import hg
from mercurial import node
from mercurial import patch
from mercurial import util as hgutil

from svn import core
from svn import delta

import hg_delta_editor
import svnwrap
import stupid as stupidmod
import cmdutil
import util
import utility_commands


def pull(ui, svn_url, hg_repo_path, skipto_rev=0, stupid=None,
         tag_locations='tags', authors=None, filemap=None, **opts):
    """pull new revisions from Subversion
    """
    svn_url = util.normalize_url(svn_url)
    old_encoding = util.swap_out_encoding()
    skipto_rev=int(skipto_rev)
    have_replay = not stupid
    if have_replay and not callable(
        delta.svn_txdelta_apply(None, None, None)[0]): #pragma: no cover
        ui.status('You are using old Subversion SWIG bindings. Replay will not'
                  ' work until you upgrade to 1.5.0 or newer. Falling back to'
                  ' a slower method that may be buggier. Please upgrade, or'
                  ' contribute a patch to use the ctypes bindings instead'
                  ' of SWIG.\n')
        have_replay = False
    initializing_repo = False
    user = opts.get('username', hgutil.getuser())
    passwd = opts.get('password', '')
    svn = svnwrap.SubversionRepo(svn_url, user, passwd)
    author_host = "@%s" % svn.uuid
    tag_locations = tag_locations.split(',')
    hg_editor = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                                 ui_=ui,
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


def push(ui, repo, hg_repo_path, svn_url, stupid=False, **opts):
    """push revisions starting at a specified head back to Subversion.
    """
    old_encoding = util.swap_out_encoding()
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    user = opts.get('username', hgutil.getuser())
    passwd = opts.get('password', '')

    # Strategy:
    # 1. Find all outgoing commits from this head
    if len(repo.parents()) != 1:
        ui.status('Cowardly refusing to push branch merge')
        return 1
    workingrev = repo.parents()[0]
    outgoing = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, workingrev.node())
    if not (outgoing and len(outgoing)):
        ui.status('No revisions to push.')
        return 0
    while outgoing:
        oldest = outgoing.pop(-1)
        old_ctx = repo[oldest]
        if len(old_ctx.parents()) != 1:
            ui.status('Found a branch merge, this needs discussion and '
                      'implementation.')
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
            cmdutil.commit_from_rev(ui, repo, old_ctx, hge, svn_url,
                                    base_revision, user, passwd)
        except cmdutil.NoFilesException:
            ui.warn("Could not push revision %s because it had no changes in svn.\n" %
                     old_ctx)
            return 1
        # 3. Fetch revisions from svn
        r = pull(ui, svn_url, hg_repo_path, stupid=stupid,
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
            utility_commands.rebase(ui, repo, extrafn=extrafn,
                                    sourcerev=needs_transplant, **opts)
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
        hge = hg_delta_editor.HgChangeReceiver(hg_repo_path, ui_=ui)
        svn_commit_hashes = dict(zip(hge.revmap.itervalues(), hge.revmap.iterkeys()))
    util.swap_out_encoding(old_encoding)
    return 0


def diff(ui, repo, hg_repo_path, **opts):
    """show a diff of the most recent revision against its parent from svn
    """
    hge = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                           ui_=ui)
    svn_commit_hashes = dict(zip(hge.revmap.itervalues(),
                                 hge.revmap.iterkeys()))
    parent = repo.parents()[0]
    o_r = util.outgoing_revisions(ui, repo, hge, svn_commit_hashes, parent.node())
    if o_r:
        parent = repo[o_r[-1]].parents()[0]
    base_rev, _junk = svn_commit_hashes[parent.node()]
    it = patch.diff(repo, parent.node(), None,
                    opts=patch.diffopts(ui, opts={'git': True,
                                                  'show_function': False,
                                                  'ignore_all_space': False,
                                                  'ignore_space_change': False,
                                                  'ignore_blank_lines': False,
                                                  'unified': True,
                                                  'text': False,
                                                  }))
    ui.write(cmdutil.filterdiff(''.join(it), base_rev))


def rebuildmeta(ui, repo, hg_repo_path, args, **opts):
    """rebuild hgsubversion metadata using values stored in revisions
    """
    if len(args) != 1:
        raise hgutil.Abort('You must pass the svn URI used to create this repo.')
    uuid = None
    url = args[0].rstrip('/')
    user = opts.get('username', hgutil.getuser())
    passwd = opts.get('password', '')
    svn = svnwrap.SubversionRepo(url, user, passwd)
    subdir = svn.subdir
    svnmetadir = os.path.join(repo.path, 'svn')
    if not os.path.exists(svnmetadir):
        os.makedirs(svnmetadir)

    revmap = open(os.path.join(svnmetadir, 'rev_map'), 'w')
    revmap.write('1\n')
    last_rev = -1
    branchinfo = {}
    noderevnums = {}
    for rev in repo:
        ctx = repo[rev]
        convinfo = ctx.extra().get('convert_revision', None)
        if convinfo:
            assert convinfo.startswith('svn:')
            revpath, revision = convinfo[40:].split('@')
            if subdir and subdir[0] != '/':
                subdir = '/' + subdir
            if subdir and subdir[-1] == '/':
                subdir = subdir[:-1]
            assert revpath.startswith(subdir), ('That does not look like the '
                                                'right location in the repo.')
            if uuid is None:
                uuid = convinfo[4:40]
                assert uuid == svn.uuid, 'UUIDs did not match!'
                urlfile = open(os.path.join(svnmetadir, 'url'), 'w')
                urlfile.write(url)
                urlfile.close()
                uuidfile = open(os.path.join(svnmetadir, 'uuid'), 'w')
                uuidfile.write(uuid)
                uuidfile.close()
            commitpath = revpath[len(subdir)+1:]
            if commitpath.startswith('branches'):
                commitpath = commitpath[len('branches/'):]
            elif commitpath == 'trunk':
                commitpath = ''
            else:
                assert False, 'Unhandled case in rebuildmeta'
            revmap.write('%s %s %s\n' % (revision,
                                         node.hex(ctx.node()),
                                         commitpath))
            revision = int(revision)
            noderevnums[ctx.node()] = revision
            if revision > last_rev:
                last_rev = revision
            branch = ctx.branch()
            if branch == 'default':
                branch = None
            if branch not in branchinfo:
                parent = ctx.parents()[0]
                if (parent.node() in noderevnums
                    and parent.branch() != ctx.branch()):
                    parentbranch = parent.branch()
                    if parentbranch == 'default':
                        parentbranch = None
                else:
                    parentbranch = None
                branchinfo[branch] = (parentbranch,
                                      noderevnums.get(parent.node(), 0),
                                      revision)
            for c in ctx.children():
                if c.branch() == 'closed-branches':
                    if branch in branchinfo:
                        del branchinfo[branch]
    branchinfofile = open(os.path.join(svnmetadir, 'branch_info'), 'w')
    pickle.dump(branchinfo, branchinfofile)
    branchinfofile.close()

    # now handle tags
    tagsinfo = {}
    realtags = svn.tags
    tagsleft = realtags.items()
    while tagsleft:
        tag, tagparent = tagsleft.pop(0)
        source, rev = tagparent
        if source.startswith('tags/'):
            src = source[len('tags/'):]
            if src in tagsinfo:
                tagsinfo[tag] = tagsinfo[src]
            elif src in realtags:
                if (realtags[src][1] <= last_rev
                    or realtags[src][0].startswith('tags/')):
                    tagsleft.append(src)
            else:
                older_tags = svn.tags_at_rev(rev)
                newsrc, newrev = older_tags[src]
                tagsleft.append((tag, (newsrc, newrev)))
            continue
        else:
            # determine the branch
            assert not source.startswith('tags/'), "Tags can't be tags of other tags."
            if source.startswith('branches/'):
                source = source[len('branches/'):]
            elif source == 'trunk':
                source = None
            else:
                source = '../' + source
        if rev <= last_rev and (source or 'default') in repo.branchtags():
            tagsinfo[tag] = source, rev

    tagsinfofile = open(os.path.join(svnmetadir, 'tag_info'), 'w')
    pickle.dump(tagsinfo, tagsinfofile)
    tagsinfofile.close()


def help(ui, args=None, **opts):
    """show help for a given subcommands or a help overview
    """
    if args:
        subcommand = args[0]
        if subcommand not in table:
            candidates = []
            for c in table:
                if c.startswith(subcommand):
                    candidates.append(c)
            if len(candidates) == 1:
                subcommand = candidates[0]
            elif len(candidates) > 1:
                ui.status('Ambiguous command. Could have been:\n%s\n' %
                          ' '.join(candidates))
                return
        doc = table[subcommand].__doc__
        if doc is None:
            doc = "No documentation available for %s." % subcommand
        ui.status(doc.strip(), '\n')
        return
    ui.status(_helpgen())


def update(ui, args, repo, clean=False, **opts):
    """update to a specified Subversion revision number
    """
    assert len(args) == 1
    rev = int(args[0])
    path = os.path.join(repo.path, 'svn', 'rev_map')
    answers = []
    for k,v in util.parse_revmap(path).iteritems():
        if k[0] == rev:
            answers.append((v, k[1]))
    if len(answers) == 1:
        if clean:
            return hg.clean(repo, answers[0][0])
        return hg.update(repo, answers[0][0])
    elif len(answers) == 0:
        ui.status('Revision %s did not produce an hg revision.\n' % rev)
        return 1
    else:
        ui.status('Ambiguous revision!\n')
        ui.status('\n'.join(['%s on %s' % (node.hex(a[0]), a[1]) for a in
                             answers]+['']))
    return 1


nourl = ['rebuildmeta'] + utility_commands.nourl
table = {
    'pull': pull,
    'push': push,
    'dcommit': push,
    'update': update,
    'help': help,
    'rebuildmeta': rebuildmeta,
    'diff': diff,
}

table.update(utility_commands.table)


def _helpgen():
    ret = ['hg svn ...', '',
           'subcommands for Subversion integration', '',
           'list of subcommands:', '']
    for name, func in sorted(table.items()):
        short_description = (func.__doc__ or '').splitlines()[0]
        ret.append(" %-10s  %s" % (name, short_description))
    return '\n'.join(ret) + '\n'
