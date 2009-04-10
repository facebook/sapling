import os
import cPickle as pickle

from mercurial import cmdutil as hgcmdutil
from mercurial import commands
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


def clone(orig, ui, source, dest=None, **opts):
    '''clone Subversion repository to a local Mercurial repository.

    If no destination directory name is specified, it defaults to the
    basename of the source plus "-hg".

    You can specify multiple paths for the location of tags using comma
    separated values.
    '''
    svnurl = ui.expandpath(source)
    if not cmdutil.issvnurl(svnurl):
        orig(ui, repo, source=source, dest=dest, *args, **opts)

    if not dest:
        dest = hg.defaultdest(source) + '-hg'
        ui.status("Assuming destination %s\n" % dest)

    if os.path.exists(dest):
        raise hgutil.Abort("destination '%s' already exists" % dest)
    url = util.normalize_url(svnurl)
    res = -1
    try:
        try:
            res = pull(None, ui, None, True, opts.pop('svn_stupid', False),
                       source=url, create_new_dest=dest, **opts)
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
            fp.write("default = %(url)s\nsvn = %(url)s\n" % {'url': svnurl.replace('%', '%%')})
            fp.close()
            if res is None or res == 0:
                commands.update(ui, repo, repo['tip'].node())

    return res


def pull(orig, ui, repo, svn=None, svn_stupid=False, source="default", create_new_dest=False,
         *args, **opts):
    """pull new revisions from Subversion
    """
    url = ui.expandpath(source)
    if not (cmdutil.issvnurl(url) or svn or create_new_dest):
        orig(ui, repo, source=source, *args, **opts)
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
    user = opts.get('username', hgutil.getuser())
    passwd = opts.get('password', '')
    svn = svnwrap.SubversionRepo(svn_url, user, passwd)
    author_host = "@%s" % svn.uuid
    # TODO these should be configurable again, but I'm torn on how.
    # Maybe this should be configured in .hg/hgrc for each repo? Seems vaguely reasonable.
    tag_locations = ['tags', ]
    authors = None
    filemap = None
    if repo:
        hg_editor = hg_delta_editor.HgChangeReceiver(repo=repo)
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


def incoming(ui, svn_url, hg_repo_path, skipto_rev=0, stupid=None,
             tag_locations='tags', authors=None, filemap=None, **opts):
    """show incoming revisions from Subversion
    """
    svn_url = util.normalize_url(svn_url)

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

    rev_stuff = (('revision', 'revnum'),
                 ('user', 'author'),
                 ('date', 'date'),
                 ('message', 'message')
                )

    ui.status('incoming changes from %s\n' % svn_url)

    for r in svn.revisions(start=start):
        ui.status('\n')
        for label, attr in rev_stuff:
            l1 = label+':'
            ui.status('%s%s\n' % (l1.ljust(13),
                                  str(r.__getattribute__(attr)).strip(), ))


def push(orig, ui, repo, dest=None, **opts):
    """push revisions starting at a specified head back to Subversion.
    """
    svnurl = ui.expandpath(dest or 'default-push', dest or 'default')
    if not cmdutil.issvnurl(svnurl):
        orig(ui, repo, dest=dest, *args, **opts)
    old_encoding = util.swap_out_encoding()
    hge = hg_delta_editor.HgChangeReceiver(repo=repo)
    assert svnurl == hge.url
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
            cmdutil.commit_from_rev(ui, repo, old_ctx, hge, svnurl,
                                    base_revision, user, passwd)
        except cmdutil.NoFilesException:
            ui.warn("Could not push revision %s because it had no changes in svn.\n" %
                     old_ctx)
            return 1
        # 3. Fetch revisions from svn
        r = pull(None, ui, repo, True, stupid=opts.get('svn_stupid', False),
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
        hge = hg_delta_editor.HgChangeReceiver(hge.path, ui_=ui)
        svn_commit_hashes = dict(zip(hge.revmap.itervalues(), hge.revmap.iterkeys()))
    util.swap_out_encoding(old_encoding)
    return 0


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
    'update': update,
    'help': help,
    'rebuildmeta': rebuildmeta,
    'incoming': incoming,
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
