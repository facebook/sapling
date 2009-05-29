import os
import cPickle as pickle

from mercurial import hg
from mercurial import node
from mercurial import util as hgutil


import hg_delta_editor
import svnwrap
import util
import utility_commands
import svnexternals


def incoming(ui, svn_url, hg_repo_path, skipto_rev=0, stupid=None,
             tag_locations='tags', authors=None, filemap=None, **opts):
    """show incoming revisions from Subversion
    """
    svn_url = util.normalize_url(svn_url)

    initializing_repo = False
    user, passwd = util.getuserpass(opts)
    svn = svnwrap.SubversionRepo(svn_url, user, passwd)
    author_host = ui.config('hgsubversion', 'defaulthost', svn.uuid)
    tag_locations = tag_locations.split(',')
    hg_editor = hg_delta_editor.HgChangeReceiver(hg_repo_path,
                                                 ui_=ui,
                                                 subdir=svn.subdir,
                                                 author_host=author_host,
                                                 tag_locations=tag_locations,
                                                 authors=authors,
                                                 filemap=filemap,
                                                 uuid=svn.uuid)
    start = max(hg_editor.last_known_revision(), skipto_rev)
    initializing_repo = (hg_editor.last_known_revision() <= 0)

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


def rebuildmeta(ui, repo, hg_repo_path, args, **opts):
    """rebuild hgsubversion metadata using values stored in revisions
    """
    if len(args) != 1:
        url = repo.ui.expandpath(dest or 'default-push', dest or 'default')
    else:
        url = args[0]
    uuid = None
    url = util.normalize_url(url.rstrip('/'))
    user, passwd = util.getuserpass(opts)
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
        if not convinfo:
            continue

        # check that the conversion metadata matches expectations
        assert convinfo.startswith('svn:')
        revpath, revision = convinfo[40:].split('@')
        if subdir and subdir[0] != '/':
            subdir = '/' + subdir
        if subdir and subdir[-1] == '/':
            subdir = subdir[:-1]
        assert revpath.startswith(subdir), ('That does not look like the '
                                            'right location in the repo.')

        # write repository uuid if required
        if uuid is None:
            uuid = convinfo[4:40]
            assert uuid == svn.uuid, 'UUIDs did not match!'
            uuidfile = open(os.path.join(svnmetadir, 'uuid'), 'w')
            uuidfile.write(uuid)
            uuidfile.close()

        # find commitpath, write to revmap
        commitpath = revpath[len(subdir)+1:]
        if commitpath.startswith('branches'):
            commitpath = commitpath[len('branches/'):]
        elif commitpath == 'trunk':
            commitpath = ''
        else:
            assert False, 'Unhandled case in rebuildmeta'
        revmap.write('%s %s %s\n' % (revision, ctx.hex(), commitpath))

        revision = int(revision)
        noderevnums[ctx.node()] = revision
        if revision > last_rev:
            last_rev = revision

        # deal with branches
        if ctx.extra().get('close'): # don't re-add, we just deleted!
            continue
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

        for cctx in ctx.children():
            if cctx.extra().get('close'):
                branchinfo.pop(branch, None)
                break

    # save off branch info
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


nourl = ['rebuildmeta', 'help'] + utility_commands.nourl
table = {
    'update': update,
    'help': help,
    'rebuildmeta': rebuildmeta,
    'incoming': incoming,
    'updateexternals': svnexternals.updateexternals,
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
