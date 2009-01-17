import os
import pickle

from mercurial import node

import svnwrap
import util

def rebuildmeta(ui, repo, hg_repo_path, args, **opts):
    """rebuild hgsubversion metadata using values stored in revisions
    """
    assert len(args) == 1, 'You must pass the svn URI used to create this repo.'
    uuid = None
    svn = svnwrap.SubversionRepo(url=args[0])
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
                urlfile.write(args[0])
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
    lastrevfile = open(os.path.join(svnmetadir, 'last_rev'), 'w')
    lastrevfile.write(str(last_rev))
    lastrevfile.close()
    branchinfofile = open(os.path.join(svnmetadir, 'branch_info'), 'w')
    pickle.dump(branchinfo, branchinfofile)
    branchinfofile.close()
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
        if source.startswith('branches/') or source == 'trunk':
            source = determinebranch(source)
            if rev <= last_rev:
                tagsinfo[tag] = source, rev
    tagsinfofile = open(os.path.join(svnmetadir, 'tag_info'), 'w')
    pickle.dump(tagsinfo, tagsinfofile)
    tagsinfofile.close()
rebuildmeta = util.register_subcommand('rebuildmeta')(rebuildmeta)


def determinebranch(branch):
    if branch.startswith('branches'):
        branch = branch[len('branches/'):]
    elif branch == 'trunk':
        branch = None
    else:
        assert False, 'Unhandled case while regenerating metadata.'
    return branch
