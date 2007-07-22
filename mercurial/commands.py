# commands.py - command processing for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import demandimport; demandimport.enable()
from node import *
from i18n import _
import bisect, os, re, sys, urllib, shlex, stat
import ui, hg, util, revlog, bundlerepo, extensions
import difflib, patch, time, help, mdiff, tempfile
import errno, version, socket
import archival, changegroup, cmdutil, hgweb.server, sshserver

# Commands start here, listed alphabetically

def add(ui, repo, *pats, **opts):
    """add the specified files on the next commit

    Schedule files to be version controlled and added to the repository.

    The files will be added to the repository at the next commit. To
    undo an add before that, see hg revert.

    If no names are given, add all files in the repository.
    """

    names = []
    for src, abs, rel, exact in cmdutil.walk(repo, pats, opts):
        if exact:
            if ui.verbose:
                ui.status(_('adding %s\n') % rel)
            names.append(abs)
        elif repo.dirstate.state(abs) == '?':
            ui.status(_('adding %s\n') % rel)
            names.append(abs)
    if not opts.get('dry_run'):
        repo.add(names)

def addremove(ui, repo, *pats, **opts):
    """add all new files, delete all missing files

    Add all new files and remove all missing files from the repository.

    New files are ignored if they match any of the patterns in .hgignore. As
    with add, these changes take effect at the next commit.

    Use the -s option to detect renamed files.  With a parameter > 0,
    this compares every removed file with every added file and records
    those similar enough as renames.  This option takes a percentage
    between 0 (disabled) and 100 (files must be identical) as its
    parameter.  Detecting renamed files this way can be expensive.
    """
    try:
        sim = float(opts.get('similarity') or 0)
    except ValueError:
        raise util.Abort(_('similarity must be a number'))
    if sim < 0 or sim > 100:
        raise util.Abort(_('similarity must be between 0 and 100'))
    return cmdutil.addremove(repo, pats, opts, similarity=sim/100.)

def annotate(ui, repo, *pats, **opts):
    """show changeset information per file line

    List changes in files, showing the revision id responsible for each line

    This command is useful to discover who did a change or when a change took
    place.

    Without the -a option, annotate will avoid processing files it
    detects as binary. With -a, annotate will generate an annotation
    anyway, probably with undesirable results.
    """
    getdate = util.cachefunc(lambda x: util.datestr(x.date()))

    if not pats:
        raise util.Abort(_('at least one file name or pattern required'))

    opmap = [['user', lambda x: ui.shortuser(x.user())],
             ['number', lambda x: str(x.rev())],
             ['changeset', lambda x: short(x.node())],
             ['date', getdate], ['follow', lambda x: x.path()]]
    if (not opts['user'] and not opts['changeset'] and not opts['date']
        and not opts['follow']):
        opts['number'] = 1

    ctx = repo.changectx(opts['rev'])

    for src, abs, rel, exact in cmdutil.walk(repo, pats, opts,
                                             node=ctx.node()):
        fctx = ctx.filectx(abs)
        if not opts['text'] and util.binary(fctx.data()):
            ui.write(_("%s: binary file\n") % ((pats and rel) or abs))
            continue

        lines = fctx.annotate(follow=opts.get('follow'))
        pieces = []

        for o, f in opmap:
            if opts[o]:
                l = [f(n) for n, dummy in lines]
                if l:
                    m = max(map(len, l))
                    pieces.append(["%*s" % (m, x) for x in l])

        if pieces:
            for p, l in zip(zip(*pieces), lines):
                ui.write("%s: %s" % (" ".join(p), l[1]))

def archive(ui, repo, dest, **opts):
    '''create unversioned archive of a repository revision

    By default, the revision used is the parent of the working
    directory; use "-r" to specify a different revision.

    To specify the type of archive to create, use "-t".  Valid
    types are:

    "files" (default): a directory full of files
    "tar": tar archive, uncompressed
    "tbz2": tar archive, compressed using bzip2
    "tgz": tar archive, compressed using gzip
    "uzip": zip archive, uncompressed
    "zip": zip archive, compressed using deflate

    The exact name of the destination archive or directory is given
    using a format string; see "hg help export" for details.

    Each member added to an archive file has a directory prefix
    prepended.  Use "-p" to specify a format string for the prefix.
    The default is the basename of the archive, with suffixes removed.
    '''

    node = repo.changectx(opts['rev']).node()
    dest = cmdutil.make_filename(repo, dest, node)
    if os.path.realpath(dest) == repo.root:
        raise util.Abort(_('repository root cannot be destination'))
    dummy, matchfn, dummy = cmdutil.matchpats(repo, [], opts)
    kind = opts.get('type') or 'files'
    prefix = opts['prefix']
    if dest == '-':
        if kind == 'files':
            raise util.Abort(_('cannot archive plain files to stdout'))
        dest = sys.stdout
        if not prefix: prefix = os.path.basename(repo.root) + '-%h'
    prefix = cmdutil.make_filename(repo, prefix, node)
    archival.archive(repo, dest, node, kind, not opts['no_decode'],
                     matchfn, prefix)

def backout(ui, repo, node=None, rev=None, **opts):
    '''reverse effect of earlier changeset

    Commit the backed out changes as a new changeset.  The new
    changeset is a child of the backed out changeset.

    If you back out a changeset other than the tip, a new head is
    created.  This head is the parent of the working directory.  If
    you back out an old changeset, your working directory will appear
    old after the backout.  You should merge the backout changeset
    with another head.

    The --merge option remembers the parent of the working directory
    before starting the backout, then merges the new head with that
    changeset afterwards.  This saves you from doing the merge by
    hand.  The result of this merge is not committed, as for a normal
    merge.'''
    if rev and node:
        raise util.Abort(_("please specify just one revision"))

    if not rev:
        rev = node

    if not rev:
        raise util.Abort(_("please specify a revision to backout"))

    cmdutil.bail_if_changed(repo)
    op1, op2 = repo.dirstate.parents()
    if op2 != nullid:
        raise util.Abort(_('outstanding uncommitted merge'))
    node = repo.lookup(rev)
    p1, p2 = repo.changelog.parents(node)
    if p1 == nullid:
        raise util.Abort(_('cannot back out a change with no parents'))
    if p2 != nullid:
        if not opts['parent']:
            raise util.Abort(_('cannot back out a merge changeset without '
                               '--parent'))
        p = repo.lookup(opts['parent'])
        if p not in (p1, p2):
            raise util.Abort(_('%s is not a parent of %s') %
                             (short(p), short(node)))
        parent = p
    else:
        if opts['parent']:
            raise util.Abort(_('cannot use --parent on non-merge changeset'))
        parent = p1
    hg.clean(repo, node, show_stats=False)
    revert_opts = opts.copy()
    revert_opts['date'] = None
    revert_opts['all'] = True
    revert_opts['rev'] = hex(parent)
    revert(ui, repo, **revert_opts)
    commit_opts = opts.copy()
    commit_opts['addremove'] = False
    if not commit_opts['message'] and not commit_opts['logfile']:
        commit_opts['message'] = _("Backed out changeset %s") % (short(node))
        commit_opts['force_editor'] = True
    commit(ui, repo, **commit_opts)
    def nice(node):
        return '%d:%s' % (repo.changelog.rev(node), short(node))
    ui.status(_('changeset %s backs out changeset %s\n') %
              (nice(repo.changelog.tip()), nice(node)))
    if op1 != node:
        if opts['merge']:
            ui.status(_('merging with changeset %s\n') % nice(op1))
            hg.merge(repo, hex(op1))
        else:
            ui.status(_('the backout changeset is a new head - '
                        'do not forget to merge\n'))
            ui.status(_('(use "backout --merge" '
                        'if you want to auto-merge)\n'))

def branch(ui, repo, label=None, **opts):
    """set or show the current branch name

    With no argument, show the current branch name. With one argument,
    set the working directory branch name (the branch does not exist in
    the repository until the next commit).

    Unless --force is specified, branch will not let you set a
    branch name that shadows an existing branch.
    """

    if label:
        if not opts.get('force') and label in repo.branchtags():
            if label not in [p.branch() for p in repo.workingctx().parents()]:
                raise util.Abort(_('a branch of the same name already exists'
                                   ' (use --force to override)'))
        repo.dirstate.setbranch(util.fromlocal(label))
        ui.status(_('marked working directory as branch %s\n') % label)
    else:
        ui.write("%s\n" % util.tolocal(repo.dirstate.branch()))

def branches(ui, repo, active=False):
    """list repository named branches

    List the repository's named branches, indicating which ones are
    inactive.  If active is specified, only show active branches.

    A branch is considered active if it contains unmerged heads.
    """
    b = repo.branchtags()
    heads = dict.fromkeys(repo.heads(), 1)
    l = [((n in heads), repo.changelog.rev(n), n, t) for t, n in b.items()]
    l.sort()
    l.reverse()
    for ishead, r, n, t in l:
        if active and not ishead:
            # If we're only displaying active branches, abort the loop on
            # encountering the first inactive head
            break
        else:
            hexfunc = ui.debugflag and hex or short
            if ui.quiet:
                ui.write("%s\n" % t)
            else:
                spaces = " " * (30 - util.locallen(t))
                # The code only gets here if inactive branches are being
                # displayed or the branch is active.
                isinactive = ((not ishead) and " (inactive)") or ''
                ui.write("%s%s %s:%s%s\n" % (t, spaces, r, hexfunc(n), isinactive))

def bundle(ui, repo, fname, dest=None, **opts):
    """create a changegroup file

    Generate a compressed changegroup file collecting changesets not
    found in the other repository.

    If no destination repository is specified the destination is assumed
    to have all the nodes specified by one or more --base parameters.

    The bundle file can then be transferred using conventional means and
    applied to another repository with the unbundle or pull command.
    This is useful when direct push and pull are not available or when
    exporting an entire repository is undesirable.

    Applying bundles preserves all changeset contents including
    permissions, copy/rename information, and revision history.
    """
    revs = opts.get('rev') or None
    if revs:
        revs = [repo.lookup(rev) for rev in revs]
    base = opts.get('base')
    if base:
        if dest:
            raise util.Abort(_("--base is incompatible with specifiying "
                               "a destination"))
        base = [repo.lookup(rev) for rev in base]
        # create the right base
        # XXX: nodesbetween / changegroup* should be "fixed" instead
        o = []
        has = {nullid: None}
        for n in base:
            has.update(repo.changelog.reachable(n))
        if revs:
            visit = list(revs)
        else:
            visit = repo.changelog.heads()
        seen = {}
        while visit:
            n = visit.pop(0)
            parents = [p for p in repo.changelog.parents(n) if p not in has]
            if len(parents) == 0:
                o.insert(0, n)
            else:
                for p in parents:
                    if p not in seen:
                        seen[p] = 1
                        visit.append(p)
    else:
        cmdutil.setremoteconfig(ui, opts)
        dest, revs = cmdutil.parseurl(
            ui.expandpath(dest or 'default-push', dest or 'default'), revs)
        other = hg.repository(ui, dest)
        o = repo.findoutgoing(other, force=opts['force'])

    if revs:
        cg = repo.changegroupsubset(o, revs, 'bundle')
    else:
        cg = repo.changegroup(o, 'bundle')
    changegroup.writebundle(cg, fname, "HG10BZ")

def cat(ui, repo, file1, *pats, **opts):
    """output the current or given revision of files

    Print the specified files as they were at the given revision.
    If no revision is given, the parent of the working directory is used,
    or tip if no revision is checked out.

    Output may be to a file, in which case the name of the file is
    given using a format string.  The formatting rules are the same as
    for the export command, with the following additions:

    %s   basename of file being printed
    %d   dirname of file being printed, or '.' if in repo root
    %p   root-relative path name of file being printed
    """
    ctx = repo.changectx(opts['rev'])
    err = 1
    for src, abs, rel, exact in cmdutil.walk(repo, (file1,) + pats, opts,
                                             ctx.node()):
        fp = cmdutil.make_file(repo, opts['output'], ctx.node(), pathname=abs)
        fp.write(ctx.filectx(abs).data())
        err = 0
    return err

def clone(ui, source, dest=None, **opts):
    """make a copy of an existing repository

    Create a copy of an existing repository in a new directory.

    If no destination directory name is specified, it defaults to the
    basename of the source.

    The location of the source is added to the new repository's
    .hg/hgrc file, as the default to be used for future pulls.

    For efficiency, hardlinks are used for cloning whenever the source
    and destination are on the same filesystem (note this applies only
    to the repository data, not to the checked out files).  Some
    filesystems, such as AFS, implement hardlinking incorrectly, but
    do not report errors.  In these cases, use the --pull option to
    avoid hardlinking.

    You can safely clone repositories and checked out files using full
    hardlinks with

      $ cp -al REPO REPOCLONE

    which is the fastest way to clone. However, the operation is not
    atomic (making sure REPO is not modified during the operation is
    up to you) and you have to make sure your editor breaks hardlinks
    (Emacs and most Linux Kernel tools do so).

    If you use the -r option to clone up to a specific revision, no
    subsequent revisions will be present in the cloned repository.
    This option implies --pull, even on local repositories.

    See pull for valid source format details.

    It is possible to specify an ssh:// URL as the destination, but no
    .hg/hgrc and working directory will be created on the remote side.
    Look at the help text for the pull command for important details
    about ssh:// URLs.
    """
    cmdutil.setremoteconfig(ui, opts)
    hg.clone(ui, source, dest,
             pull=opts['pull'],
             stream=opts['uncompressed'],
             rev=opts['rev'],
             update=not opts['noupdate'])

def commit(ui, repo, *pats, **opts):
    """commit the specified files or all outstanding changes

    Commit changes to the given files into the repository.

    If a list of files is omitted, all changes reported by "hg status"
    will be committed.

    If no commit message is specified, the editor configured in your hgrc
    or in the EDITOR environment variable is started to enter a message.
    """
    message = cmdutil.logmessage(opts)

    if opts['addremove']:
        cmdutil.addremove(repo, pats, opts)
    fns, match, anypats = cmdutil.matchpats(repo, pats, opts)
    if pats:
        status = repo.status(files=fns, match=match)
        modified, added, removed, deleted, unknown = status[:5]
        files = modified + added + removed
        slist = None
        for f in fns:
            if f == '.':
                continue
            if f not in files:
                rf = repo.wjoin(f)
                try:
                    mode = os.lstat(rf)[stat.ST_MODE]
                except OSError:
                    raise util.Abort(_("file %s not found!") % rf)
                if stat.S_ISDIR(mode):
                    name = f + '/'
                    if slist is None:
                        slist = list(files)
                        slist.sort()
                    i = bisect.bisect(slist, name)
                    if i >= len(slist) or not slist[i].startswith(name):
                        raise util.Abort(_("no match under directory %s!")
                                         % rf)
                elif not (stat.S_ISREG(mode) or stat.S_ISLNK(mode)):
                    raise util.Abort(_("can't commit %s: "
                                       "unsupported file type!") % rf)
                elif repo.dirstate.state(f) == '?':
                    raise util.Abort(_("file %s not tracked!") % rf)
    else:
        files = []
    try:
        repo.commit(files, message, opts['user'], opts['date'], match,
                    force_editor=opts.get('force_editor'))
    except ValueError, inst:
        raise util.Abort(str(inst))

def docopy(ui, repo, pats, opts, wlock):
    # called with the repo lock held
    #
    # hgsep => pathname that uses "/" to separate directories
    # ossep => pathname that uses os.sep to separate directories
    cwd = repo.getcwd()
    errors = 0
    copied = []
    targets = {}

    # abs: hgsep
    # rel: ossep
    # return: hgsep
    def okaytocopy(abs, rel, exact):
        reasons = {'?': _('is not managed'),
                   'r': _('has been marked for remove')}
        state = repo.dirstate.state(abs)
        reason = reasons.get(state)
        if reason:
            if exact:
                ui.warn(_('%s: not copying - file %s\n') % (rel, reason))
        else:
            if state == 'a':
                origsrc = repo.dirstate.copied(abs)
                if origsrc is not None:
                    return origsrc
            return abs

    # origsrc: hgsep
    # abssrc: hgsep
    # relsrc: ossep
    # otarget: ossep
    def copy(origsrc, abssrc, relsrc, otarget, exact):
        abstarget = util.canonpath(repo.root, cwd, otarget)
        reltarget = repo.pathto(abstarget, cwd)
        prevsrc = targets.get(abstarget)
        src = repo.wjoin(abssrc)
        target = repo.wjoin(abstarget)
        if prevsrc is not None:
            ui.warn(_('%s: not overwriting - %s collides with %s\n') %
                    (reltarget, repo.pathto(abssrc, cwd),
                     repo.pathto(prevsrc, cwd)))
            return
        if (not opts['after'] and os.path.exists(target) or
            opts['after'] and repo.dirstate.state(abstarget) not in '?ar'):
            if not opts['force']:
                ui.warn(_('%s: not overwriting - file exists\n') %
                        reltarget)
                return
            if not opts['after'] and not opts.get('dry_run'):
                os.unlink(target)
        if opts['after']:
            if not os.path.exists(target):
                return
        else:
            targetdir = os.path.dirname(target) or '.'
            if not os.path.isdir(targetdir) and not opts.get('dry_run'):
                os.makedirs(targetdir)
            try:
                restore = repo.dirstate.state(abstarget) == 'r'
                if restore and not opts.get('dry_run'):
                    repo.undelete([abstarget], wlock)
                try:
                    if not opts.get('dry_run'):
                        util.copyfile(src, target)
                    restore = False
                finally:
                    if restore:
                        repo.remove([abstarget], wlock=wlock)
            except IOError, inst:
                if inst.errno == errno.ENOENT:
                    ui.warn(_('%s: deleted in working copy\n') % relsrc)
                else:
                    ui.warn(_('%s: cannot copy - %s\n') %
                            (relsrc, inst.strerror))
                    errors += 1
                    return
        if ui.verbose or not exact:
            ui.status(_('copying %s to %s\n') % (relsrc, reltarget))
        targets[abstarget] = abssrc
        if abstarget != origsrc:
            if repo.dirstate.state(origsrc) == 'a':
                if not ui.quiet:
                    ui.warn(_("%s has not been committed yet, so no copy "
                              "data will be stored for %s.\n")
                            % (repo.pathto(origsrc, cwd), reltarget))
                if abstarget not in repo.dirstate and not opts.get('dry_run'):
                    repo.add([abstarget], wlock)
            elif not opts.get('dry_run'):
                repo.copy(origsrc, abstarget, wlock)
        copied.append((abssrc, relsrc, exact))

    # pat: ossep
    # dest ossep
    # srcs: list of (hgsep, hgsep, ossep, bool)
    # return: function that takes hgsep and returns ossep
    def targetpathfn(pat, dest, srcs):
        if os.path.isdir(pat):
            abspfx = util.canonpath(repo.root, cwd, pat)
            abspfx = util.localpath(abspfx)
            if destdirexists:
                striplen = len(os.path.split(abspfx)[0])
            else:
                striplen = len(abspfx)
            if striplen:
                striplen += len(os.sep)
            res = lambda p: os.path.join(dest, util.localpath(p)[striplen:])
        elif destdirexists:
            res = lambda p: os.path.join(dest,
                                         os.path.basename(util.localpath(p)))
        else:
            res = lambda p: dest
        return res

    # pat: ossep
    # dest ossep
    # srcs: list of (hgsep, hgsep, ossep, bool)
    # return: function that takes hgsep and returns ossep
    def targetpathafterfn(pat, dest, srcs):
        if util.patkind(pat, None)[0]:
            # a mercurial pattern
            res = lambda p: os.path.join(dest,
                                         os.path.basename(util.localpath(p)))
        else:
            abspfx = util.canonpath(repo.root, cwd, pat)
            if len(abspfx) < len(srcs[0][0]):
                # A directory. Either the target path contains the last
                # component of the source path or it does not.
                def evalpath(striplen):
                    score = 0
                    for s in srcs:
                        t = os.path.join(dest, util.localpath(s[0])[striplen:])
                        if os.path.exists(t):
                            score += 1
                    return score

                abspfx = util.localpath(abspfx)
                striplen = len(abspfx)
                if striplen:
                    striplen += len(os.sep)
                if os.path.isdir(os.path.join(dest, os.path.split(abspfx)[1])):
                    score = evalpath(striplen)
                    striplen1 = len(os.path.split(abspfx)[0])
                    if striplen1:
                        striplen1 += len(os.sep)
                    if evalpath(striplen1) > score:
                        striplen = striplen1
                res = lambda p: os.path.join(dest,
                                             util.localpath(p)[striplen:])
            else:
                # a file
                if destdirexists:
                    res = lambda p: os.path.join(dest,
                                        os.path.basename(util.localpath(p)))
                else:
                    res = lambda p: dest
        return res


    pats = util.expand_glob(pats)
    if not pats:
        raise util.Abort(_('no source or destination specified'))
    if len(pats) == 1:
        raise util.Abort(_('no destination specified'))
    dest = pats.pop()
    destdirexists = os.path.isdir(dest)
    if (len(pats) > 1 or util.patkind(pats[0], None)[0]) and not destdirexists:
        raise util.Abort(_('with multiple sources, destination must be an '
                         'existing directory'))
    if opts['after']:
        tfn = targetpathafterfn
    else:
        tfn = targetpathfn
    copylist = []
    for pat in pats:
        srcs = []
        for tag, abssrc, relsrc, exact in cmdutil.walk(repo, [pat], opts,
                                                       globbed=True):
            origsrc = okaytocopy(abssrc, relsrc, exact)
            if origsrc:
                srcs.append((origsrc, abssrc, relsrc, exact))
        if not srcs:
            continue
        copylist.append((tfn(pat, dest, srcs), srcs))
    if not copylist:
        raise util.Abort(_('no files to copy'))

    for targetpath, srcs in copylist:
        for origsrc, abssrc, relsrc, exact in srcs:
            copy(origsrc, abssrc, relsrc, targetpath(abssrc), exact)

    if errors:
        ui.warn(_('(consider using --after)\n'))
    return errors, copied

def copy(ui, repo, *pats, **opts):
    """mark files as copied for the next commit

    Mark dest as having copies of source files.  If dest is a
    directory, copies are put in that directory.  If dest is a file,
    there can only be one source.

    By default, this command copies the contents of files as they
    stand in the working directory.  If invoked with --after, the
    operation is recorded, but no copying is performed.

    This command takes effect in the next commit. To undo a copy
    before that, see hg revert.
    """
    wlock = repo.wlock(0)
    errs, copied = docopy(ui, repo, pats, opts, wlock)
    return errs

def debugancestor(ui, index, rev1, rev2):
    """find the ancestor revision of two revisions in a given index"""
    r = revlog.revlog(util.opener(os.getcwd(), audit=False), index)
    a = r.ancestor(r.lookup(rev1), r.lookup(rev2))
    ui.write("%d:%s\n" % (r.rev(a), hex(a)))

def debugcomplete(ui, cmd='', **opts):
    """returns the completion list associated with the given command"""

    if opts['options']:
        options = []
        otables = [globalopts]
        if cmd:
            aliases, entry = cmdutil.findcmd(ui, cmd)
            otables.append(entry[1])
        for t in otables:
            for o in t:
                if o[0]:
                    options.append('-%s' % o[0])
                options.append('--%s' % o[1])
        ui.write("%s\n" % "\n".join(options))
        return

    clist = cmdutil.findpossible(ui, cmd).keys()
    clist.sort()
    ui.write("%s\n" % "\n".join(clist))

def debugrebuildstate(ui, repo, rev=""):
    """rebuild the dirstate as it would look like for the given revision"""
    if rev == "":
        rev = repo.changelog.tip()
    ctx = repo.changectx(rev)
    files = ctx.manifest()
    wlock = repo.wlock()
    repo.dirstate.rebuild(rev, files)

def debugcheckstate(ui, repo):
    """validate the correctness of the current dirstate"""
    parent1, parent2 = repo.dirstate.parents()
    dc = repo.dirstate
    m1 = repo.changectx(parent1).manifest()
    m2 = repo.changectx(parent2).manifest()
    errors = 0
    for f in dc:
        state = repo.dirstate.state(f)
        if state in "nr" and f not in m1:
            ui.warn(_("%s in state %s, but not in manifest1\n") % (f, state))
            errors += 1
        if state in "a" and f in m1:
            ui.warn(_("%s in state %s, but also in manifest1\n") % (f, state))
            errors += 1
        if state in "m" and f not in m1 and f not in m2:
            ui.warn(_("%s in state %s, but not in either manifest\n") %
                    (f, state))
            errors += 1
    for f in m1:
        state = repo.dirstate.state(f)
        if state not in "nrm":
            ui.warn(_("%s in manifest1, but listed as state %s") % (f, state))
            errors += 1
    if errors:
        error = _(".hg/dirstate inconsistent with current parent's manifest")
        raise util.Abort(error)

def showconfig(ui, repo, *values, **opts):
    """show combined config settings from all hgrc files

    With no args, print names and values of all config items.

    With one arg of the form section.name, print just the value of
    that config item.

    With multiple args, print names and values of all config items
    with matching section names."""

    untrusted = bool(opts.get('untrusted'))
    if values:
        if len([v for v in values if '.' in v]) > 1:
            raise util.Abort(_('only one config item permitted'))
    for section, name, value in ui.walkconfig(untrusted=untrusted):
        sectname = section + '.' + name
        if values:
            for v in values:
                if v == section:
                    ui.write('%s=%s\n' % (sectname, value))
                elif v == sectname:
                    ui.write(value, '\n')
        else:
            ui.write('%s=%s\n' % (sectname, value))

def debugsetparents(ui, repo, rev1, rev2=None):
    """manually set the parents of the current working directory

    This is useful for writing repository conversion tools, but should
    be used with care.
    """

    if not rev2:
        rev2 = hex(nullid)

    wlock = repo.wlock()
    try:
        repo.dirstate.setparents(repo.lookup(rev1), repo.lookup(rev2))
    finally:
        wlock.release()

def debugstate(ui, repo):
    """show the contents of the current dirstate"""
    dc = repo.dirstate
    for file_ in dc:
        if dc[file_][3] == -1:
            # Pad or slice to locale representation
            locale_len = len(time.strftime("%x %X", time.localtime(0)))
            timestr = 'unset'
            timestr = timestr[:locale_len] + ' '*(locale_len - len(timestr))
        else:
            timestr = time.strftime("%x %X", time.localtime(dc[file_][3]))
        ui.write("%c %3o %10d %s %s\n"
                 % (dc[file_][0], dc[file_][1] & 0777, dc[file_][2],
                    timestr, file_))
    for f in repo.dirstate.copies():
        ui.write(_("copy: %s -> %s\n") % (repo.dirstate.copied(f), f))

def debugdata(ui, file_, rev):
    """dump the contents of a data file revision"""
    r = revlog.revlog(util.opener(os.getcwd(), audit=False), file_[:-2] + ".i")
    try:
        ui.write(r.revision(r.lookup(rev)))
    except KeyError:
        raise util.Abort(_('invalid revision identifier %s') % rev)

def debugdate(ui, date, range=None, **opts):
    """parse and display a date"""
    if opts["extended"]:
        d = util.parsedate(date, util.extendeddateformats)
    else:
        d = util.parsedate(date)
    ui.write("internal: %s %s\n" % d)
    ui.write("standard: %s\n" % util.datestr(d))
    if range:
        m = util.matchdate(range)
        ui.write("match: %s\n" % m(d[0]))

def debugindex(ui, file_):
    """dump the contents of an index file"""
    r = revlog.revlog(util.opener(os.getcwd(), audit=False), file_)
    ui.write("   rev    offset  length   base linkrev" +
             " nodeid       p1           p2\n")
    for i in xrange(r.count()):
        node = r.node(i)
        pp = r.parents(node)
        ui.write("% 6d % 9d % 7d % 6d % 7d %s %s %s\n" % (
                i, r.start(i), r.length(i), r.base(i), r.linkrev(node),
            short(node), short(pp[0]), short(pp[1])))

def debugindexdot(ui, file_):
    """dump an index DAG as a .dot file"""
    r = revlog.revlog(util.opener(os.getcwd(), audit=False), file_)
    ui.write("digraph G {\n")
    for i in xrange(r.count()):
        node = r.node(i)
        pp = r.parents(node)
        ui.write("\t%d -> %d\n" % (r.rev(pp[0]), i))
        if pp[1] != nullid:
            ui.write("\t%d -> %d\n" % (r.rev(pp[1]), i))
    ui.write("}\n")

def debuginstall(ui):
    '''test Mercurial installation'''

    def writetemp(contents):
        (fd, name) = tempfile.mkstemp()
        f = os.fdopen(fd, "wb")
        f.write(contents)
        f.close()
        return name

    problems = 0

    # encoding
    ui.status(_("Checking encoding (%s)...\n") % util._encoding)
    try:
        util.fromlocal("test")
    except util.Abort, inst:
        ui.write(" %s\n" % inst)
        ui.write(_(" (check that your locale is properly set)\n"))
        problems += 1

    # compiled modules
    ui.status(_("Checking extensions...\n"))
    try:
        import bdiff, mpatch, base85
    except Exception, inst:
        ui.write(" %s\n" % inst)
        ui.write(_(" One or more extensions could not be found"))
        ui.write(_(" (check that you compiled the extensions)\n"))
        problems += 1

    # templates
    ui.status(_("Checking templates...\n"))
    try:
        import templater
        t = templater.templater(templater.templatepath("map-cmdline.default"))
    except Exception, inst:
        ui.write(" %s\n" % inst)
        ui.write(_(" (templates seem to have been installed incorrectly)\n"))
        problems += 1

    # patch
    ui.status(_("Checking patch...\n"))
    patcher = ui.config('ui', 'patch')
    patcher = ((patcher and util.find_exe(patcher)) or
               util.find_exe('gpatch') or
               util.find_exe('patch'))
    if not patcher:
        ui.write(_(" Can't find patch or gpatch in PATH\n"))
        ui.write(_(" (specify a patch utility in your .hgrc file)\n"))
        problems += 1
    else:
        # actually attempt a patch here
        a = "1\n2\n3\n4\n"
        b = "1\n2\n3\ninsert\n4\n"
        fa = writetemp(a)
        d = mdiff.unidiff(a, None, b, None, os.path.basename(fa))
        fd = writetemp(d)

        files = {}
        try:
            patch.patch(fd, ui, cwd=os.path.dirname(fa), files=files)
        except util.Abort, e:
            ui.write(_(" patch call failed:\n"))
            ui.write(" " + str(e) + "\n")
            problems += 1
        else:
            if list(files) != [os.path.basename(fa)]:
                ui.write(_(" unexpected patch output!"))
                ui.write(_(" (you may have an incompatible version of patch)\n"))
                problems += 1
            a = file(fa).read()
            if a != b:
                ui.write(_(" patch test failed!"))
                ui.write(_(" (you may have an incompatible version of patch)\n"))
                problems += 1

        os.unlink(fa)
        os.unlink(fd)

    # merge helper
    ui.status(_("Checking merge helper...\n"))
    cmd = (os.environ.get("HGMERGE") or ui.config("ui", "merge")
           or "hgmerge")
    cmdpath = util.find_exe(cmd) or util.find_exe(cmd.split()[0])
    if not cmdpath:
        if cmd == 'hgmerge':
            ui.write(_(" No merge helper set and can't find default"
                       " hgmerge script in PATH\n"))
            ui.write(_(" (specify a merge helper in your .hgrc file)\n"))
        else:
            ui.write(_(" Can't find merge helper '%s' in PATH\n") % cmd)
            ui.write(_(" (specify a merge helper in your .hgrc file)\n"))
            problems += 1
    else:
        # actually attempt a patch here
        fa = writetemp("1\n2\n3\n4\n")
        fl = writetemp("1\n2\n3\ninsert\n4\n")
        fr = writetemp("begin\n1\n2\n3\n4\n")
        r = util.system('%s "%s" "%s" "%s"' % (cmd, fl, fa, fr))
        if r:
            ui.write(_(" Got unexpected merge error %d!\n") % r)
            problems += 1
        m = file(fl).read()
        if m != "begin\n1\n2\n3\ninsert\n4\n":
            ui.write(_(" Got unexpected merge results!\n"))
            ui.write(_(" (your merge helper may have the"
                       " wrong argument order)\n"))
            ui.write(_(" Result: %r\n") % m)
            problems += 1
        os.unlink(fa)
        os.unlink(fl)
        os.unlink(fr)

    # editor
    ui.status(_("Checking commit editor...\n"))
    editor = (os.environ.get("HGEDITOR") or
              ui.config("ui", "editor") or
              os.environ.get("EDITOR", "vi"))
    cmdpath = util.find_exe(editor) or util.find_exe(editor.split()[0])
    if not cmdpath:
        if editor == 'vi':
            ui.write(_(" No commit editor set and can't find vi in PATH\n"))
            ui.write(_(" (specify a commit editor in your .hgrc file)\n"))
        else:
            ui.write(_(" Can't find editor '%s' in PATH\n") % editor)
            ui.write(_(" (specify a commit editor in your .hgrc file)\n"))
            problems += 1

    # check username
    ui.status(_("Checking username...\n"))
    user = os.environ.get("HGUSER")
    if user is None:
        user = ui.config("ui", "username")
    if user is None:
        user = os.environ.get("EMAIL")
    if not user:
        ui.warn(" ")
        ui.username()
        ui.write(_(" (specify a username in your .hgrc file)\n"))

    if not problems:
        ui.status(_("No problems detected\n"))
    else:
        ui.write(_("%s problems detected,"
                   " please check your install!\n") % problems)

    return problems

def debugrename(ui, repo, file1, *pats, **opts):
    """dump rename information"""

    ctx = repo.changectx(opts.get('rev', 'tip'))
    for src, abs, rel, exact in cmdutil.walk(repo, (file1,) + pats, opts,
                                             ctx.node()):
        m = ctx.filectx(abs).renamed()
        if m:
            ui.write(_("%s renamed from %s:%s\n") % (rel, m[0], hex(m[1])))
        else:
            ui.write(_("%s not renamed\n") % rel)

def debugwalk(ui, repo, *pats, **opts):
    """show how files match on given patterns"""
    items = list(cmdutil.walk(repo, pats, opts))
    if not items:
        return
    fmt = '%%s  %%-%ds  %%-%ds  %%s' % (
        max([len(abs) for (src, abs, rel, exact) in items]),
        max([len(rel) for (src, abs, rel, exact) in items]))
    for src, abs, rel, exact in items:
        line = fmt % (src, abs, rel, exact and 'exact' or '')
        ui.write("%s\n" % line.rstrip())

def diff(ui, repo, *pats, **opts):
    """diff repository (or selected files)

    Show differences between revisions for the specified files.

    Differences between files are shown using the unified diff format.

    NOTE: diff may generate unexpected results for merges, as it will
    default to comparing against the working directory's first parent
    changeset if no revisions are specified.

    When two revision arguments are given, then changes are shown
    between those revisions. If only one revision is specified then
    that revision is compared to the working directory, and, when no
    revisions are specified, the working directory files are compared
    to its parent.

    Without the -a option, diff will avoid generating diffs of files
    it detects as binary. With -a, diff will generate a diff anyway,
    probably with undesirable results.
    """
    node1, node2 = cmdutil.revpair(repo, opts['rev'])

    fns, matchfn, anypats = cmdutil.matchpats(repo, pats, opts)

    patch.diff(repo, node1, node2, fns, match=matchfn,
               opts=patch.diffopts(ui, opts))

def export(ui, repo, *changesets, **opts):
    """dump the header and diffs for one or more changesets

    Print the changeset header and diffs for one or more revisions.

    The information shown in the changeset header is: author,
    changeset hash, parent(s) and commit comment.

    NOTE: export may generate unexpected diff output for merge changesets,
    as it will compare the merge changeset against its first parent only.

    Output may be to a file, in which case the name of the file is
    given using a format string.  The formatting rules are as follows:

    %%   literal "%" character
    %H   changeset hash (40 bytes of hexadecimal)
    %N   number of patches being generated
    %R   changeset revision number
    %b   basename of the exporting repository
    %h   short-form changeset hash (12 bytes of hexadecimal)
    %n   zero-padded sequence number, starting at 1
    %r   zero-padded changeset revision number

    Without the -a option, export will avoid generating diffs of files
    it detects as binary. With -a, export will generate a diff anyway,
    probably with undesirable results.

    With the --switch-parent option, the diff will be against the second
    parent. It can be useful to review a merge.
    """
    if not changesets:
        raise util.Abort(_("export requires at least one changeset"))
    revs = cmdutil.revrange(repo, changesets)
    if len(revs) > 1:
        ui.note(_('exporting patches:\n'))
    else:
        ui.note(_('exporting patch:\n'))
    patch.export(repo, revs, template=opts['output'],
                 switch_parent=opts['switch_parent'],
                 opts=patch.diffopts(ui, opts))

def grep(ui, repo, pattern, *pats, **opts):
    """search for a pattern in specified files and revisions

    Search revisions of files for a regular expression.

    This command behaves differently than Unix grep.  It only accepts
    Python/Perl regexps.  It searches repository history, not the
    working directory.  It always prints the revision number in which
    a match appears.

    By default, grep only prints output for the first revision of a
    file in which it finds a match.  To get it to print every revision
    that contains a change in match status ("-" for a match that
    becomes a non-match, or "+" for a non-match that becomes a match),
    use the --all flag.
    """
    reflags = 0
    if opts['ignore_case']:
        reflags |= re.I
    try:
        regexp = re.compile(pattern, reflags)
    except Exception, inst:
        ui.warn(_("grep: invalid match pattern: %s!\n") % inst)
        return None
    sep, eol = ':', '\n'
    if opts['print0']:
        sep = eol = '\0'

    fcache = {}
    def getfile(fn):
        if fn not in fcache:
            fcache[fn] = repo.file(fn)
        return fcache[fn]

    def matchlines(body):
        begin = 0
        linenum = 0
        while True:
            match = regexp.search(body, begin)
            if not match:
                break
            mstart, mend = match.span()
            linenum += body.count('\n', begin, mstart) + 1
            lstart = body.rfind('\n', begin, mstart) + 1 or begin
            lend = body.find('\n', mend)
            yield linenum, mstart - lstart, mend - lstart, body[lstart:lend]
            begin = lend + 1

    class linestate(object):
        def __init__(self, line, linenum, colstart, colend):
            self.line = line
            self.linenum = linenum
            self.colstart = colstart
            self.colend = colend

        def __eq__(self, other):
            return self.line == other.line

    matches = {}
    copies = {}
    def grepbody(fn, rev, body):
        matches[rev].setdefault(fn, [])
        m = matches[rev][fn]
        for lnum, cstart, cend, line in matchlines(body):
            s = linestate(line, lnum, cstart, cend)
            m.append(s)

    def difflinestates(a, b):
        sm = difflib.SequenceMatcher(None, a, b)
        for tag, alo, ahi, blo, bhi in sm.get_opcodes():
            if tag == 'insert':
                for i in xrange(blo, bhi):
                    yield ('+', b[i])
            elif tag == 'delete':
                for i in xrange(alo, ahi):
                    yield ('-', a[i])
            elif tag == 'replace':
                for i in xrange(alo, ahi):
                    yield ('-', a[i])
                for i in xrange(blo, bhi):
                    yield ('+', b[i])

    prev = {}
    def display(fn, rev, states, prevstates):
        found = False
        filerevmatches = {}
        r = prev.get(fn, -1)
        if opts['all']:
            iter = difflinestates(states, prevstates)
        else:
            iter = [('', l) for l in prevstates]
        for change, l in iter:
            cols = [fn, str(r)]
            if opts['line_number']:
                cols.append(str(l.linenum))
            if opts['all']:
                cols.append(change)
            if opts['user']:
                cols.append(ui.shortuser(get(r)[1]))
            if opts['files_with_matches']:
                c = (fn, r)
                if c in filerevmatches:
                    continue
                filerevmatches[c] = 1
            else:
                cols.append(l.line)
            ui.write(sep.join(cols), eol)
            found = True
        return found

    fstate = {}
    skip = {}
    get = util.cachefunc(lambda r: repo.changectx(r).changeset())
    changeiter, matchfn = cmdutil.walkchangerevs(ui, repo, pats, get, opts)
    found = False
    follow = opts.get('follow')
    for st, rev, fns in changeiter:
        if st == 'window':
            matches.clear()
        elif st == 'add':
            mf = repo.changectx(rev).manifest()
            matches[rev] = {}
            for fn in fns:
                if fn in skip:
                    continue
                fstate.setdefault(fn, {})
                try:
                    grepbody(fn, rev, getfile(fn).read(mf[fn]))
                    if follow:
                        copied = getfile(fn).renamed(mf[fn])
                        if copied:
                            copies.setdefault(rev, {})[fn] = copied[0]
                except KeyError:
                    pass
        elif st == 'iter':
            states = matches[rev].items()
            states.sort()
            for fn, m in states:
                copy = copies.get(rev, {}).get(fn)
                if fn in skip:
                    if copy:
                        skip[copy] = True
                    continue
                if fn in prev or fstate[fn]:
                    r = display(fn, rev, m, fstate[fn])
                    found = found or r
                    if r and not opts['all']:
                        skip[fn] = True
                        if copy:
                            skip[copy] = True
                fstate[fn] = m
                if copy:
                    fstate[copy] = m
                prev[fn] = rev

    fstate = fstate.items()
    fstate.sort()
    for fn, state in fstate:
        if fn in skip:
            continue
        if fn not in copies.get(prev[fn], {}):
            found = display(fn, rev, {}, state) or found
    return (not found and 1) or 0

def heads(ui, repo, *branchrevs, **opts):
    """show current repository heads or show branch heads

    With no arguments, show all repository head changesets.

    If branch or revisions names are given this will show the heads of
    the specified branches or the branches those revisions are tagged
    with.

    Repository "heads" are changesets that don't have child
    changesets. They are where development generally takes place and
    are the usual targets for update and merge operations.

    Branch heads are changesets that have a given branch tag, but have
    no child changesets with that tag.  They are usually where
    development on the given branch takes place.
    """
    if opts['rev']:
        start = repo.lookup(opts['rev'])
    else:
        start = None
    if not branchrevs:
        # Assume we're looking repo-wide heads if no revs were specified.
        heads = repo.heads(start)
    else:
        heads = []
        visitedset = util.set()
        for branchrev in branchrevs:
            branch = repo.changectx(branchrev).branch()
            if branch in visitedset:
                continue
            visitedset.add(branch)
            bheads = repo.branchheads(branch, start)
            if not bheads:
                if branch != branchrev:
                    ui.warn(_("no changes on branch %s containing %s are "
                              "reachable from %s\n")
                            % (branch, branchrev, opts['rev']))
                else:
                    ui.warn(_("no changes on branch %s are reachable from %s\n")
                            % (branch, opts['rev']))
            heads.extend(bheads)
    if not heads:
        return 1
    displayer = cmdutil.show_changeset(ui, repo, opts)
    for n in heads:
        displayer.show(changenode=n)

def help_(ui, name=None, with_version=False):
    """show help for a command, extension, or list of commands

    With no arguments, print a list of commands and short help.

    Given a command name, print help for that command.

    Given an extension name, print help for that extension, and the
    commands it provides."""
    option_lists = []

    def addglobalopts(aliases):
        if ui.verbose:
            option_lists.append((_("global options:"), globalopts))
            if name == 'shortlist':
                option_lists.append((_('use "hg help" for the full list '
                                       'of commands'), ()))
        else:
            if name == 'shortlist':
                msg = _('use "hg help" for the full list of commands '
                        'or "hg -v" for details')
            elif aliases:
                msg = _('use "hg -v help%s" to show aliases and '
                        'global options') % (name and " " + name or "")
            else:
                msg = _('use "hg -v help %s" to show global options') % name
            option_lists.append((msg, ()))

    def helpcmd(name):
        if with_version:
            version_(ui)
            ui.write('\n')
        aliases, i = cmdutil.findcmd(ui, name)
        # synopsis
        ui.write("%s\n\n" % i[2])

        # description
        doc = i[0].__doc__
        if not doc:
            doc = _("(No help text available)")
        if ui.quiet:
            doc = doc.splitlines(0)[0]
        ui.write("%s\n" % doc.rstrip())

        if not ui.quiet:
            # aliases
            if len(aliases) > 1:
                ui.write(_("\naliases: %s\n") % ', '.join(aliases[1:]))

            # options
            if i[1]:
                option_lists.append((_("options:\n"), i[1]))

            addglobalopts(False)

    def helplist(header, select=None):
        h = {}
        cmds = {}
        for c, e in table.items():
            f = c.split("|", 1)[0]
            if select and not select(f):
                continue
            if name == "shortlist" and not f.startswith("^"):
                continue
            f = f.lstrip("^")
            if not ui.debugflag and f.startswith("debug"):
                continue
            doc = e[0].__doc__
            if not doc:
                doc = _("(No help text available)")
            h[f] = doc.splitlines(0)[0].rstrip()
            cmds[f] = c.lstrip("^")

        if not h:
            ui.status(_('no commands defined\n'))
            return

        ui.status(header)
        fns = h.keys()
        fns.sort()
        m = max(map(len, fns))
        for f in fns:
            if ui.verbose:
                commands = cmds[f].replace("|",", ")
                ui.write(" %s:\n      %s\n"%(commands, h[f]))
            else:
                ui.write(' %-*s   %s\n' % (m, f, h[f]))

        if not ui.quiet:
            addglobalopts(True)

    def helptopic(name):
        v = None
        for i in help.helptable:
            l = i.split('|')
            if name in l:
                v = i
                header = l[-1]
        if not v:
            raise cmdutil.UnknownCommand(name)

        # description
        doc = help.helptable[v]
        if not doc:
            doc = _("(No help text available)")
        if callable(doc):
            doc = doc()

        ui.write("%s\n" % header)
        ui.write("%s\n" % doc.rstrip())

    def helpext(name):
        try:
            mod = extensions.find(name)
        except KeyError:
            raise cmdutil.UnknownCommand(name)

        doc = (mod.__doc__ or _('No help text available')).splitlines(0)
        ui.write(_('%s extension - %s\n') % (name.split('.')[-1], doc[0]))
        for d in doc[1:]:
            ui.write(d, '\n')

        ui.status('\n')

        try:
            ct = mod.cmdtable
        except AttributeError:
            ct = {}

        modcmds = dict.fromkeys([c.split('|', 1)[0] for c in ct])
        helplist(_('list of commands:\n\n'), modcmds.has_key)

    if name and name != 'shortlist':
        i = None
        for f in (helpcmd, helptopic, helpext):
            try:
                f(name)
                i = None
                break
            except cmdutil.UnknownCommand, inst:
                i = inst
        if i:
            raise i

    else:
        # program name
        if ui.verbose or with_version:
            version_(ui)
        else:
            ui.status(_("Mercurial Distributed SCM\n"))
        ui.status('\n')

        # list of commands
        if name == "shortlist":
            header = _('basic commands:\n\n')
        else:
            header = _('list of commands:\n\n')

        helplist(header)

    # list all option lists
    opt_output = []
    for title, options in option_lists:
        opt_output.append(("\n%s" % title, None))
        for shortopt, longopt, default, desc in options:
            if "DEPRECATED" in desc and not ui.verbose: continue
            opt_output.append(("%2s%s" % (shortopt and "-%s" % shortopt,
                                          longopt and " --%s" % longopt),
                               "%s%s" % (desc,
                                         default
                                         and _(" (default: %s)") % default
                                         or "")))

    if opt_output:
        opts_len = max([len(line[0]) for line in opt_output if line[1]] or [0])
        for first, second in opt_output:
            if second:
                ui.write(" %-*s  %s\n" % (opts_len, first, second))
            else:
                ui.write("%s\n" % first)

def identify(ui, repo, source=None,
             rev=None, num=None, id=None, branch=None, tags=None):
    """identify the working copy or specified revision

    With no revision, print a summary of the current state of the repo.

    With a path, do a lookup in another repository.

    This summary identifies the repository state using one or two parent
    hash identifiers, followed by a "+" if there are uncommitted changes
    in the working directory, a list of tags for this revision and a branch
    name for non-default branches.
    """

    hexfunc = ui.debugflag and hex or short
    default = not (num or id or branch or tags)
    output = []

    if source:
        source, revs = cmdutil.parseurl(ui.expandpath(source), [])
        srepo = hg.repository(ui, source)
        if not rev and revs:
            rev = revs[0]
        if not rev:
            rev = "tip"
        if num or branch or tags:
            raise util.Abort(
                "can't query remote revision number, branch, or tags")
        output = [hexfunc(srepo.lookup(rev))]
    elif not rev:
        ctx = repo.workingctx()
        parents = ctx.parents()
        changed = False
        if default or id or num:
            changed = ctx.files() + ctx.deleted()
        if default or id:
            output = ["%s%s" % ('+'.join([hexfunc(p.node()) for p in parents]),
                                (changed) and "+" or "")]
        if num:
            output.append("%s%s" % ('+'.join([str(p.rev()) for p in parents]),
                                    (changed) and "+" or ""))
    else:
        ctx = repo.changectx(rev)
        if default or id:
            output = [hexfunc(ctx.node())]
        if num:
            output.append(str(ctx.rev()))

    if not source and default and not ui.quiet:
        b = util.tolocal(ctx.branch())
        if b != 'default':
            output.append("(%s)" % b)

        # multiple tags for a single parent separated by '/'
        t = "/".join(ctx.tags())
        if t:
            output.append(t)

    if branch:
        output.append(util.tolocal(ctx.branch()))

    if tags:
        output.extend(ctx.tags())

    ui.write("%s\n" % ' '.join(output))

def import_(ui, repo, patch1, *patches, **opts):
    """import an ordered set of patches

    Import a list of patches and commit them individually.

    If there are outstanding changes in the working directory, import
    will abort unless given the -f flag.

    You can import a patch straight from a mail message.  Even patches
    as attachments work (body part must be type text/plain or
    text/x-patch to be used).  From and Subject headers of email
    message are used as default committer and commit message.  All
    text/plain body parts before first diff are added to commit
    message.

    If the imported patch was generated by hg export, user and description
    from patch override values from message headers and body.  Values
    given on command line with -m and -u override these.

    If --exact is specified, import will set the working directory
    to the parent of each patch before applying it, and will abort
    if the resulting changeset has a different ID than the one
    recorded in the patch. This may happen due to character set
    problems or other deficiencies in the text patch format.

    To read a patch from standard input, use patch name "-".
    """
    patches = (patch1,) + patches

    if opts.get('exact') or not opts['force']:
        cmdutil.bail_if_changed(repo)

    d = opts["base"]
    strip = opts["strip"]

    wlock = repo.wlock()
    lock = repo.lock()

    for p in patches:
        pf = os.path.join(d, p)

        if pf == '-':
            ui.status(_("applying patch from stdin\n"))
            tmpname, message, user, date, branch, nodeid, p1, p2 = patch.extract(ui, sys.stdin)
        else:
            ui.status(_("applying %s\n") % p)
            tmpname, message, user, date, branch, nodeid, p1, p2 = patch.extract(ui, file(pf, 'rb'))

        if tmpname is None:
            raise util.Abort(_('no diffs found'))

        try:
            cmdline_message = cmdutil.logmessage(opts)
            if cmdline_message:
                # pickup the cmdline msg
                message = cmdline_message
            elif message:
                # pickup the patch msg
                message = message.strip()
            else:
                # launch the editor
                message = None
            ui.debug(_('message:\n%s\n') % message)

            wp = repo.workingctx().parents()
            if opts.get('exact'):
                if not nodeid or not p1:
                    raise util.Abort(_('not a mercurial patch'))
                p1 = repo.lookup(p1)
                p2 = repo.lookup(p2 or hex(nullid))

                if p1 != wp[0].node():
                    hg.clean(repo, p1, wlock=wlock)
                repo.dirstate.setparents(p1, p2)
            elif p2:
                try:
                    p1 = repo.lookup(p1)
                    p2 = repo.lookup(p2)
                    if p1 == wp[0].node():
                        repo.dirstate.setparents(p1, p2)
                except hg.RepoError:
                    pass
            if opts.get('exact') or opts.get('import_branch'):
                repo.dirstate.setbranch(branch or 'default')

            files = {}
            try:
                fuzz = patch.patch(tmpname, ui, strip=strip, cwd=repo.root,
                                   files=files)
            finally:
                files = patch.updatedir(ui, repo, files, wlock=wlock)
            n = repo.commit(files, message, user, date, wlock=wlock, lock=lock)
            if opts.get('exact'):
                if hex(n) != nodeid:
                    repo.rollback(wlock=wlock, lock=lock)
                    raise util.Abort(_('patch is damaged or loses information'))
        finally:
            os.unlink(tmpname)

def incoming(ui, repo, source="default", **opts):
    """show new changesets found in source

    Show new changesets found in the specified path/URL or the default
    pull location. These are the changesets that would be pulled if a pull
    was requested.

    For remote repository, using --bundle avoids downloading the changesets
    twice if the incoming is followed by a pull.

    See pull for valid source format details.
    """
    source, revs = cmdutil.parseurl(ui.expandpath(source), opts['rev'])
    cmdutil.setremoteconfig(ui, opts)

    other = hg.repository(ui, source)
    ui.status(_('comparing with %s\n') % source)
    if revs:
        if 'lookup' in other.capabilities:
            revs = [other.lookup(rev) for rev in revs]
        else:
            error = _("Other repository doesn't support revision lookup, so a rev cannot be specified.")
            raise util.Abort(error)
    incoming = repo.findincoming(other, heads=revs, force=opts["force"])
    if not incoming:
        try:
            os.unlink(opts["bundle"])
        except:
            pass
        ui.status(_("no changes found\n"))
        return 1

    cleanup = None
    try:
        fname = opts["bundle"]
        if fname or not other.local():
            # create a bundle (uncompressed if other repo is not local)
            if revs is None:
                cg = other.changegroup(incoming, "incoming")
            else:
                if 'changegroupsubset' not in other.capabilities:
                    raise util.Abort(_("Partial incoming cannot be done because other repository doesn't support changegroupsubset."))
                cg = other.changegroupsubset(incoming, revs, 'incoming')
            bundletype = other.local() and "HG10BZ" or "HG10UN"
            fname = cleanup = changegroup.writebundle(cg, fname, bundletype)
            # keep written bundle?
            if opts["bundle"]:
                cleanup = None
            if not other.local():
                # use the created uncompressed bundlerepo
                other = bundlerepo.bundlerepository(ui, repo.root, fname)

        o = other.changelog.nodesbetween(incoming, revs)[0]
        if opts['newest_first']:
            o.reverse()
        displayer = cmdutil.show_changeset(ui, other, opts)
        for n in o:
            parents = [p for p in other.changelog.parents(n) if p != nullid]
            if opts['no_merges'] and len(parents) == 2:
                continue
            displayer.show(changenode=n)
    finally:
        if hasattr(other, 'close'):
            other.close()
        if cleanup:
            os.unlink(cleanup)

def init(ui, dest=".", **opts):
    """create a new repository in the given directory

    Initialize a new repository in the given directory.  If the given
    directory does not exist, it is created.

    If no directory is given, the current directory is used.

    It is possible to specify an ssh:// URL as the destination.
    Look at the help text for the pull command for important details
    about ssh:// URLs.
    """
    cmdutil.setremoteconfig(ui, opts)
    hg.repository(ui, dest, create=1)

def locate(ui, repo, *pats, **opts):
    """locate files matching specific patterns

    Print all files under Mercurial control whose names match the
    given patterns.

    This command searches the entire repository by default.  To search
    just the current directory and its subdirectories, use
    "--include .".

    If no patterns are given to match, this command prints all file
    names.

    If you want to feed the output of this command into the "xargs"
    command, use the "-0" option to both this command and "xargs".
    This will avoid the problem of "xargs" treating single filenames
    that contain white space as multiple filenames.
    """
    end = opts['print0'] and '\0' or '\n'
    rev = opts['rev']
    if rev:
        node = repo.lookup(rev)
    else:
        node = None

    ret = 1
    for src, abs, rel, exact in cmdutil.walk(repo, pats, opts, node=node,
                                             badmatch=util.always,
                                             default='relglob'):
        if src == 'b':
            continue
        if not node and repo.dirstate.state(abs) == '?':
            continue
        if opts['fullpath']:
            ui.write(os.path.join(repo.root, abs), end)
        else:
            ui.write(((pats and rel) or abs), end)
        ret = 0

    return ret

def log(ui, repo, *pats, **opts):
    """show revision history of entire repository or files

    Print the revision history of the specified files or the entire
    project.

    File history is shown without following rename or copy history of
    files.  Use -f/--follow with a file name to follow history across
    renames and copies. --follow without a file name will only show
    ancestors or descendants of the starting revision. --follow-first
    only follows the first parent of merge revisions.

    If no revision range is specified, the default is tip:0 unless
    --follow is set, in which case the working directory parent is
    used as the starting revision.

    By default this command outputs: changeset id and hash, tags,
    non-trivial parents, user, date and time, and a summary for each
    commit. When the -v/--verbose switch is used, the list of changed
    files and full commit message is shown.

    NOTE: log -p may generate unexpected diff output for merge
    changesets, as it will compare the merge changeset against its
    first parent only. Also, the files: list will only reflect files
    that are different from BOTH parents.

    """

    get = util.cachefunc(lambda r: repo.changectx(r).changeset())
    changeiter, matchfn = cmdutil.walkchangerevs(ui, repo, pats, get, opts)

    if opts['limit']:
        try:
            limit = int(opts['limit'])
        except ValueError:
            raise util.Abort(_('limit must be a positive integer'))
        if limit <= 0: raise util.Abort(_('limit must be positive'))
    else:
        limit = sys.maxint
    count = 0

    if opts['copies'] and opts['rev']:
        endrev = max(cmdutil.revrange(repo, opts['rev'])) + 1
    else:
        endrev = repo.changelog.count()
    rcache = {}
    ncache = {}
    dcache = []
    def getrenamed(fn, rev, man):
        '''looks up all renames for a file (up to endrev) the first
        time the file is given. It indexes on the changerev and only
        parses the manifest if linkrev != changerev.
        Returns rename info for fn at changerev rev.'''
        if fn not in rcache:
            rcache[fn] = {}
            ncache[fn] = {}
            fl = repo.file(fn)
            for i in xrange(fl.count()):
                node = fl.node(i)
                lr = fl.linkrev(node)
                renamed = fl.renamed(node)
                rcache[fn][lr] = renamed
                if renamed:
                    ncache[fn][node] = renamed
                if lr >= endrev:
                    break
        if rev in rcache[fn]:
            return rcache[fn][rev]
        mr = repo.manifest.rev(man)
        if repo.manifest.parentrevs(mr) != (mr - 1, nullrev):
            return ncache[fn].get(repo.manifest.find(man, fn)[0])
        if not dcache or dcache[0] != man:
            dcache[:] = [man, repo.manifest.readdelta(man)]
        if fn in dcache[1]:
            return ncache[fn].get(dcache[1][fn])
        return None

    df = False
    if opts["date"]:
        df = util.matchdate(opts["date"])

    displayer = cmdutil.show_changeset(ui, repo, opts, True, matchfn)
    for st, rev, fns in changeiter:
        if st == 'add':
            changenode = repo.changelog.node(rev)
            parents = [p for p in repo.changelog.parentrevs(rev)
                       if p != nullrev]
            if opts['no_merges'] and len(parents) == 2:
                continue
            if opts['only_merges'] and len(parents) != 2:
                continue

            if df:
                changes = get(rev)
                if not df(changes[2][0]):
                    continue

            if opts['keyword']:
                changes = get(rev)
                miss = 0
                for k in [kw.lower() for kw in opts['keyword']]:
                    if not (k in changes[1].lower() or
                            k in changes[4].lower() or
                            k in " ".join(changes[3]).lower()):
                        miss = 1
                        break
                if miss:
                    continue

            copies = []
            if opts.get('copies') and rev:
                mf = get(rev)[0]
                for fn in get(rev)[3]:
                    rename = getrenamed(fn, rev, mf)
                    if rename:
                        copies.append((fn, rename[0]))
            displayer.show(rev, changenode, copies=copies)
        elif st == 'iter':
            if count == limit: break
            if displayer.flush(rev):
                count += 1

def manifest(ui, repo, rev=None):
    """output the current or given revision of the project manifest

    Print a list of version controlled files for the given revision.
    If no revision is given, the parent of the working directory is used,
    or tip if no revision is checked out.

    The manifest is the list of files being version controlled. If no revision
    is given then the first parent of the working directory is used.

    With -v flag, print file permissions. With --debug flag, print
    file revision hashes.
    """

    m = repo.changectx(rev).manifest()
    files = m.keys()
    files.sort()

    for f in files:
        if ui.debugflag:
            ui.write("%40s " % hex(m[f]))
        if ui.verbose:
            ui.write("%3s " % (m.execf(f) and "755" or "644"))
        ui.write("%s\n" % f)

def merge(ui, repo, node=None, force=None, rev=None):
    """merge working directory with another revision

    Merge the contents of the current working directory and the
    requested revision. Files that changed between either parent are
    marked as changed for the next commit and a commit must be
    performed before any further updates are allowed.

    If no revision is specified, the working directory's parent is a
    head revision, and the repository contains exactly one other head,
    the other head is merged with by default.  Otherwise, an explicit
    revision to merge with must be provided.
    """

    if rev and node:
        raise util.Abort(_("please specify just one revision"))

    if not node:
        node = rev

    if not node:
        heads = repo.heads()
        if len(heads) > 2:
            raise util.Abort(_('repo has %d heads - '
                               'please merge with an explicit rev') %
                             len(heads))
        if len(heads) == 1:
            raise util.Abort(_('there is nothing to merge - '
                               'use "hg update" instead'))
        parent = repo.dirstate.parents()[0]
        if parent not in heads:
            raise util.Abort(_('working dir not at a head rev - '
                               'use "hg update" or merge with an explicit rev'))
        node = parent == heads[0] and heads[-1] or heads[0]
    return hg.merge(repo, node, force=force)

def outgoing(ui, repo, dest=None, **opts):
    """show changesets not found in destination

    Show changesets not found in the specified destination repository or
    the default push location. These are the changesets that would be pushed
    if a push was requested.

    See pull for valid destination format details.
    """
    dest, revs = cmdutil.parseurl(
        ui.expandpath(dest or 'default-push', dest or 'default'), opts['rev'])
    cmdutil.setremoteconfig(ui, opts)
    if revs:
        revs = [repo.lookup(rev) for rev in revs]

    other = hg.repository(ui, dest)
    ui.status(_('comparing with %s\n') % dest)
    o = repo.findoutgoing(other, force=opts['force'])
    if not o:
        ui.status(_("no changes found\n"))
        return 1
    o = repo.changelog.nodesbetween(o, revs)[0]
    if opts['newest_first']:
        o.reverse()
    displayer = cmdutil.show_changeset(ui, repo, opts)
    for n in o:
        parents = [p for p in repo.changelog.parents(n) if p != nullid]
        if opts['no_merges'] and len(parents) == 2:
            continue
        displayer.show(changenode=n)

def parents(ui, repo, file_=None, **opts):
    """show the parents of the working dir or revision

    Print the working directory's parent revisions. If a
    revision is given via --rev, the parent of that revision
    will be printed. If a file argument is given, revision in
    which the file was last changed (before the working directory
    revision or the argument to --rev if given) is printed.
    """
    rev = opts.get('rev')
    if file_:
        files, match, anypats = cmdutil.matchpats(repo, (file_,), opts)
        if anypats or len(files) != 1:
            raise util.Abort(_('can only specify an explicit file name'))
        ctx = repo.filectx(files[0], changeid=rev)
    elif rev:
        ctx = repo.changectx(rev)
    else:
        ctx = repo.workingctx()
    p = [cp.node() for cp in ctx.parents()]

    displayer = cmdutil.show_changeset(ui, repo, opts)
    for n in p:
        if n != nullid:
            displayer.show(changenode=n)

def paths(ui, repo, search=None):
    """show definition of symbolic path names

    Show definition of symbolic path name NAME. If no name is given, show
    definition of available names.

    Path names are defined in the [paths] section of /etc/mercurial/hgrc
    and $HOME/.hgrc.  If run inside a repository, .hg/hgrc is used, too.
    """
    if search:
        for name, path in ui.configitems("paths"):
            if name == search:
                ui.write("%s\n" % path)
                return
        ui.warn(_("not found!\n"))
        return 1
    else:
        for name, path in ui.configitems("paths"):
            ui.write("%s = %s\n" % (name, path))

def postincoming(ui, repo, modheads, optupdate):
    if modheads == 0:
        return
    if optupdate:
        if modheads == 1:
            return hg.update(repo, None)
        else:
            ui.status(_("not updating, since new heads added\n"))
    if modheads > 1:
        ui.status(_("(run 'hg heads' to see heads, 'hg merge' to merge)\n"))
    else:
        ui.status(_("(run 'hg update' to get a working copy)\n"))

def pull(ui, repo, source="default", **opts):
    """pull changes from the specified source

    Pull changes from a remote repository to a local one.

    This finds all changes from the repository at the specified path
    or URL and adds them to the local repository. By default, this
    does not update the copy of the project in the working directory.

    Valid URLs are of the form:

      local/filesystem/path (or file://local/filesystem/path)
      http://[user@]host[:port]/[path]
      https://[user@]host[:port]/[path]
      ssh://[user@]host[:port]/[path]
      static-http://host[:port]/[path]

    Paths in the local filesystem can either point to Mercurial
    repositories or to bundle files (as created by 'hg bundle' or
    'hg incoming --bundle'). The static-http:// protocol, albeit slow,
    allows access to a Mercurial repository where you simply use a web
    server to publish the .hg directory as static content.

    An optional identifier after # indicates a particular branch, tag,
    or changeset to pull.

    Some notes about using SSH with Mercurial:
    - SSH requires an accessible shell account on the destination machine
      and a copy of hg in the remote path or specified with as remotecmd.
    - path is relative to the remote user's home directory by default.
      Use an extra slash at the start of a path to specify an absolute path:
        ssh://example.com//tmp/repository
    - Mercurial doesn't use its own compression via SSH; the right thing
      to do is to configure it in your ~/.ssh/config, e.g.:
        Host *.mylocalnetwork.example.com
          Compression no
        Host *
          Compression yes
      Alternatively specify "ssh -C" as your ssh command in your hgrc or
      with the --ssh command line option.
    """
    source, revs = cmdutil.parseurl(ui.expandpath(source), opts['rev'])
    cmdutil.setremoteconfig(ui, opts)

    other = hg.repository(ui, source)
    ui.status(_('pulling from %s\n') % (source))
    if revs:
        if 'lookup' in other.capabilities:
            revs = [other.lookup(rev) for rev in revs]
        else:
            error = _("Other repository doesn't support revision lookup, so a rev cannot be specified.")
            raise util.Abort(error)

    modheads = repo.pull(other, heads=revs, force=opts['force'])
    return postincoming(ui, repo, modheads, opts['update'])

def push(ui, repo, dest=None, **opts):
    """push changes to the specified destination

    Push changes from the local repository to the given destination.

    This is the symmetrical operation for pull. It helps to move
    changes from the current repository to a different one. If the
    destination is local this is identical to a pull in that directory
    from the current one.

    By default, push will refuse to run if it detects the result would
    increase the number of remote heads. This generally indicates the
    the client has forgotten to sync and merge before pushing.

    Valid URLs are of the form:

      local/filesystem/path (or file://local/filesystem/path)
      ssh://[user@]host[:port]/[path]
      http://[user@]host[:port]/[path]
      https://[user@]host[:port]/[path]

    An optional identifier after # indicates a particular branch, tag,
    or changeset to push.

    Look at the help text for the pull command for important details
    about ssh:// URLs.

    Pushing to http:// and https:// URLs is only possible, if this
    feature is explicitly enabled on the remote Mercurial server.
    """
    dest, revs = cmdutil.parseurl(
        ui.expandpath(dest or 'default-push', dest or 'default'), opts['rev'])
    cmdutil.setremoteconfig(ui, opts)

    other = hg.repository(ui, dest)
    ui.status('pushing to %s\n' % (dest))
    if revs:
        revs = [repo.lookup(rev) for rev in revs]
    r = repo.push(other, opts['force'], revs=revs)
    return r == 0

def rawcommit(ui, repo, *pats, **opts):
    """raw commit interface (DEPRECATED)

    (DEPRECATED)
    Lowlevel commit, for use in helper scripts.

    This command is not intended to be used by normal users, as it is
    primarily useful for importing from other SCMs.

    This command is now deprecated and will be removed in a future
    release, please use debugsetparents and commit instead.
    """

    ui.warn(_("(the rawcommit command is deprecated)\n"))

    message = cmdutil.logmessage(opts)

    files, match, anypats = cmdutil.matchpats(repo, pats, opts)
    if opts['files']:
        files += open(opts['files']).read().splitlines()

    parents = [repo.lookup(p) for p in opts['parent']]

    try:
        repo.rawcommit(files, message, opts['user'], opts['date'], *parents)
    except ValueError, inst:
        raise util.Abort(str(inst))

def recover(ui, repo):
    """roll back an interrupted transaction

    Recover from an interrupted commit or pull.

    This command tries to fix the repository status after an interrupted
    operation. It should only be necessary when Mercurial suggests it.
    """
    if repo.recover():
        return hg.verify(repo)
    return 1

def remove(ui, repo, *pats, **opts):
    """remove the specified files on the next commit

    Schedule the indicated files for removal from the repository.

    This only removes files from the current branch, not from the
    entire project history.  If the files still exist in the working
    directory, they will be deleted from it.  If invoked with --after,
    files are marked as removed, but not actually unlinked unless --force
    is also given. Without exact file names, --after will only mark
    files as removed if they are no longer in the working directory.

    This command schedules the files to be removed at the next commit.
    To undo a remove before that, see hg revert.

    Modified files and added files are not removed by default.  To
    remove them, use the -f/--force option.
    """
    names = []
    if not opts['after'] and not pats:
        raise util.Abort(_('no files specified'))
    files, matchfn, anypats = cmdutil.matchpats(repo, pats, opts)
    exact = dict.fromkeys(files)
    mardu = map(dict.fromkeys, repo.status(files=files, match=matchfn))[:5]
    modified, added, removed, deleted, unknown = mardu
    remove, forget = [], []
    for src, abs, rel, exact in cmdutil.walk(repo, pats, opts):
        reason = None
        if abs in modified and not opts['force']:
            reason = _('is modified (use -f to force removal)')
        elif abs in added:
            if opts['force']:
                forget.append(abs)
                continue
            reason = _('has been marked for add (use -f to force removal)')
        elif repo.dirstate.state(abs) == '?':
            reason = _('is not managed')
        elif opts['after'] and not exact and abs not in deleted:
            continue
        elif abs in removed:
            continue
        if reason:
            if exact:
                ui.warn(_('not removing %s: file %s\n') % (rel, reason))
        else:
            if ui.verbose or not exact:
                ui.status(_('removing %s\n') % rel)
            remove.append(abs)
    repo.forget(forget)
    repo.remove(remove, unlink=opts['force'] or not opts['after'])

def rename(ui, repo, *pats, **opts):
    """rename files; equivalent of copy + remove

    Mark dest as copies of sources; mark sources for deletion.  If
    dest is a directory, copies are put in that directory.  If dest is
    a file, there can only be one source.

    By default, this command copies the contents of files as they
    stand in the working directory.  If invoked with --after, the
    operation is recorded, but no copying is performed.

    This command takes effect in the next commit. To undo a rename
    before that, see hg revert.
    """
    wlock = repo.wlock(0)
    errs, copied = docopy(ui, repo, pats, opts, wlock)
    names = []
    for abs, rel, exact in copied:
        if ui.verbose or not exact:
            ui.status(_('removing %s\n') % rel)
        names.append(abs)
    if not opts.get('dry_run'):
        repo.remove(names, True, wlock=wlock)
    return errs

def revert(ui, repo, *pats, **opts):
    """revert files or dirs to their states as of some revision

    With no revision specified, revert the named files or directories
    to the contents they had in the parent of the working directory.
    This restores the contents of the affected files to an unmodified
    state and unschedules adds, removes, copies, and renames. If the
    working directory has two parents, you must explicitly specify the
    revision to revert to.

    Modified files are saved with a .orig suffix before reverting.
    To disable these backups, use --no-backup.

    Using the -r option, revert the given files or directories to their
    contents as of a specific revision. This can be helpful to "roll
    back" some or all of a change that should not have been committed.

    Revert modifies the working directory.  It does not commit any
    changes, or change the parent of the working directory.  If you
    revert to a revision other than the parent of the working
    directory, the reverted files will thus appear modified
    afterwards.

    If a file has been deleted, it is restored.  If the executable
    mode of a file was changed, it is reset.

    If names are given, all files matching the names are reverted.

    If no arguments are given, no files are reverted.
    """

    if opts["date"]:
        if opts["rev"]:
            raise util.Abort(_("you can't specify a revision and a date"))
        opts["rev"] = cmdutil.finddate(ui, repo, opts["date"])

    if not pats and not opts['all']:
        raise util.Abort(_('no files or directories specified; '
                           'use --all to revert the whole repo'))

    parent, p2 = repo.dirstate.parents()
    if not opts['rev'] and p2 != nullid:
        raise util.Abort(_('uncommitted merge - please provide a '
                           'specific revision'))
    ctx = repo.changectx(opts['rev'])
    node = ctx.node()
    mf = ctx.manifest()
    if node == parent:
        pmf = mf
    else:
        pmf = None

    wlock = repo.wlock()

    # need all matching names in dirstate and manifest of target rev,
    # so have to walk both. do not print errors if files exist in one
    # but not other.

    names = {}
    target_only = {}

    # walk dirstate.

    for src, abs, rel, exact in cmdutil.walk(repo, pats, opts,
                                             badmatch=mf.has_key):
        names[abs] = (rel, exact)
        if src == 'b':
            target_only[abs] = True

    # walk target manifest.

    def badmatch(path):
        if path in names:
            return True
        path_ = path + '/'
        for f in names:
            if f.startswith(path_):
                return True
        return False

    for src, abs, rel, exact in cmdutil.walk(repo, pats, opts, node=node,
                                             badmatch=badmatch):
        if abs in names or src == 'b':
            continue
        names[abs] = (rel, exact)
        target_only[abs] = True

    changes = repo.status(match=names.has_key, wlock=wlock)[:5]
    modified, added, removed, deleted, unknown = map(dict.fromkeys, changes)

    revert = ([], _('reverting %s\n'))
    add = ([], _('adding %s\n'))
    remove = ([], _('removing %s\n'))
    forget = ([], _('forgetting %s\n'))
    undelete = ([], _('undeleting %s\n'))
    update = {}

    disptable = (
        # dispatch table:
        #   file state
        #   action if in target manifest
        #   action if not in target manifest
        #   make backup if in target manifest
        #   make backup if not in target manifest
        (modified, revert, remove, True, True),
        (added, revert, forget, True, False),
        (removed, undelete, None, False, False),
        (deleted, revert, remove, False, False),
        (unknown, add, None, True, False),
        (target_only, add, None, False, False),
        )

    entries = names.items()
    entries.sort()

    for abs, (rel, exact) in entries:
        mfentry = mf.get(abs)
        target = repo.wjoin(abs)
        def handle(xlist, dobackup):
            xlist[0].append(abs)
            update[abs] = 1
            if dobackup and not opts['no_backup'] and util.lexists(target):
                bakname = "%s.orig" % rel
                ui.note(_('saving current version of %s as %s\n') %
                        (rel, bakname))
                if not opts.get('dry_run'):
                    util.copyfile(target, bakname)
            if ui.verbose or not exact:
                ui.status(xlist[1] % rel)
        for table, hitlist, misslist, backuphit, backupmiss in disptable:
            if abs not in table: continue
            # file has changed in dirstate
            if mfentry:
                handle(hitlist, backuphit)
            elif misslist is not None:
                handle(misslist, backupmiss)
            else:
                if exact: ui.warn(_('file not managed: %s\n') % rel)
            break
        else:
            # file has not changed in dirstate
            if node == parent:
                if exact: ui.warn(_('no changes needed to %s\n') % rel)
                continue
            if pmf is None:
                # only need parent manifest in this unlikely case,
                # so do not read by default
                pmf = repo.changectx(parent).manifest()
            if abs in pmf:
                if mfentry:
                    # if version of file is same in parent and target
                    # manifests, do nothing
                    if pmf[abs] != mfentry:
                        handle(revert, False)
                else:
                    handle(remove, False)

    if not opts.get('dry_run'):
        repo.dirstate.forget(forget[0])
        r = hg.revert(repo, node, update.has_key, wlock)
        repo.dirstate.update(add[0], 'a')
        repo.dirstate.update(undelete[0], 'n')
        repo.dirstate.update(remove[0], 'r')
        return r

def rollback(ui, repo):
    """roll back the last transaction in this repository

    Roll back the last transaction in this repository, restoring the
    project to its state prior to the transaction.

    Transactions are used to encapsulate the effects of all commands
    that create new changesets or propagate existing changesets into a
    repository. For example, the following commands are transactional,
    and their effects can be rolled back:

      commit
      import
      pull
      push (with this repository as destination)
      unbundle

    This command should be used with care. There is only one level of
    rollback, and there is no way to undo a rollback. It will also
    restore the dirstate at the time of the last transaction, which
    may lose subsequent dirstate changes.

    This command is not intended for use on public repositories. Once
    changes are visible for pull by other users, rolling a transaction
    back locally is ineffective (someone else may already have pulled
    the changes). Furthermore, a race is possible with readers of the
    repository; for example an in-progress pull from the repository
    may fail if a rollback is performed.
    """
    repo.rollback()

def root(ui, repo):
    """print the root (top) of the current working dir

    Print the root directory of the current repository.
    """
    ui.write(repo.root + "\n")

def serve(ui, repo, **opts):
    """export the repository via HTTP

    Start a local HTTP repository browser and pull server.

    By default, the server logs accesses to stdout and errors to
    stderr.  Use the "-A" and "-E" options to log to files.
    """

    if opts["stdio"]:
        if repo is None:
            raise hg.RepoError(_("There is no Mercurial repository here"
                                 " (.hg not found)"))
        s = sshserver.sshserver(ui, repo)
        s.serve_forever()

    parentui = ui.parentui or ui
    optlist = ("name templates style address port ipv6"
               " accesslog errorlog webdir_conf")
    for o in optlist.split():
        if opts[o]:
            parentui.setconfig("web", o, str(opts[o]))
            if repo.ui != parentui:
                repo.ui.setconfig("web", o, str(opts[o]))

    if repo is None and not ui.config("web", "webdir_conf"):
        raise hg.RepoError(_("There is no Mercurial repository here"
                             " (.hg not found)"))

    class service:
        def init(self):
            util.set_signal_handler()
            try:
                self.httpd = hgweb.server.create_server(parentui, repo)
            except socket.error, inst:
                raise util.Abort(_('cannot start server: ') + inst.args[1])

            if not ui.verbose: return

            if self.httpd.port != 80:
                ui.status(_('listening at http://%s:%d/\n') %
                          (self.httpd.addr, self.httpd.port))
            else:
                ui.status(_('listening at http://%s/\n') % self.httpd.addr)

        def run(self):
            self.httpd.serve_forever()

    service = service()

    cmdutil.service(opts, initfn=service.init, runfn=service.run)

def status(ui, repo, *pats, **opts):
    """show changed files in the working directory

    Show status of files in the repository.  If names are given, only
    files that match are shown.  Files that are clean or ignored, are
    not listed unless -c (clean), -i (ignored) or -A is given.

    NOTE: status may appear to disagree with diff if permissions have
    changed or a merge has occurred. The standard diff format does not
    report permission changes and diff only reports changes relative
    to one merge parent.

    If one revision is given, it is used as the base revision.
    If two revisions are given, the difference between them is shown.

    The codes used to show the status of files are:
    M = modified
    A = added
    R = removed
    C = clean
    ! = deleted, but still tracked
    ? = not tracked
    I = ignored (not shown by default)
      = the previous added file was copied from here
    """

    all = opts['all']
    node1, node2 = cmdutil.revpair(repo, opts.get('rev'))

    files, matchfn, anypats = cmdutil.matchpats(repo, pats, opts)
    cwd = (pats and repo.getcwd()) or ''
    modified, added, removed, deleted, unknown, ignored, clean = [
        n for n in repo.status(node1=node1, node2=node2, files=files,
                             match=matchfn,
                             list_ignored=all or opts['ignored'],
                             list_clean=all or opts['clean'])]

    changetypes = (('modified', 'M', modified),
                   ('added', 'A', added),
                   ('removed', 'R', removed),
                   ('deleted', '!', deleted),
                   ('unknown', '?', unknown),
                   ('ignored', 'I', ignored))

    explicit_changetypes = changetypes + (('clean', 'C', clean),)

    end = opts['print0'] and '\0' or '\n'

    for opt, char, changes in ([ct for ct in explicit_changetypes
                                if all or opts[ct[0]]]
                               or changetypes):
        if opts['no_status']:
            format = "%%s%s" % end
        else:
            format = "%s %%s%s" % (char, end)

        for f in changes:
            ui.write(format % repo.pathto(f, cwd))
            if ((all or opts.get('copies')) and not opts.get('no_status')):
                copied = repo.dirstate.copied(f)
                if copied:
                    ui.write('  %s%s' % (repo.pathto(copied, cwd), end))

def tag(ui, repo, name, rev_=None, **opts):
    """add a tag for the current or given revision

    Name a particular revision using <name>.

    Tags are used to name particular revisions of the repository and are
    very useful to compare different revision, to go back to significant
    earlier versions or to mark branch points as releases, etc.

    If no revision is given, the parent of the working directory is used,
    or tip if no revision is checked out.

    To facilitate version control, distribution, and merging of tags,
    they are stored as a file named ".hgtags" which is managed
    similarly to other project files and can be hand-edited if
    necessary.  The file '.hg/localtags' is used for local tags (not
    shared among repositories).
    """
    if name in ['tip', '.', 'null']:
        raise util.Abort(_("the name '%s' is reserved") % name)
    if rev_ is not None:
        ui.warn(_("use of 'hg tag NAME [REV]' is deprecated, "
                  "please use 'hg tag [-r REV] NAME' instead\n"))
        if opts['rev']:
            raise util.Abort(_("use only one form to specify the revision"))
    if opts['rev'] and opts['remove']:
        raise util.Abort(_("--rev and --remove are incompatible"))
    if opts['rev']:
        rev_ = opts['rev']
    message = opts['message']
    if opts['remove']:
        if not name in repo.tags():
            raise util.Abort(_('tag %s does not exist') % name)
        rev_ = nullid
        if not message:
            message = _('Removed tag %s') % name
    elif name in repo.tags() and not opts['force']:
        raise util.Abort(_('a tag named %s already exists (use -f to force)')
                         % name)
    if not rev_ and repo.dirstate.parents()[1] != nullid:
        raise util.Abort(_('uncommitted merge - please provide a '
                           'specific revision'))
    r = repo.changectx(rev_).node()

    if not message:
        message = _('Added tag %s for changeset %s') % (name, short(r))

    repo.tag(name, r, message, opts['local'], opts['user'], opts['date'])

def tags(ui, repo):
    """list repository tags

    List the repository tags.

    This lists both regular and local tags.
    """

    l = repo.tagslist()
    l.reverse()
    hexfunc = ui.debugflag and hex or short
    for t, n in l:
        try:
            hn = hexfunc(n)
            r = "%5d:%s" % (repo.changelog.rev(n), hexfunc(n))
        except revlog.LookupError:
            r = "    ?:%s" % hn
        if ui.quiet:
            ui.write("%s\n" % t)
        else:
            spaces = " " * (30 - util.locallen(t))
            ui.write("%s%s %s\n" % (t, spaces, r))

def tip(ui, repo, **opts):
    """show the tip revision

    Show the tip revision.
    """
    cmdutil.show_changeset(ui, repo, opts).show(nullrev+repo.changelog.count())

def unbundle(ui, repo, fname1, *fnames, **opts):
    """apply one or more changegroup files

    Apply one or more compressed changegroup files generated by the
    bundle command.
    """
    fnames = (fname1,) + fnames
    result = None
    for fname in fnames:
        if os.path.exists(fname):
            f = open(fname, "rb")
        else:
            f = urllib.urlopen(fname)
        gen = changegroup.readbundle(f, fname)
        modheads = repo.addchangegroup(gen, 'unbundle', 'bundle:' + fname)

    return postincoming(ui, repo, modheads, opts['update'])

def update(ui, repo, node=None, rev=None, clean=False, date=None):
    """update working directory

    Update the working directory to the specified revision, or the
    tip of the current branch if none is specified.

    If there are no outstanding changes in the working directory and
    there is a linear relationship between the current version and the
    requested version, the result is the requested version.

    To merge the working directory with another revision, use the
    merge command.

    By default, update will refuse to run if doing so would require
    discarding local changes.
    """
    if rev and node:
        raise util.Abort(_("please specify just one revision"))

    if not rev:
        rev = node

    if date:
        if rev:
            raise util.Abort(_("you can't specify a revision and a date"))
        rev = cmdutil.finddate(ui, repo, date)

    if clean:
        return hg.clean(repo, rev)
    else:
        return hg.update(repo, rev)

def verify(ui, repo):
    """verify the integrity of the repository

    Verify the integrity of the current repository.

    This will perform an extensive check of the repository's
    integrity, validating the hashes and checksums of each entry in
    the changelog, manifest, and tracked files, as well as the
    integrity of their crosslinks and indices.
    """
    return hg.verify(repo)

def version_(ui):
    """output version and copyright information"""
    ui.write(_("Mercurial Distributed SCM (version %s)\n")
             % version.get_version())
    ui.status(_(
        "\nCopyright (C) 2005-2007 Matt Mackall <mpm@selenic.com> and others\n"
        "This is free software; see the source for copying conditions. "
        "There is NO\nwarranty; "
        "not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n"
    ))

# Command options and aliases are listed here, alphabetically

globalopts = [
    ('R', 'repository', '',
     _('repository root directory or symbolic path name')),
    ('', 'cwd', '', _('change working directory')),
    ('y', 'noninteractive', None,
     _('do not prompt, assume \'yes\' for any required answers')),
    ('q', 'quiet', None, _('suppress output')),
    ('v', 'verbose', None, _('enable additional output')),
    ('', 'config', [], _('set/override config option')),
    ('', 'debug', None, _('enable debugging output')),
    ('', 'debugger', None, _('start debugger')),
    ('', 'encoding', util._encoding, _('set the charset encoding')),
    ('', 'encodingmode', util._encodingmode, _('set the charset encoding mode')),
    ('', 'lsprof', None, _('print improved command execution profile')),
    ('', 'traceback', None, _('print traceback on exception')),
    ('', 'time', None, _('time how long the command takes')),
    ('', 'profile', None, _('print command execution profile')),
    ('', 'version', None, _('output version information and exit')),
    ('h', 'help', None, _('display help and exit')),
]

dryrunopts = [('n', 'dry-run', None,
               _('do not perform actions, just print output'))]

remoteopts = [
    ('e', 'ssh', '', _('specify ssh command to use')),
    ('', 'remotecmd', '', _('specify hg command to run on the remote side')),
]

walkopts = [
    ('I', 'include', [], _('include names matching the given patterns')),
    ('X', 'exclude', [], _('exclude names matching the given patterns')),
]

commitopts = [
    ('m', 'message', '', _('use <text> as commit message')),
    ('l', 'logfile', '', _('read commit message from <file>')),
]

table = {
    "^add": (add, walkopts + dryrunopts, _('hg add [OPTION]... [FILE]...')),
    "addremove":
        (addremove,
         [('s', 'similarity', '',
           _('guess renamed files by similarity (0<=s<=100)')),
         ] + walkopts + dryrunopts,
         _('hg addremove [OPTION]... [FILE]...')),
    "^annotate":
        (annotate,
         [('r', 'rev', '', _('annotate the specified revision')),
          ('f', 'follow', None, _('follow file copies and renames')),
          ('a', 'text', None, _('treat all files as text')),
          ('u', 'user', None, _('list the author')),
          ('d', 'date', None, _('list the date')),
          ('n', 'number', None, _('list the revision number (default)')),
          ('c', 'changeset', None, _('list the changeset')),
         ] + walkopts,
         _('hg annotate [-r REV] [-f] [-a] [-u] [-d] [-n] [-c] FILE...')),
    "archive":
        (archive,
         [('', 'no-decode', None, _('do not pass files through decoders')),
          ('p', 'prefix', '', _('directory prefix for files in archive')),
          ('r', 'rev', '', _('revision to distribute')),
          ('t', 'type', '', _('type of distribution to create')),
         ] + walkopts,
         _('hg archive [OPTION]... DEST')),
    "backout":
        (backout,
         [('', 'merge', None,
           _('merge with old dirstate parent after backout')),
          ('d', 'date', '', _('record datecode as commit date')),
          ('', 'parent', '', _('parent to choose when backing out merge')),
          ('u', 'user', '', _('record user as committer')),
          ('r', 'rev', '', _('revision to backout')),
         ] + walkopts + commitopts,
         _('hg backout [OPTION]... [-r] REV')),
    "branch":
        (branch,
         [('f', 'force', None,
           _('set branch name even if it shadows an existing branch'))],
         _('hg branch [NAME]')),
    "branches":
        (branches,
         [('a', 'active', False,
           _('show only branches that have unmerged heads'))],
         _('hg branches [-a]')),
    "bundle":
        (bundle,
         [('f', 'force', None,
           _('run even when remote repository is unrelated')),
          ('r', 'rev', [],
           _('a changeset you would like to bundle')),
          ('', 'base', [],
           _('a base changeset to specify instead of a destination')),
         ] + remoteopts,
         _('hg bundle [-f] [-r REV]... [--base REV]... FILE [DEST]')),
    "cat":
        (cat,
         [('o', 'output', '', _('print output to file with formatted name')),
          ('r', 'rev', '', _('print the given revision')),
         ] + walkopts,
         _('hg cat [OPTION]... FILE...')),
    "^clone":
        (clone,
         [('U', 'noupdate', None, _('do not update the new working directory')),
          ('r', 'rev', [],
           _('a changeset you would like to have after cloning')),
          ('', 'pull', None, _('use pull protocol to copy metadata')),
          ('', 'uncompressed', None,
           _('use uncompressed transfer (fast over LAN)')),
         ] + remoteopts,
         _('hg clone [OPTION]... SOURCE [DEST]')),
    "^commit|ci":
        (commit,
         [('A', 'addremove', None,
           _('mark new/missing files as added/removed before committing')),
          ('d', 'date', '', _('record datecode as commit date')),
          ('u', 'user', '', _('record user as commiter')),
         ] + walkopts + commitopts,
         _('hg commit [OPTION]... [FILE]...')),
    "copy|cp":
        (copy,
         [('A', 'after', None, _('record a copy that has already occurred')),
          ('f', 'force', None,
           _('forcibly copy over an existing managed file')),
         ] + walkopts + dryrunopts,
         _('hg copy [OPTION]... [SOURCE]... DEST')),
    "debugancestor": (debugancestor, [], _('debugancestor INDEX REV1 REV2')),
    "debugcomplete":
        (debugcomplete,
         [('o', 'options', None, _('show the command options'))],
         _('debugcomplete [-o] CMD')),
    "debuginstall": (debuginstall, [], _('debuginstall')),
    "debugrebuildstate":
        (debugrebuildstate,
         [('r', 'rev', '', _('revision to rebuild to'))],
         _('debugrebuildstate [-r REV] [REV]')),
    "debugcheckstate": (debugcheckstate, [], _('debugcheckstate')),
    "debugsetparents": (debugsetparents, [], _('debugsetparents REV1 [REV2]')),
    "debugstate": (debugstate, [], _('debugstate')),
    "debugdate":
        (debugdate,
         [('e', 'extended', None, _('try extended date formats'))],
         _('debugdate [-e] DATE [RANGE]')),
    "debugdata": (debugdata, [], _('debugdata FILE REV')),
    "debugindex": (debugindex, [], _('debugindex FILE')),
    "debugindexdot": (debugindexdot, [], _('debugindexdot FILE')),
    "debugrename":
        (debugrename,
         [('r', 'rev', '', _('revision to debug'))],
         _('debugrename [-r REV] FILE')),
    "debugwalk": (debugwalk, walkopts, _('debugwalk [OPTION]... [FILE]...')),
    "^diff":
        (diff,
         [('r', 'rev', [], _('revision')),
          ('a', 'text', None, _('treat all files as text')),
          ('p', 'show-function', None,
           _('show which function each change is in')),
          ('g', 'git', None, _('use git extended diff format')),
          ('', 'nodates', None, _("don't include dates in diff headers")),
          ('w', 'ignore-all-space', None,
           _('ignore white space when comparing lines')),
          ('b', 'ignore-space-change', None,
           _('ignore changes in the amount of white space')),
          ('B', 'ignore-blank-lines', None,
           _('ignore changes whose lines are all blank')),
         ] + walkopts,
         _('hg diff [OPTION]... [-r REV1 [-r REV2]] [FILE]...')),
    "^export":
        (export,
         [('o', 'output', '', _('print output to file with formatted name')),
          ('a', 'text', None, _('treat all files as text')),
          ('g', 'git', None, _('use git extended diff format')),
          ('', 'nodates', None, _("don't include dates in diff headers")),
          ('', 'switch-parent', None, _('diff against the second parent'))],
         _('hg export [OPTION]... [-o OUTFILESPEC] REV...')),
    "grep":
        (grep,
         [('0', 'print0', None, _('end fields with NUL')),
          ('', 'all', None, _('print all revisions that match')),
          ('f', 'follow', None,
           _('follow changeset history, or file history across copies and renames')),
          ('i', 'ignore-case', None, _('ignore case when matching')),
          ('l', 'files-with-matches', None,
           _('print only filenames and revs that match')),
          ('n', 'line-number', None, _('print matching line numbers')),
          ('r', 'rev', [], _('search in given revision range')),
          ('u', 'user', None, _('print user who committed change')),
         ] + walkopts,
         _('hg grep [OPTION]... PATTERN [FILE]...')),
    "heads":
        (heads,
         [('', 'style', '', _('display using template map file')),
          ('r', 'rev', '', _('show only heads which are descendants of rev')),
          ('', 'template', '', _('display with template'))],
         _('hg heads [-r REV] [REV]...')),
    "help": (help_, [], _('hg help [COMMAND]')),
    "identify|id":
        (identify,
         [('r', 'rev', '', _('identify the specified rev')),
          ('n', 'num', None, _('show local revision number')),
          ('i', 'id', None, _('show global revision id')),
          ('b', 'branch', None, _('show branch')),
          ('t', 'tags', None, _('show tags'))],
         _('hg identify [-nibt] [-r REV] [SOURCE]')),
    "import|patch":
        (import_,
         [('p', 'strip', 1,
           _('directory strip option for patch. This has the same\n'
             'meaning as the corresponding patch option')),
          ('b', 'base', '', _('base path')),
          ('f', 'force', None,
           _('skip check for outstanding uncommitted changes')),
          ('', 'exact', None,
           _('apply patch to the nodes from which it was generated')),
          ('', 'import-branch', None,
           _('Use any branch information in patch (implied by --exact)'))] + commitopts,
         _('hg import [-p NUM] [-m MESSAGE] [-f] PATCH...')),
    "incoming|in": (incoming,
         [('M', 'no-merges', None, _('do not show merges')),
          ('f', 'force', None,
           _('run even when remote repository is unrelated')),
          ('', 'style', '', _('display using template map file')),
          ('n', 'newest-first', None, _('show newest record first')),
          ('', 'bundle', '', _('file to store the bundles into')),
          ('p', 'patch', None, _('show patch')),
          ('r', 'rev', [], _('a specific revision up to which you would like to pull')),
          ('', 'template', '', _('display with template')),
         ] + remoteopts,
         _('hg incoming [-p] [-n] [-M] [-f] [-r REV]...'
           ' [--bundle FILENAME] [SOURCE]')),
    "^init":
        (init,
         remoteopts,
         _('hg init [-e CMD] [--remotecmd CMD] [DEST]')),
    "locate":
        (locate,
         [('r', 'rev', '', _('search the repository as it stood at rev')),
          ('0', 'print0', None,
           _('end filenames with NUL, for use with xargs')),
          ('f', 'fullpath', None,
           _('print complete paths from the filesystem root')),
         ] + walkopts,
         _('hg locate [OPTION]... [PATTERN]...')),
    "^log|history":
        (log,
         [('f', 'follow', None,
           _('follow changeset history, or file history across copies and renames')),
          ('', 'follow-first', None,
           _('only follow the first parent of merge changesets')),
          ('d', 'date', '', _('show revs matching date spec')),
          ('C', 'copies', None, _('show copied files')),
          ('k', 'keyword', [], _('do case-insensitive search for a keyword')),
          ('l', 'limit', '', _('limit number of changes displayed')),
          ('r', 'rev', [], _('show the specified revision or range')),
          ('', 'removed', None, _('include revs where files were removed')),
          ('M', 'no-merges', None, _('do not show merges')),
          ('', 'style', '', _('display using template map file')),
          ('m', 'only-merges', None, _('show only merges')),
          ('p', 'patch', None, _('show patch')),
          ('P', 'prune', [], _('do not display revision or any of its ancestors')),
          ('', 'template', '', _('display with template')),
         ] + walkopts,
         _('hg log [OPTION]... [FILE]')),
    "manifest": (manifest, [], _('hg manifest [REV]')),
    "^merge":
        (merge,
         [('f', 'force', None, _('force a merge with outstanding changes')),
          ('r', 'rev', '', _('revision to merge')),
             ],
         _('hg merge [-f] [[-r] REV]')),
    "outgoing|out": (outgoing,
         [('M', 'no-merges', None, _('do not show merges')),
          ('f', 'force', None,
           _('run even when remote repository is unrelated')),
          ('p', 'patch', None, _('show patch')),
          ('', 'style', '', _('display using template map file')),
          ('r', 'rev', [], _('a specific revision you would like to push')),
          ('n', 'newest-first', None, _('show newest record first')),
          ('', 'template', '', _('display with template')),
         ] + remoteopts,
         _('hg outgoing [-M] [-p] [-n] [-f] [-r REV]... [DEST]')),
    "^parents":
        (parents,
         [('r', 'rev', '', _('show parents from the specified rev')),
          ('', 'style', '', _('display using template map file')),
          ('', 'template', '', _('display with template'))],
         _('hg parents [-r REV] [FILE]')),
    "paths": (paths, [], _('hg paths [NAME]')),
    "^pull":
        (pull,
         [('u', 'update', None,
           _('update to new tip if changesets were pulled')),
          ('f', 'force', None,
           _('run even when remote repository is unrelated')),
          ('r', 'rev', [],
           _('a specific revision up to which you would like to pull')),
         ] + remoteopts,
         _('hg pull [-u] [-f] [-r REV]... [-e CMD] [--remotecmd CMD] [SOURCE]')),
    "^push":
        (push,
         [('f', 'force', None, _('force push')),
          ('r', 'rev', [], _('a specific revision you would like to push')),
         ] + remoteopts,
         _('hg push [-f] [-r REV]... [-e CMD] [--remotecmd CMD] [DEST]')),
    "debugrawcommit|rawcommit":
        (rawcommit,
         [('p', 'parent', [], _('parent')),
          ('d', 'date', '', _('date code')),
          ('u', 'user', '', _('user')),
          ('F', 'files', '', _('file list'))
          ] + commitopts,
         _('hg debugrawcommit [OPTION]... [FILE]...')),
    "recover": (recover, [], _('hg recover')),
    "^remove|rm":
        (remove,
         [('A', 'after', None, _('record remove that has already occurred')),
          ('f', 'force', None, _('remove file even if modified')),
         ] + walkopts,
         _('hg remove [OPTION]... FILE...')),
    "rename|mv":
        (rename,
         [('A', 'after', None, _('record a rename that has already occurred')),
          ('f', 'force', None,
           _('forcibly copy over an existing managed file')),
         ] + walkopts + dryrunopts,
         _('hg rename [OPTION]... SOURCE... DEST')),
    "^revert":
        (revert,
         [('a', 'all', None, _('revert all changes when no arguments given')),
          ('d', 'date', '', _('tipmost revision matching date')),
          ('r', 'rev', '', _('revision to revert to')),
          ('', 'no-backup', None, _('do not save backup copies of files')),
         ] + walkopts + dryrunopts,
         _('hg revert [OPTION]... [-r REV] [NAME]...')),
    "rollback": (rollback, [], _('hg rollback')),
    "root": (root, [], _('hg root')),
    "showconfig|debugconfig":
        (showconfig,
         [('u', 'untrusted', None, _('show untrusted configuration options'))],
         _('showconfig [-u] [NAME]...')),
    "^serve":
        (serve,
         [('A', 'accesslog', '', _('name of access log file to write to')),
          ('d', 'daemon', None, _('run server in background')),
          ('', 'daemon-pipefds', '', _('used internally by daemon mode')),
          ('E', 'errorlog', '', _('name of error log file to write to')),
          ('p', 'port', 0, _('port to use (default: 8000)')),
          ('a', 'address', '', _('address to use')),
          ('n', 'name', '',
           _('name to show in web pages (default: working dir)')),
          ('', 'webdir-conf', '', _('name of the webdir config file'
                                    ' (serve more than one repo)')),
          ('', 'pid-file', '', _('name of file to write process ID to')),
          ('', 'stdio', None, _('for remote clients')),
          ('t', 'templates', '', _('web templates to use')),
          ('', 'style', '', _('template style to use')),
          ('6', 'ipv6', None, _('use IPv6 in addition to IPv4'))],
         _('hg serve [OPTION]...')),
    "^status|st":
        (status,
         [('A', 'all', None, _('show status of all files')),
          ('m', 'modified', None, _('show only modified files')),
          ('a', 'added', None, _('show only added files')),
          ('r', 'removed', None, _('show only removed files')),
          ('d', 'deleted', None, _('show only deleted (but tracked) files')),
          ('c', 'clean', None, _('show only files without changes')),
          ('u', 'unknown', None, _('show only unknown (not tracked) files')),
          ('i', 'ignored', None, _('show only ignored files')),
          ('n', 'no-status', None, _('hide status prefix')),
          ('C', 'copies', None, _('show source of copied files')),
          ('0', 'print0', None,
           _('end filenames with NUL, for use with xargs')),
          ('', 'rev', [], _('show difference from revision')),
         ] + walkopts,
         _('hg status [OPTION]... [FILE]...')),
    "tag":
        (tag,
         [('f', 'force', None, _('replace existing tag')),
          ('l', 'local', None, _('make the tag local')),
          ('m', 'message', '', _('message for tag commit log entry')),
          ('d', 'date', '', _('record datecode as commit date')),
          ('u', 'user', '', _('record user as commiter')),
          ('r', 'rev', '', _('revision to tag')),
          ('', 'remove', None, _('remove a tag'))],
         _('hg tag [-l] [-m TEXT] [-d DATE] [-u USER] [-r REV] NAME')),
    "tags": (tags, [], _('hg tags')),
    "tip":
        (tip,
         [('', 'style', '', _('display using template map file')),
          ('p', 'patch', None, _('show patch')),
          ('', 'template', '', _('display with template'))],
         _('hg tip [-p]')),
    "unbundle":
        (unbundle,
         [('u', 'update', None,
           _('update to new tip if changesets were unbundled'))],
         _('hg unbundle [-u] FILE...')),
    "^update|up|checkout|co":
        (update,
         [('C', 'clean', None, _('overwrite locally modified files')),
          ('d', 'date', '', _('tipmost revision matching date')),
          ('r', 'rev', '', _('revision'))],
         _('hg update [-C] [-d DATE] [[-r] REV]')),
    "verify": (verify, [], _('hg verify')),
    "version": (version_, [], _('hg version')),
}

norepo = ("clone init version help debugancestor debugcomplete debugdata"
          " debugindex debugindexdot debugdate debuginstall")
optionalrepo = ("paths serve showconfig")

def dispatch(args, argv0=None):
    try:
        u = ui.ui(traceback='--traceback' in args)
    except util.Abort, inst:
        sys.stderr.write(_("abort: %s\n") % inst)
        return -1
    return cmdutil.runcatch(u, args, argv0=argv0)

def run():
    sys.exit(dispatch(sys.argv[1:], argv0=sys.argv[0]))
