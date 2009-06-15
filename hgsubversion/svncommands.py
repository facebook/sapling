import os
import cPickle as pickle

from mercurial import hg
from mercurial import node
from mercurial import util as hgutil

import svnwrap
import util
import utility_commands
import svnexternals


def verify(ui, repo, *args, **opts):
    '''verify current revision against Subversion repository
    '''

    if not args:
        url = repo.ui.expandpath('default')
    else:
        url = args[0]

    rev = opts.get('verifynode', '.')
    ctx = repo[rev]
    if 'close' in ctx.extra():
        ui.write('cannot verify closed branch')
        return 0
    srev = ctx.extra().get('convert_revision')
    if srev is None:
        raise hgutil.Abort('revision %s not from SVN' % ctx)

    srev = int(srev.split('@')[1])
    ui.write('verifying %s against r%i\n' % (ctx, srev))

    url = util.normalize_url(url.rstrip('/'))
    user, passwd = util.getuserpass(opts)
    svn = svnwrap.SubversionRepo(url, user, passwd)

    btypes = {'default': 'trunk'}
    branchpath = btypes.get(ctx.branch(), 'branches/%s' % ctx.branch())
    svnfiles = set()
    result = 0
    for fn, type in svn.list_files(branchpath, srev):
        if type != 'f':
            continue
        svnfiles.add(fn)
        data, mode = svn.get_file(branchpath + '/'  + fn, srev)
        fctx = ctx[fn]
        dmatch = fctx.data() == data
        mmatch = fctx.flags() == mode
        if not (dmatch and mmatch):
            ui.write('difference in file %s' % fn)
            result = 1

    hgfiles = set(ctx)
    hgfiles.discard('.hgtags')
    hgfiles.discard('.hgsvnexternals')
    if hgfiles != svnfiles:
        missing = set(hgfiles).symmetric_difference(svnfiles)
        ui.write('missing files: %s' % (', '.join(missing)))
        result = 1

    return result


def rebuildmeta(ui, repo, hg_repo_path, args, **opts):
    """rebuild hgsubversion metadata using values stored in revisions
    """
    if len(args) != 1:
        dest = args[0]
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

        # don't reflect closed branches
        if ctx.extra().get('close') and not ctx.files():
            continue

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
        if ctx.extra().get('close'):
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


table = {
    'update': update,
    'help': help,
    'rebuildmeta': rebuildmeta,
    'updateexternals': svnexternals.updateexternals,
    'verify': verify,
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
