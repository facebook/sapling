import os
import posixpath
import sys
import traceback
import urlparse
import errno

from mercurial import commands
from mercurial import hg
from mercurial import node
from mercurial import util as hgutil
from mercurial import error

import layouts
import maps
import svnwrap
import svnrepo
import util
import svnexternals
import verify
import svnmeta


def updatemeta(ui, repo, args, **opts):
    """Do a partial rebuild of the subversion metadata.

    Assumes that the metadata that currently exists is valid, but that
    some is missing, e.g. because you have pulled some revisions via a
    native mercurial method.

    """

    return _buildmeta(ui, repo, args, partial=True)


def rebuildmeta(ui, repo, args, unsafe_skip_uuid_check=False, **opts):
    """rebuild hgsubversion metadata using values stored in revisions
    """
    return _buildmeta(ui, repo, args, partial=False,
                      skipuuid=unsafe_skip_uuid_check)

def _buildmeta(ui, repo, args, partial=False, skipuuid=False):

    if repo is None:
        raise error.RepoError("There is no Mercurial repository"
                              " here (.hg not found)")

    dest = None
    validateuuid = False
    if len(args) == 1:
        dest = args[0]
        validateuuid = True
    elif len(args) > 1:
        raise hgutil.Abort('rebuildmeta takes 1 or no arguments')
    url = repo.ui.expandpath(dest or repo.ui.config('paths', 'default-push') or
                             repo.ui.config('paths', 'default') or '')

    meta = svnmeta.SVNMeta(repo, skiperrorcheck=True)

    svn = None
    if meta.subdir is None:
        svn = svnrepo.svnremoterepo(ui, url).svn
        meta.subdir = svn.subdir

    youngest = 0
    startrev = 0
    branchinfo = {}
    revmap = meta.revmap
    if partial:
        try:
            # we can't use meta.lastpulled here because we are bootstraping the
            # lastpulled and want to keep the cached value on disk during a
            # partial rebuild
            foundpartialinfo = False
            youngestpath = os.path.join(meta.metapath, 'lastpulled')
            if os.path.exists(youngestpath):
                youngest = util.load(youngestpath)
                lasthash = revmap.lasthash
                if len(revmap) > 0 and lasthash:
                    startrev = repo[lasthash].rev() + 1
                    branchinfo = util.load(meta.branch_info_file)
                    foundpartialinfo = True
            if not foundpartialinfo:
                ui.status('missing some metadata -- doing a full rebuild\n')
                partial = False
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
            ui.status('missing some metadata -- doing a full rebuild\n')
        except AttributeError:
            ui.status('no metadata available -- doing a full rebuild\n')

    if not partial:
        revmap.clear()

    last_rev = -1
    if not partial and os.path.exists(meta.tagfile):
        os.unlink(meta.tagfile)

    skipped = set()
    closed = set()

    numrevs = len(repo) - startrev

    # ctx.children() visits all revisions in the repository after ctx. Calling
    # it would make us use O(revisions^2) time, so we perform an extra traversal
    # of the repository instead. During this traversal, we find all converted
    # changesets that close a branch, and store their first parent
    for rev in xrange(startrev, len(repo)):
        ui.progress('prepare', rev - startrev, total=numrevs)
        try:
            ctx = repo[rev]
        except error.RepoError:
            # this revision is hidden
            continue

        convinfo = util.getsvnrev(ctx, None)
        if not convinfo:
            continue
        svnrevnum = int(convinfo.rsplit('@', 1)[1])
        youngest = max(youngest, svnrevnum)

        if ctx.extra().get('close', None) is None:
            continue

        droprev = lambda x: x.rsplit('@', 1)[0]
        parentctx = ctx.parents()[0]
        parentinfo = util.getsvnrev(parentctx, '@')

        if droprev(parentinfo) == droprev(convinfo):
            if parentctx.rev() < startrev:
                parentbranch = parentctx.branch()
                if parentbranch == 'default':
                    parentbranch = None
                branchinfo.pop(parentbranch)
            else:
                closed.add(parentctx.rev())

    meta.lastpulled = youngest
    ui.progress('prepare', None, total=numrevs)

    revmapbuf = []
    for rev in xrange(startrev, len(repo)):
        ui.progress('rebuild', rev-startrev, total=numrevs)
        try:
            ctx = repo[rev]
        except error.RepoError:
            # this revision is hidden
            continue

        convinfo = util.getsvnrev(ctx, None)
        if not convinfo:
            continue
        if '.hgtags' in ctx.files():
            parent = ctx.parents()[0]
            parentdata = ''
            if '.hgtags' in parent:
                parentdata = parent.filectx('.hgtags').data()
            newdata = ctx.filectx('.hgtags').data()
            for newtag in newdata[len(parentdata):-1].split('\n'):
                ha, tag = newtag.split(' ', 1)
                tagged = util.getsvnrev(repo[ha], None)
                if tagged is None:
                    tagged = -1
                else:
                    tagged = int(tagged[40:].split('@')[1])
                # This is max(tagged rev, tagging rev) because if it is a normal
                # tag, the tagging revision has the right rev number. However, if it
                # was an edited tag, then the tagged revision has the correct revision
                # number.
                tagging = int(convinfo[40:].split('@')[1])
                tagrev = max(tagged, tagging)
                meta.tags[tag] = node.bin(ha), tagrev

        # check that the conversion metadata matches expectations
        assert convinfo.startswith('svn:')
        revpath, revision = convinfo[40:].split('@')
        # use tmp variable for testing
        subdir = meta.subdir
        if subdir and subdir[0] != '/':
            subdir = '/' + subdir
        if subdir and subdir[-1] == '/':
            subdir = subdir[:-1]
        assert revpath.startswith(subdir), ('That does not look like the '
                                            'right location in the repo.')

        # meta.layout is a config-cached property so instead of testing for
        # None we test to see if the layout is 'auto' and, if so, try to guess
        # the layout based on the commits (where subdir is compared to the
        # revpath extracted from the commit)
        if meta.layout == 'auto':
            meta.layout = meta.layout_from_commit(subdir, revpath,
                                                  ctx.branch())
        elif meta.layout == 'single':
            assert (subdir or '/') == revpath, ('Possible layout detection'
                                                ' defect in replay')

        # write repository uuid if required
        if meta.uuid is None or validateuuid:
            validateuuid = False
            uuid = convinfo[4:40]
            if not skipuuid:
                if svn is None:
                    svn = svnrepo.svnremoterepo(ui, url).svn
                if uuid != svn.uuid:
                    raise hgutil.Abort('remote svn repository identifier '
                                       'does not match')
            meta.uuid = uuid

        # don't reflect closed branches
        if (ctx.extra().get('close') and not ctx.files() or
            ctx.parents()[0].node() in skipped):
            skipped.add(ctx.node())
            continue

        # find commitpath, write to revmap
        commitpath = revpath[len(subdir)+1:]

        tag_locations = meta.layoutobj.taglocations
        found_tag = False
        for location in tag_locations:
            if commitpath.startswith(location + '/'):
                found_tag = True
                break
        if found_tag and ctx.extra().get('close'):
            continue

        branch = meta.layoutobj.localname(commitpath)
        revmapbuf.append((revision, branch, ctx.node()))

        revision = int(revision)
        if revision > last_rev:
            last_rev = revision

        # deal with branches
        if branch and branch.startswith('../'):
            parent = ctx
            while parent.node() != node.nullid:
                parentextra = parent.extra()
                parentinfo = util.getsvnrev(parent)
                assert parentinfo
                parent = parent.parents()[0]

                parentpath = parentinfo[40:].split('@')[0][len(subdir) + 1:]

                found_tag = False
                for location in tag_locations:
                    if parentpath.startswith(location + '/'):
                        found_tag = True
                        break
                if found_tag and parentextra.get('close'):
                    continue

                branch = meta.layoutobj.localname(parentpath)
                break

        if rev in closed:
            # a direct child of this changeset closes the branch; drop it
            branchinfo.pop(branch, None)
        elif ctx.extra().get('close'):
            pass
        elif branch not in branchinfo:
            parent = ctx.parents()[0]
            if (parent.node() not in skipped
                and util.getsvnrev(parent, '').startswith('svn:')
                and parent.branch() != ctx.branch()):
                parentbranch = parent.branch()
                if parentbranch == 'default':
                    parentbranch = None
            else:
                parentbranch = None
            # branchinfo is a map from mercurial branch to a
            # (svn branch, svn parent revision, svn revision) tuple
            parentrev = util.getsvnrev(parent, '@').split('@')[1] or 0
            branchinfo[branch] = (parentbranch,
                                  int(parentrev),
                                  revision)

    revmap.batchset(revmapbuf)
    ui.progress('rebuild', None, total=numrevs)

    # save off branch info
    util.dump(branchinfo, meta.branch_info_file)


def help_(ui, args=None, **opts):
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
                raise error.AmbiguousCommand(subcommand, candidates)
                return
        doc = table[subcommand].__doc__
        if doc is None:
            doc = "No documentation available for %s." % subcommand
        ui.status(doc.strip(), '\n')
        return
    commands.help_(ui, 'svn')


def update(ui, args, repo, clean=False, **opts):
    """update to a specified Subversion revision number
    """

    try:
        rev = int(args[0])
    except IndexError:
        raise error.CommandError('svn',
                                 "no revision number specified for 'update'")
    except ValueError:
        raise error.Abort("'%s' is not a valid Subversion revision number"
                          % args[0])

    meta = repo.svnmeta()

    answers = []
    for k, v in meta.revmap.iteritems():
        if k[0] == rev:
            answers.append((v, k[1]))

    if len(answers) == 1:
        if clean:
            return hg.clean(repo, answers[0][0])
        return hg.update(repo, answers[0][0])
    elif len(answers) == 0:
        ui.status('revision %s did not produce an hg revision\n' % rev)
        return 1
    else:
        ui.status('ambiguous revision!\n')
        revs = ['%s on %s' % (node.hex(a[0]), a[1]) for a in answers] + ['']
        ui.status('\n'.join(revs))
    return 1


def genignore(ui, repo, force=False, **opts):
    """generate .hgignore from svn:ignore properties.
    """

    if repo is None:
        raise error.RepoError("There is no Mercurial repository"
                              " here (.hg not found)")

    ignpath = repo.wjoin('.hgignore')
    if not force and os.path.exists(ignpath):
        raise hgutil.Abort('not overwriting existing .hgignore, try --force?')
    svn = svnrepo.svnremoterepo(repo.ui).svn
    meta = repo.svnmeta()
    hashes = meta.revmap.hashes()
    parent = util.parentrev(ui, repo, meta, hashes)
    r, br = hashes[parent.node()]
    branchpath = meta.layoutobj.remotename(br)
    if branchpath:
        branchpath += '/'
    ignorelines = ['.hgignore', 'syntax:glob']
    dirs = [''] + [d[0] for d in svn.list_files(branchpath, r)
                   if d[1] == 'd']
    for dir in dirs:
        path = '%s%s' % (branchpath, dir)
        props = svn.list_props(path, r)
        if 'svn:ignore' not in props:
            continue
        lines = props['svn:ignore'].strip().split('\n')
        ignorelines += [dir and (dir + '/' + prop) or prop for prop in lines if prop.strip()]

    repo.wopener('.hgignore', 'w').write('\n'.join(ignorelines) + '\n')


def info(ui, repo, **opts):
    """show Subversion details similar to `svn info'
    """

    if repo is None:
        raise error.RepoError("There is no Mercurial repository"
                              " here (.hg not found)")

    meta = repo.svnmeta()
    hashes = meta.revmap.hashes()

    if opts.get('rev'):
        parent = repo[opts['rev']]
    else:
        parent = util.parentrev(ui, repo, meta, hashes)

    pn = parent.node()
    if pn not in hashes:
        ui.status('Not a child of an svn revision.\n')
        return 0
    r, br = hashes[pn]
    subdir = util.getsvnrev(parent)[40:].split('@')[0]
    remoterepo = svnrepo.svnremoterepo(repo.ui)
    url = meta.layoutobj.remotepath(br, remoterepo.svnurl)
    author = meta.authors.reverselookup(parent.user())
    # cleverly figure out repo root w/o actually contacting the server
    reporoot = url[:len(url)-len(subdir)]
    ui.write('''URL: %(url)s
Repository Root: %(reporoot)s
Repository UUID: %(uuid)s
Revision: %(revision)s
Node Kind: directory
Last Changed Author: %(author)s
Last Changed Rev: %(revision)s
Last Changed Date: %(date)s\n''' %
              {'reporoot': reporoot,
               'uuid': meta.uuid,
               'url': url,
               'author': author,
               'revision': r,
               # TODO I'd like to format this to the user's local TZ if possible
               'date': hgutil.datestr(parent.date(),
                                      '%Y-%m-%d %H:%M:%S %1%2 (%a, %d %b %Y)')
              })


def listauthors(ui, args, authors=None, **opts):
    """list all authors in a Subversion repository
    """
    if not len(args):
        ui.status('No repository specified.\n')
        return
    svn = svnrepo.svnremoterepo(ui, args[0]).svn
    author_set = set()
    for rev in svn.revisions():
        if rev.author is None:
            author_set.add('(no author)')
        else:
            author_set.add(rev.author)
    if authors:
        authorfile = open(authors, 'w')
        authorfile.write('%s=\n' % '=\n'.join(sorted(author_set)))
        authorfile.close()
    else:
        ui.write('%s\n' % '\n'.join(sorted(author_set)))


def _helpgen():
    ret = ['subcommands for Subversion integration', '',
           'list of subcommands:', '']
    for name, func in sorted(table.items()):
        if func.__doc__:
            short_description = func.__doc__.splitlines()[0]
        else:
            short_description = ''
        ret.append(" :%s: %s" % (name, short_description))
    return '\n'.join(ret) + '\n'

def svn(ui, repo, subcommand, *args, **opts):
    '''see detailed help for list of subcommands'''

    # guess command if prefix
    if subcommand not in table:
        candidates = []
        for c in table:
            if c.startswith(subcommand):
                candidates.append(c)
        if len(candidates) == 1:
            subcommand = candidates[0]
        elif not candidates:
            raise error.CommandError('svn',
                                     "unknown subcommand '%s'" % subcommand)
        else:
            raise error.AmbiguousCommand(subcommand, candidates)

    # override subversion credentials
    for key in ('username', 'password'):
        if key in opts:
            ui.setconfig('hgsubversion', key, opts[key])

    try:
        commandfunc = table[subcommand]
        return commandfunc(ui, args=args, repo=repo, **opts)
    except svnwrap.SubversionConnectionException, e:
        raise hgutil.Abort(*e.args)
    except TypeError:
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) == 1:
            ui.status('Bad arguments for subcommand %s\n' % subcommand)
        else:
            raise
    except KeyError, e:
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) == 1:
            ui.status('Unknown subcommand %s\n' % subcommand)
        else:
            raise

svn.optionalrepo=True
svn.norepo = False

table = {
    'genignore': genignore,
    'info': info,
    'listauthors': listauthors,
    'update': update,
    'help': help_,
    'updatemeta': updatemeta,
    'rebuildmeta': rebuildmeta,
    'updateexternals': svnexternals.updateexternals,
    'verify': verify.verify,
}
svn.__doc__ = _helpgen()
