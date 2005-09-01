# commands.py - command processing for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
from node import *
demandload(globals(), "os re sys signal shutil imp")
demandload(globals(), "fancyopts ui hg util lock revlog")
demandload(globals(), "fnmatch hgweb mdiff random signal time traceback")
demandload(globals(), "errno socket version struct atexit sets")

class UnknownCommand(Exception):
    """Exception raised if command is not in the command table."""

def filterfiles(filters, files):
    l = [x for x in files if x in filters]

    for t in filters:
        if t and t[-1] != "/":
            t += "/"
        l += [x for x in files if x.startswith(t)]
    return l

def relpath(repo, args):
    cwd = repo.getcwd()
    if cwd:
        return [util.normpath(os.path.join(cwd, x)) for x in args]
    return args

def matchpats(repo, cwd, pats=[], opts={}, head=''):
    return util.matcher(repo.root, cwd, pats or ['.'], opts.get('include'),
                        opts.get('exclude'), head)

def makewalk(repo, pats, opts, head=''):
    cwd = repo.getcwd()
    files, matchfn, anypats = matchpats(repo, cwd, pats, opts, head)
    exact = dict(zip(files, files))
    def walk():
        for src, fn in repo.walk(files=files, match=matchfn):
            yield src, fn, util.pathto(cwd, fn), fn in exact
    return files, matchfn, walk()

def walk(repo, pats, opts, head=''):
    files, matchfn, results = makewalk(repo, pats, opts, head)
    for r in results:
        yield r

def walkchangerevs(ui, repo, cwd, pats, opts):
    '''Iterate over files and the revs they changed in.

    Callers most commonly need to iterate backwards over the history
    it is interested in.  Doing so has awful (quadratic-looking)
    performance, so we use iterators in a "windowed" way.

    We walk a window of revisions in the desired order.  Within the
    window, we first walk forwards to gather data, then in the desired
    order (usually backwards) to display it.

    This function returns an (iterator, getchange) pair.  The
    getchange function returns the changelog entry for a numeric
    revision.  The iterator yields 3-tuples.  They will be of one of
    the following forms:

    "window", incrementing, lastrev: stepping through a window,
    positive if walking forwards through revs, last rev in the
    sequence iterated over - use to reset state for the current window

    "add", rev, fns: out-of-order traversal of the given file names
    fns, which changed during revision rev - use to gather data for
    possible display

    "iter", rev, None: in-order traversal of the revs earlier iterated
    over with "add" - use to display data'''
    cwd = repo.getcwd()
    if not pats and cwd:
        opts['include'] = [os.path.join(cwd, i) for i in opts['include']]
        opts['exclude'] = [os.path.join(cwd, x) for x in opts['exclude']]
    files, matchfn, anypats = matchpats(repo, (pats and cwd) or '',
                                        pats, opts)
    revs = map(int, revrange(ui, repo, opts['rev'] or ['tip:0']))
    wanted = {}
    slowpath = anypats
    window = 300
    fncache = {}

    chcache = {}
    def getchange(rev):
        ch = chcache.get(rev)
        if ch is None:
            chcache[rev] = ch = repo.changelog.read(repo.lookup(str(rev)))
        return ch

    if not slowpath and not files:
        # No files, no patterns.  Display all revs.
        wanted = dict(zip(revs, revs))
    if not slowpath:
        # Only files, no patterns.  Check the history of each file.
        def filerevgen(filelog):
            for i in xrange(filelog.count() - 1, -1, -window):
                revs = []
                for j in xrange(max(0, i - window), i + 1):
                    revs.append(filelog.linkrev(filelog.node(j)))
                revs.reverse()
                for rev in revs:
                    yield rev

        minrev, maxrev = min(revs), max(revs)
        for file in files:
            filelog = repo.file(file)
            # A zero count may be a directory or deleted file, so
            # try to find matching entries on the slow path.
            if filelog.count() == 0:
                slowpath = True
                break
            for rev in filerevgen(filelog):
                if rev <= maxrev:
                    if rev < minrev:
                        break
                    fncache.setdefault(rev, [])
                    fncache[rev].append(file)
                    wanted[rev] = 1
    if slowpath:
        # The slow path checks files modified in every changeset.
        def changerevgen():
            for i in xrange(repo.changelog.count() - 1, -1, -window):
                for j in xrange(max(0, i - window), i + 1):
                    yield j, getchange(j)[3]

        for rev, changefiles in changerevgen():
            matches = filter(matchfn, changefiles)
            if matches:
                fncache[rev] = matches
                wanted[rev] = 1

    def iterate():
        for i in xrange(0, len(revs), window):
            yield 'window', revs[0] < revs[-1], revs[-1]
            nrevs = [rev for rev in revs[i:min(i+window, len(revs))]
                     if rev in wanted]
            srevs = list(nrevs)
            srevs.sort()
            for rev in srevs:
                fns = fncache.get(rev) or filter(matchfn, getchange(rev)[3])
                yield 'add', rev, fns
            for rev in nrevs:
                yield 'iter', rev, None
    return iterate(), getchange

revrangesep = ':'

def revrange(ui, repo, revs, revlog=None):
    """Yield revision as strings from a list of revision specifications."""
    if revlog is None:
        revlog = repo.changelog
    revcount = revlog.count()
    def fix(val, defval):
        if not val:
            return defval
        try:
            num = int(val)
            if str(num) != val:
                raise ValueError
            if num < 0:
                num += revcount
            if not (0 <= num < revcount):
                raise ValueError
        except ValueError:
            try:
                num = repo.changelog.rev(repo.lookup(val))
            except KeyError:
                try:
                    num = revlog.rev(revlog.lookup(val))
                except KeyError:
                    raise util.Abort('invalid revision identifier %s', val)
        return num
    seen = {}
    for spec in revs:
        if spec.find(revrangesep) >= 0:
            start, end = spec.split(revrangesep, 1)
            start = fix(start, 0)
            end = fix(end, revcount - 1)
            step = start > end and -1 or 1
            for rev in xrange(start, end+step, step):
                if rev in seen: continue
                seen[rev] = 1
                yield str(rev)
        else:
            rev = fix(spec, None)
            if rev in seen: continue
            seen[rev] = 1
            yield str(rev)

def make_filename(repo, r, pat, node=None,
                  total=None, seqno=None, revwidth=None):
    node_expander = {
        'H': lambda: hex(node),
        'R': lambda: str(r.rev(node)),
        'h': lambda: short(node),
        }
    expander = {
        '%': lambda: '%',
        'b': lambda: os.path.basename(repo.root),
        }

    try:
        if node:
            expander.update(node_expander)
        if node and revwidth is not None:
            expander['r'] = lambda: str(r.rev(node)).zfill(revwidth)
        if total is not None:
            expander['N'] = lambda: str(total)
        if seqno is not None:
            expander['n'] = lambda: str(seqno)
        if total is not None and seqno is not None:
            expander['n'] = lambda:str(seqno).zfill(len(str(total)))

        newname = []
        patlen = len(pat)
        i = 0
        while i < patlen:
            c = pat[i]
            if c == '%':
                i += 1
                c = pat[i]
                c = expander[c]()
            newname.append(c)
            i += 1
        return ''.join(newname)
    except KeyError, inst:
        raise util.Abort("invalid format spec '%%%s' in output file name",
                    inst.args[0])

def make_file(repo, r, pat, node=None,
              total=None, seqno=None, revwidth=None, mode='wb'):
    if not pat or pat == '-':
        return 'w' in mode and sys.stdout or sys.stdin
    if hasattr(pat, 'write') and 'w' in mode:
        return pat
    if hasattr(pat, 'read') and 'r' in mode:
        return pat
    return open(make_filename(repo, r, pat, node, total, seqno, revwidth),
                mode)

def dodiff(fp, ui, repo, node1, node2, files=None, match=util.always,
           changes=None, text=False):
    def date(c):
        return time.asctime(time.gmtime(float(c[2].split(' ')[0])))

    if not changes:
        (c, a, d, u) = repo.changes(node1, node2, files, match=match)
    else:
        (c, a, d, u) = changes
    if files:
        c, a, d = map(lambda x: filterfiles(files, x), (c, a, d))

    if not c and not a and not d:
        return

    if node2:
        change = repo.changelog.read(node2)
        mmap2 = repo.manifest.read(change[0])
        date2 = date(change)
        def read(f):
            return repo.file(f).read(mmap2[f])
    else:
        date2 = time.asctime()
        if not node1:
            node1 = repo.dirstate.parents()[0]
        def read(f):
            return repo.wfile(f).read()

    if ui.quiet:
        r = None
    else:
        hexfunc = ui.verbose and hex or short
        r = [hexfunc(node) for node in [node1, node2] if node]

    change = repo.changelog.read(node1)
    mmap = repo.manifest.read(change[0])
    date1 = date(change)

    for f in c:
        to = None
        if f in mmap:
            to = repo.file(f).read(mmap[f])
        tn = read(f)
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r, text=text))
    for f in a:
        to = None
        tn = read(f)
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r, text=text))
    for f in d:
        to = repo.file(f).read(mmap[f])
        tn = None
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r, text=text))

def trimuser(ui, name, rev, revcache):
    """trim the name of the user who committed a change"""
    user = revcache.get(rev)
    if user is None:
        user = revcache[rev] = ui.shortuser(name)
    return user

def show_changeset(ui, repo, rev=0, changenode=None, brinfo=None):
    """show a single changeset or file revision"""
    log = repo.changelog
    if changenode is None:
        changenode = log.node(rev)
    elif not rev:
        rev = log.rev(changenode)

    if ui.quiet:
        ui.write("%d:%s\n" % (rev, short(changenode)))
        return

    changes = log.read(changenode)

    t, tz = changes[2].split(' ')
    # a conversion tool was sticking non-integer offsets into repos
    try:
        tz = int(tz)
    except ValueError:
        tz = 0
    date = time.asctime(time.localtime(float(t))) + " %+05d" % (int(tz)/-36)

    parents = [(log.rev(p), ui.verbose and hex(p) or short(p))
               for p in log.parents(changenode)
               if ui.debugflag or p != nullid]
    if not ui.debugflag and len(parents) == 1 and parents[0][0] == rev-1:
        parents = []

    if ui.verbose:
        ui.write("changeset:   %d:%s\n" % (rev, hex(changenode)))
    else:
        ui.write("changeset:   %d:%s\n" % (rev, short(changenode)))

    for tag in repo.nodetags(changenode):
        ui.status("tag:         %s\n" % tag)
    for parent in parents:
        ui.write("parent:      %d:%s\n" % parent)

    if brinfo and changenode in brinfo:
        br = brinfo[changenode]
        ui.write("branch:      %s\n" % " ".join(br))

    ui.debug("manifest:    %d:%s\n" % (repo.manifest.rev(changes[0]),
                                      hex(changes[0])))
    ui.status("user:        %s\n" % changes[1])
    ui.status("date:        %s\n" % date)

    if ui.debugflag:
        files = repo.changes(log.parents(changenode)[0], changenode)
        for key, value in zip(["files:", "files+:", "files-:"], files):
            if value:
                ui.note("%-12s %s\n" % (key, " ".join(value)))
    else:
        ui.note("files:       %s\n" % " ".join(changes[3]))

    description = changes[4].strip()
    if description:
        if ui.verbose:
            ui.status("description:\n")
            ui.status(description)
            ui.status("\n\n")
        else:
            ui.status("summary:     %s\n" % description.splitlines()[0])
    ui.status("\n")

def show_version(ui):
    """output version and copyright information"""
    ui.write("Mercurial Distributed SCM (version %s)\n"
             % version.get_version())
    ui.status(
        "\nCopyright (C) 2005 Matt Mackall <mpm@selenic.com>\n"
        "This is free software; see the source for copying conditions. "
        "There is NO\nwarranty; "
        "not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n"
    )

def help_(ui, cmd=None, with_version=False):
    """show help for a given command or all commands"""
    option_lists = []
    if cmd and cmd != 'shortlist':
        if with_version:
            show_version(ui)
            ui.write('\n')
        key, i = find(cmd)
        # synopsis
        ui.write("%s\n\n" % i[2])

        # description
        doc = i[0].__doc__
        if ui.quiet:
            doc = doc.splitlines(0)[0]
        ui.write("%s\n" % doc.rstrip())

        if not ui.quiet:
            # aliases
            aliases = ', '.join(key.split('|')[1:])
            if aliases:
                ui.write("\naliases: %s\n" % aliases)

            # options
            if i[1]:
                option_lists.append(("options", i[1]))

    else:
        # program name
        if ui.verbose or with_version:
            show_version(ui)
        else:
            ui.status("Mercurial Distributed SCM\n")
        ui.status('\n')

        # list of commands
        if cmd == "shortlist":
            ui.status('basic commands (use "hg help" '
                      'for the full list or option "-v" for details):\n\n')
        elif ui.verbose:
            ui.status('list of commands:\n\n')
        else:
            ui.status('list of commands (use "hg help -v" '
                      'to show aliases and global options):\n\n')

        h = {}
        cmds = {}
        for c, e in table.items():
            f = c.split("|")[0]
            if cmd == "shortlist" and not f.startswith("^"):
                continue
            f = f.lstrip("^")
            if not ui.debugflag and f.startswith("debug"):
                continue
            d = ""
            if e[0].__doc__:
                d = e[0].__doc__.splitlines(0)[0].rstrip()
            h[f] = d
            cmds[f]=c.lstrip("^")

        fns = h.keys()
        fns.sort()
        m = max(map(len, fns))
        for f in fns:
            if ui.verbose:
                commands = cmds[f].replace("|",", ")
                ui.write(" %s:\n      %s\n"%(commands,h[f]))
            else:
                ui.write(' %-*s   %s\n' % (m, f, h[f]))

    # global options
    if ui.verbose:
        option_lists.append(("global options", globalopts))

    # list all option lists
    opt_output = []
    for title, options in option_lists:
        opt_output.append(("\n%s:\n" % title, None))
        for shortopt, longopt, default, desc in options:
            opt_output.append(("%2s%s" % (shortopt and "-%s" % shortopt,
                                          longopt and " --%s" % longopt),
                               "%s%s" % (desc,
                                         default and " (default: %s)" % default
                                         or "")))

    if opt_output:
        opts_len = max([len(line[0]) for line in opt_output if line[1]])
        for first, second in opt_output:
            if second:
                ui.write(" %-*s  %s\n" % (opts_len, first, second))
            else:
                ui.write("%s\n" % first)

# Commands start here, listed alphabetically

def add(ui, repo, *pats, **opts):
    '''add the specified files on the next commit'''
    names = []
    for src, abs, rel, exact in walk(repo, pats, opts):
        if exact:
            names.append(abs)
        elif repo.dirstate.state(abs) == '?':
            ui.status('adding %s\n' % rel)
            names.append(abs)
    repo.add(names)

def addremove(ui, repo, *pats, **opts):
    """add all new files, delete all missing files"""
    add, remove = [], []
    for src, abs, rel, exact in walk(repo, pats, opts):
        if src == 'f' and repo.dirstate.state(abs) == '?':
            add.append(abs)
            if not exact:
                ui.status('adding ', rel, '\n')
        if repo.dirstate.state(abs) != 'r' and not os.path.exists(rel):
            remove.append(abs)
            if not exact:
                ui.status('removing ', rel, '\n')
    repo.add(add)
    repo.remove(remove)

def annotate(ui, repo, *pats, **opts):
    """show changeset information per file line"""
    def getnode(rev):
        return short(repo.changelog.node(rev))

    ucache = {}
    def getname(rev):
        cl = repo.changelog.read(repo.changelog.node(rev))
        return trimuser(ui, cl[1], rev, ucache)

    if not pats:
        raise util.Abort('at least one file name or pattern required')

    opmap = [['user', getname], ['number', str], ['changeset', getnode]]
    if not opts['user'] and not opts['changeset']:
        opts['number'] = 1

    if opts['rev']:
        node = repo.changelog.lookup(opts['rev'])
    else:
        node = repo.dirstate.parents()[0]
    change = repo.changelog.read(node)
    mmap = repo.manifest.read(change[0])

    for src, abs, rel, exact in walk(repo, pats, opts):
        if abs not in mmap:
            ui.warn("warning: %s is not in the repository!\n" % rel)
            continue

        f = repo.file(abs)
        if not opts['text'] and util.binary(f.read(mmap[abs])):
            ui.write("%s: binary file\n" % rel)
            continue

        lines = f.annotate(mmap[abs])
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

def cat(ui, repo, file1, rev=None, **opts):
    """output the latest or given revision of a file"""
    r = repo.file(relpath(repo, [file1])[0])
    if rev:
        try:
            # assume all revision numbers are for changesets
            n = repo.lookup(rev)
            change = repo.changelog.read(n)
            m = repo.manifest.read(change[0])
            n = m[relpath(repo, [file1])[0]]
        except hg.RepoError, KeyError:
            n = r.lookup(rev)
    else:
        n = r.tip()
    fp = make_file(repo, r, opts['output'], node=n)
    fp.write(r.read(n))

def clone(ui, source, dest=None, **opts):
    """make a copy of an existing repository"""
    if dest is None:
        dest = os.path.basename(os.path.normpath(source))

    if os.path.exists(dest):
        ui.warn("abort: destination '%s' already exists\n" % dest)
        return 1

    dest = os.path.realpath(dest)

    class Dircleanup:
        def __init__(self, dir_):
            self.rmtree = shutil.rmtree
            self.dir_ = dir_
            os.mkdir(dir_)
        def close(self):
            self.dir_ = None
        def __del__(self):
            if self.dir_:
                self.rmtree(self.dir_, True)

    if opts['ssh']:
        ui.setconfig("ui", "ssh", opts['ssh'])
    if opts['remotecmd']:
        ui.setconfig("ui", "remotecmd", opts['remotecmd'])

    d = Dircleanup(dest)
    source = ui.expandpath(source)
    abspath = source
    other = hg.repository(ui, source)

    if other.dev() != -1:
        abspath = os.path.abspath(source)
        copyfile = (os.stat(dest).st_dev == other.dev()
                    and getattr(os, 'link', None) or shutil.copy2)
        if copyfile is not shutil.copy2:
            ui.note("cloning by hardlink\n")
        # we use a lock here because because we're not nicely ordered
        l = lock.lock(os.path.join(source, ".hg", "lock"))

        util.copytree(os.path.join(source, ".hg"), os.path.join(dest, ".hg"),
                      copyfile)

        for fn in "dirstate", "lock":
            try:
                os.unlink(os.path.join(dest, ".hg", fn))
            except OSError:
                pass

        repo = hg.repository(ui, dest)

    else:
        repo = hg.repository(ui, dest, create=1)
        repo.pull(other)

    f = repo.opener("hgrc", "a")
    f.write("\n[paths]\n")
    f.write("default = %s\n" % abspath)

    if not opts['noupdate']:
        update(ui, repo)

    d.close()

def commit(ui, repo, *pats, **opts):
    """commit the specified files or all outstanding changes"""
    if opts['text']:
        ui.warn("Warning: -t and --text is deprecated,"
                " please use -m or --message instead.\n")
    message = opts['message'] or opts['text']
    logfile = opts['logfile']
    if not message and logfile:
        try:
            if logfile == '-':
                message = sys.stdin.read()
            else:
                message = open(logfile).read()
        except IOError, why:
            ui.warn("Can't read commit message %s: %s\n" % (logfile, why))

    if opts['addremove']:
        addremove(ui, repo, *pats, **opts)
    cwd = repo.getcwd()
    if not pats and cwd:
        opts['include'] = [os.path.join(cwd, i) for i in opts['include']]
        opts['exclude'] = [os.path.join(cwd, x) for x in opts['exclude']]
    fns, match, anypats = matchpats(repo, (pats and repo.getcwd()) or '',
                                    pats, opts)
    if pats:
        c, a, d, u = repo.changes(files=fns, match=match)
        files = c + a + [fn for fn in d if repo.dirstate.state(fn) == 'r']
    else:
        files = []
    repo.commit(files, message, opts['user'], opts['date'], match)

def copy(ui, repo, source, dest):
    """mark a file as copied or renamed for the next commit"""
    return repo.copy(*relpath(repo, (source, dest)))

def debugcheckstate(ui, repo):
    """validate the correctness of the current dirstate"""
    parent1, parent2 = repo.dirstate.parents()
    repo.dirstate.read()
    dc = repo.dirstate.map
    keys = dc.keys()
    keys.sort()
    m1n = repo.changelog.read(parent1)[0]
    m2n = repo.changelog.read(parent2)[0]
    m1 = repo.manifest.read(m1n)
    m2 = repo.manifest.read(m2n)
    errors = 0
    for f in dc:
        state = repo.dirstate.state(f)
        if state in "nr" and f not in m1:
            ui.warn("%s in state %s, but not in manifest1\n" % (f, state))
            errors += 1
        if state in "a" and f in m1:
            ui.warn("%s in state %s, but also in manifest1\n" % (f, state))
            errors += 1
        if state in "m" and f not in m1 and f not in m2:
            ui.warn("%s in state %s, but not in either manifest\n" %
                    (f, state))
            errors += 1
    for f in m1:
        state = repo.dirstate.state(f)
        if state not in "nrm":
            ui.warn("%s in manifest1, but listed as state %s" % (f, state))
            errors += 1
    if errors:
        raise util.Abort(".hg/dirstate inconsistent with current parent's manifest")

def debugconfig(ui):
    """show combined config settings from all hgrc files"""
    try:
        repo = hg.repository(ui)
    except hg.RepoError:
        pass
    for section, name, value in ui.walkconfig():
        ui.write('%s.%s=%s\n' % (section, name, value))

def debugstate(ui, repo):
    """show the contents of the current dirstate"""
    repo.dirstate.read()
    dc = repo.dirstate.map
    keys = dc.keys()
    keys.sort()
    for file_ in keys:
        ui.write("%c %3o %10d %s %s\n"
                 % (dc[file_][0], dc[file_][1] & 0777, dc[file_][2],
                    time.strftime("%x %X",
                                  time.localtime(dc[file_][3])), file_))
    for f in repo.dirstate.copies:
        ui.write("copy: %s -> %s\n" % (repo.dirstate.copies[f], f))

def debugdata(ui, file_, rev):
    """dump the contents of an data file revision"""
    r = revlog.revlog(file, file_[:-2] + ".i", file_)
    ui.write(r.revision(r.lookup(rev)))

def debugindex(ui, file_):
    """dump the contents of an index file"""
    r = revlog.revlog(file, file_, "")
    ui.write("   rev    offset  length   base linkrev" +
             " nodeid       p1           p2\n")
    for i in range(r.count()):
        e = r.index[i]
        ui.write("% 6d % 9d % 7d % 6d % 7d %s %s %s\n" % (
                i, e[0], e[1], e[2], e[3],
            short(e[6]), short(e[4]), short(e[5])))

def debugindexdot(ui, file_):
    """dump an index DAG as a .dot file"""
    r = revlog.revlog(file, file_, "")
    ui.write("digraph G {\n")
    for i in range(r.count()):
        e = r.index[i]
        ui.write("\t%d -> %d\n" % (r.rev(e[4]), i))
        if e[5] != nullid:
            ui.write("\t%d -> %d\n" % (r.rev(e[5]), i))
    ui.write("}\n")

def debugrename(ui, repo, file, rev=None):
    r = repo.file(relpath(repo, [file])[0])
    if rev:
        try:
            # assume all revision numbers are for changesets
            n = repo.lookup(rev)
            change = repo.changelog.read(n)
            m = repo.manifest.read(change[0])
            n = m[relpath(repo, [file])[0]]
        except hg.RepoError, KeyError:
            n = r.lookup(rev)
    else:
        n = r.tip()
    m = r.renamed(n)
    if m:
        ui.write("renamed from %s:%s\n" % (m[0], hex(m[1])))
    else:
        ui.write("not renamed\n")

def debugwalk(ui, repo, *pats, **opts):
    """show how files match on given patterns"""
    items = list(walk(repo, pats, opts))
    if not items:
        return
    fmt = '%%s  %%-%ds  %%-%ds  %%s\n' % (
        max([len(abs) for (src, abs, rel, exact) in items]),
        max([len(rel) for (src, abs, rel, exact) in items]))
    for src, abs, rel, exact in items:
        ui.write(fmt % (src, abs, rel, exact and 'exact' or ''))

def diff(ui, repo, *pats, **opts):
    """diff working directory (or selected files)"""
    node1, node2 = None, None
    revs = [repo.lookup(x) for x in opts['rev']]

    if len(revs) > 0:
        node1 = revs[0]
    if len(revs) > 1:
        node2 = revs[1]
    if len(revs) > 2:
        raise util.Abort("too many revisions to diff")

    files = []
    match = util.always
    if pats:
        roots, match, results = makewalk(repo, pats, opts)
        for src, abs, rel, exact in results:
            files.append(abs)

    dodiff(sys.stdout, ui, repo, node1, node2, files, match=match,
           text=opts['text'])

def doexport(ui, repo, changeset, seqno, total, revwidth, opts):
    node = repo.lookup(changeset)
    prev, other = repo.changelog.parents(node)
    change = repo.changelog.read(node)

    fp = make_file(repo, repo.changelog, opts['output'],
                   node=node, total=total, seqno=seqno,
                   revwidth=revwidth)
    if fp != sys.stdout:
        ui.note("%s\n" % fp.name)

    fp.write("# HG changeset patch\n")
    fp.write("# User %s\n" % change[1])
    fp.write("# Node ID %s\n" % hex(node))
    fp.write("# Parent  %s\n" % hex(prev))
    if other != nullid:
        fp.write("# Parent  %s\n" % hex(other))
    fp.write(change[4].rstrip())
    fp.write("\n\n")

    dodiff(fp, ui, repo, prev, node, text=opts['text'])
    if fp != sys.stdout:
        fp.close()

def export(ui, repo, *changesets, **opts):
    """dump the header and diffs for one or more changesets"""
    if not changesets:
        raise util.Abort("export requires at least one changeset")
    seqno = 0
    revs = list(revrange(ui, repo, changesets))
    total = len(revs)
    revwidth = max(map(len, revs))
    ui.note(len(revs) > 1 and "Exporting patches:\n" or "Exporting patch:\n")
    for cset in revs:
        seqno += 1
        doexport(ui, repo, cset, seqno, total, revwidth, opts)

def forget(ui, repo, *pats, **opts):
    """don't add the specified files on the next commit"""
    forget = []
    for src, abs, rel, exact in walk(repo, pats, opts):
        if repo.dirstate.state(abs) == 'a':
            forget.append(abs)
            if not exact:
                ui.status('forgetting ', rel, '\n')
    repo.forget(forget)

def grep(ui, repo, pattern, *pats, **opts):
    """search for a pattern in specified files and revisions"""
    reflags = 0
    if opts['ignore_case']:
        reflags |= re.I
    regexp = re.compile(pattern, reflags)
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

    class linestate:
        def __init__(self, line, linenum, colstart, colend):
            self.line = line
            self.linenum = linenum
            self.colstart = colstart
            self.colend = colend
        def __eq__(self, other):
            return self.line == other.line
        def __hash__(self):
            return hash(self.line)

    matches = {}
    def grepbody(fn, rev, body):
        matches[rev].setdefault(fn, {})
        m = matches[rev][fn]
        for lnum, cstart, cend, line in matchlines(body):
            s = linestate(line, lnum, cstart, cend)
            m[s] = s

    prev = {}
    ucache = {}
    def display(fn, rev, states, prevstates):
        diff = list(sets.Set(states).symmetric_difference(sets.Set(prevstates)))
        diff.sort(lambda x, y: cmp(x.linenum, y.linenum))
        counts = {'-': 0, '+': 0}
        filerevmatches = {}
        for l in diff:
            if incrementing or not opts['every_match']:
                change = ((l in prevstates) and '-') or '+'
                r = rev
            else:
                change = ((l in states) and '-') or '+'
                r = prev[fn]
            cols = [fn, str(rev)]
            if opts['line_number']: cols.append(str(l.linenum))
            if opts['every_match']: cols.append(change)
            if opts['user']: cols.append(trimuser(ui, getchange(rev)[1], rev,
                                                  ucache))
            if opts['files_with_matches']:
                c = (fn, rev)
                if c in filerevmatches: continue
                filerevmatches[c] = 1
            else:
                cols.append(l.line)
            ui.write(sep.join(cols), eol)
            counts[change] += 1
        return counts['+'], counts['-']

    fstate = {}
    skip = {}
    changeiter, getchange = walkchangerevs(ui, repo, repo.getcwd(), pats, opts)
    count = 0
    for st, rev, fns in changeiter:
        if st == 'window':
            incrementing = rev
            matches.clear()
        elif st == 'add':
            change = repo.changelog.read(repo.lookup(str(rev)))
            mf = repo.manifest.read(change[0])
            matches[rev] = {}
            for fn in fns:
                if fn in skip: continue
                fstate.setdefault(fn, {})
                try:
                    grepbody(fn, rev, getfile(fn).read(mf[fn]))
                except KeyError:
                    pass
        elif st == 'iter':
            states = matches[rev].items()
            states.sort()
            for fn, m in states:
                if fn in skip: continue
                if incrementing or not opts['every_match'] or fstate[fn]:
                    pos, neg = display(fn, rev, m, fstate[fn])
                    count += pos + neg
                    if pos and not opts['every_match']:
                        skip[fn] = True
                fstate[fn] = m
                prev[fn] = rev

    if not incrementing:
        fstate = fstate.items()
        fstate.sort()
        for fn, state in fstate:
            if fn in skip: continue
            display(fn, rev, {}, state)
    return (count == 0 and 1) or 0

def heads(ui, repo, **opts):
    """show current repository heads"""
    heads = repo.changelog.heads()
    br = None
    if opts['branches']:
        br = repo.branchlookup(heads)
    for n in repo.changelog.heads():
        show_changeset(ui, repo, changenode=n, brinfo=br)

def identify(ui, repo):
    """print information about the working copy"""
    parents = [p for p in repo.dirstate.parents() if p != nullid]
    if not parents:
        ui.write("unknown\n")
        return

    hexfunc = ui.verbose and hex or short
    (c, a, d, u) = repo.changes()
    output = ["%s%s" % ('+'.join([hexfunc(parent) for parent in parents]),
                        (c or a or d) and "+" or "")]

    if not ui.quiet:
        # multiple tags for a single parent separated by '/'
        parenttags = ['/'.join(tags)
                      for tags in map(repo.nodetags, parents) if tags]
        # tags for multiple parents separated by ' + '
        if parenttags:
            output.append(' + '.join(parenttags))

    ui.write("%s\n" % ' '.join(output))

def import_(ui, repo, patch1, *patches, **opts):
    """import an ordered set of patches"""
    patches = (patch1,) + patches

    if not opts['force']:
        (c, a, d, u) = repo.changes()
        if c or a or d:
            ui.warn("abort: outstanding uncommitted changes!\n")
            return 1

    d = opts["base"]
    strip = opts["strip"]

    mailre = re.compile(r'(From |[\w-]+:)')

    for patch in patches:
        ui.status("applying %s\n" % patch)
        pf = os.path.join(d, patch)

        message = []
        user = None
        hgpatch = False
        for line in file(pf):
            line = line.rstrip()
            if not message and mailre.match(line) and not opts['mail_like']:
                if len(line) > 35: line = line[:32] + '...'
                raise util.Abort('first line looks like a '
                                 'mail header: ' + line)
            if line.startswith("--- ") or line.startswith("diff -r"):
                break
            elif hgpatch:
                # parse values when importing the result of an hg export
                if line.startswith("# User "):
                    user = line[7:]
                    ui.debug('User: %s\n' % user)
                elif not line.startswith("# ") and line:
                    message.append(line)
                    hgpatch = False
            elif line == '# HG changeset patch':
                hgpatch = True
                message = []       # We may have collected garbage
            else:
                message.append(line)

        # make sure message isn't empty
        if not message:
            message = "imported patch %s\n" % patch
        else:
            message = "%s\n" % '\n'.join(message)
        ui.debug('message:\n%s\n' % message)

        f = os.popen("patch -p%d < '%s'" % (strip, pf))
        files = []
        for l in f.read().splitlines():
            l.rstrip('\r\n');
            ui.status("%s\n" % l)
            if l.startswith('patching file '):
                pf = l[14:]
                if pf not in files:
                    files.append(pf)
        patcherr = f.close()
        if patcherr:
            raise util.Abort("patch failed")

        if len(files) > 0:
            addremove(ui, repo, *files)
        repo.commit(files, message, user)

def incoming(ui, repo, source="default"):
    """show new changesets found in source"""
    source = ui.expandpath(source)
    other = hg.repository(ui, source)
    if not other.local():
        ui.warn("abort: incoming doesn't work for remote"
                + " repositories yet, sorry!\n")
        return 1
    o = repo.findincoming(other)
    if not o:
        return
    o = other.newer(o)
    o.reverse()
    for n in o:
        show_changeset(ui, other, changenode=n)

def init(ui, dest="."):
    """create a new repository in the given directory"""
    if not os.path.exists(dest):
        os.mkdir(dest)
    hg.repository(ui, dest, create=1)

def locate(ui, repo, *pats, **opts):
    """locate files matching specific patterns"""
    end = opts['print0'] and '\0' or '\n'

    for src, abs, rel, exact in walk(repo, pats, opts, '(?:.*/|)'):
        if repo.dirstate.state(abs) == '?':
            continue
        if opts['fullpath']:
            ui.write(os.path.join(repo.root, abs), end)
        else:
            ui.write(rel, end)

def log(ui, repo, *pats, **opts):
    """show revision history of entire repository or files"""
    class dui:
        # Implement and delegate some ui protocol.  Save hunks of
        # output for later display in the desired order.
        def __init__(self, ui):
            self.ui = ui
            self.hunk = {}
        def bump(self, rev):
            self.rev = rev
            self.hunk[rev] = []
        def note(self, *args):
            if self.verbose:
                self.write(*args)
        def status(self, *args):
            if not self.quiet:
                self.write(*args)
        def write(self, *args):
            self.hunk[self.rev].append(args)
        def __getattr__(self, key):
            return getattr(self.ui, key)
    cwd = repo.getcwd()
    if not pats and cwd:
        opts['include'] = [os.path.join(cwd, i) for i in opts['include']]
        opts['exclude'] = [os.path.join(cwd, x) for x in opts['exclude']]
    changeiter, getchange = walkchangerevs(ui, repo, (pats and cwd) or '',
                                           pats, opts)
    for st, rev, fns in changeiter:
        if st == 'window':
            du = dui(ui)
        elif st == 'add':
            du.bump(rev)
            show_changeset(du, repo, rev)
            if opts['patch']:
                changenode = repo.changelog.node(rev)
                prev, other = repo.changelog.parents(changenode)
                dodiff(du, du, repo, prev, changenode, fns)
                du.write("\n\n")
        elif st == 'iter':
            for args in du.hunk[rev]:
                ui.write(*args)

def manifest(ui, repo, rev=None):
    """output the latest or given revision of the project manifest"""
    if rev:
        try:
            # assume all revision numbers are for changesets
            n = repo.lookup(rev)
            change = repo.changelog.read(n)
            n = change[0]
        except hg.RepoError:
            n = repo.manifest.lookup(rev)
    else:
        n = repo.manifest.tip()
    m = repo.manifest.read(n)
    mf = repo.manifest.readflags(n)
    files = m.keys()
    files.sort()

    for f in files:
        ui.write("%40s %3s %s\n" % (hex(m[f]), mf[f] and "755" or "644", f))

def outgoing(ui, repo, dest="default-push"):
    """show changesets not found in destination"""
    dest = ui.expandpath(dest)
    other = hg.repository(ui, dest)
    o = repo.findoutgoing(other)
    o = repo.newer(o)
    o.reverse()
    for n in o:
        show_changeset(ui, repo, changenode=n)

def parents(ui, repo, rev=None):
    """show the parents of the working dir or revision"""
    if rev:
        p = repo.changelog.parents(repo.lookup(rev))
    else:
        p = repo.dirstate.parents()

    for n in p:
        if n != nullid:
            show_changeset(ui, repo, changenode=n)

def paths(ui, search=None):
    """show definition of symbolic path names"""
    try:
        repo = hg.repository(ui=ui)
    except hg.RepoError:
        pass

    if search:
        for name, path in ui.configitems("paths"):
            if name == search:
                ui.write("%s\n" % path)
                return
        ui.warn("not found!\n")
        return 1
    else:
        for name, path in ui.configitems("paths"):
            ui.write("%s = %s\n" % (name, path))

def pull(ui, repo, source="default", **opts):
    """pull changes from the specified source"""
    source = ui.expandpath(source)
    ui.status('pulling from %s\n' % (source))

    if opts['ssh']:
        ui.setconfig("ui", "ssh", opts['ssh'])
    if opts['remotecmd']:
        ui.setconfig("ui", "remotecmd", opts['remotecmd'])

    other = hg.repository(ui, source)
    r = repo.pull(other)
    if not r:
        if opts['update']:
            return update(ui, repo)
        else:
            ui.status("(run 'hg update' to get a working copy)\n")

    return r

def push(ui, repo, dest="default-push", force=False, ssh=None, remotecmd=None):
    """push changes to the specified destination"""
    dest = ui.expandpath(dest)
    ui.status('pushing to %s\n' % (dest))

    if ssh:
        ui.setconfig("ui", "ssh", ssh)
    if remotecmd:
        ui.setconfig("ui", "remotecmd", remotecmd)

    other = hg.repository(ui, dest)
    r = repo.push(other, force)
    return r

def rawcommit(ui, repo, *flist, **rc):
    "raw commit interface"
    if rc['text']:
        ui.warn("Warning: -t and --text is deprecated,"
                " please use -m or --message instead.\n")
    message = rc['message'] or rc['text']
    if not message and rc['logfile']:
        try:
            message = open(rc['logfile']).read()
        except IOError:
            pass
    if not message and not rc['logfile']:
        ui.warn("abort: missing commit message\n")
        return 1

    files = relpath(repo, list(flist))
    if rc['files']:
        files += open(rc['files']).read().splitlines()

    rc['parent'] = map(repo.lookup, rc['parent'])

    repo.rawcommit(files, message, rc['user'], rc['date'], *rc['parent'])

def recover(ui, repo):
    """roll back an interrupted transaction"""
    repo.recover()

def remove(ui, repo, pat, *pats, **opts):
    """remove the specified files on the next commit"""
    names = []
    def okaytoremove(abs, rel, exact):
        c, a, d, u = repo.changes(files = [abs])
        reason = None
        if c: reason = 'is modified'
        elif a: reason = 'has been marked for add'
        elif u: reason = 'not managed'
        if reason and exact:
            ui.warn('not removing %s: file %s\n' % (rel, reason))
        else:
            return True
    for src, abs, rel, exact in walk(repo, (pat,) + pats, opts):
        if okaytoremove(abs, rel, exact):
            if not exact: ui.status('removing %s\n' % rel)
            names.append(abs)
    repo.remove(names)

def revert(ui, repo, *names, **opts):
    """revert modified files or dirs back to their unmodified states"""
    node = opts['rev'] and repo.lookup(opts['rev']) or \
           repo.dirstate.parents()[0]
    root = os.path.realpath(repo.root)

    def trimpath(p):
        p = os.path.realpath(p)
        if p.startswith(root):
            rest = p[len(root):]
            if not rest:
                return rest
            if p.startswith(os.sep):
                return rest[1:]
            return p

    relnames = map(trimpath, names or [os.getcwd()])
    chosen = {}

    def choose(name):
        def body(name):
            for r in relnames:
                if not name.startswith(r):
                    continue
                rest = name[len(r):]
                if not rest:
                    return r, True
                depth = rest.count(os.sep)
                if not r:
                    if depth == 0 or not opts['nonrecursive']:
                        return r, True
                elif rest[0] == os.sep:
                    if depth == 1 or not opts['nonrecursive']:
                        return r, True
            return None, False
        relname, ret = body(name)
        if ret:
            chosen[relname] = 1
        return ret

    r = repo.update(node, False, True, choose, False)
    for n in relnames:
        if n not in chosen:
            ui.warn('error: no matches for %s\n' % n)
            r = 1
    sys.stdout.flush()
    return r

def root(ui, repo):
    """print the root (top) of the current working dir"""
    ui.write(repo.root + "\n")

def serve(ui, repo, **opts):
    """export the repository via HTTP"""

    if opts["stdio"]:
        fin, fout = sys.stdin, sys.stdout
        sys.stdout = sys.stderr

        def getarg():
            argline = fin.readline()[:-1]
            arg, l = argline.split()
            val = fin.read(int(l))
            return arg, val
        def respond(v):
            fout.write("%d\n" % len(v))
            fout.write(v)
            fout.flush()

        lock = None

        while 1:
            cmd = fin.readline()[:-1]
            if cmd == '':
                return
            if cmd == "heads":
                h = repo.heads()
                respond(" ".join(map(hex, h)) + "\n")
            if cmd == "lock":
                lock = repo.lock()
                respond("")
            if cmd == "unlock":
                if lock:
                    lock.release()
                lock = None
                respond("")
            elif cmd == "branches":
                arg, nodes = getarg()
                nodes = map(bin, nodes.split(" "))
                r = []
                for b in repo.branches(nodes):
                    r.append(" ".join(map(hex, b)) + "\n")
                respond("".join(r))
            elif cmd == "between":
                arg, pairs = getarg()
                pairs = [map(bin, p.split("-")) for p in pairs.split(" ")]
                r = []
                for b in repo.between(pairs):
                    r.append(" ".join(map(hex, b)) + "\n")
                respond("".join(r))
            elif cmd == "changegroup":
                nodes = []
                arg, roots = getarg()
                nodes = map(bin, roots.split(" "))

                cg = repo.changegroup(nodes)
                while 1:
                    d = cg.read(4096)
                    if not d:
                        break
                    fout.write(d)

                fout.flush()

            elif cmd == "addchangegroup":
                if not lock:
                    respond("not locked")
                    continue
                respond("")

                r = repo.addchangegroup(fin)
                respond("")

    optlist = "name templates style address port ipv6 accesslog errorlog"
    for o in optlist.split():
        if opts[o]:
            ui.setconfig("web", o, opts[o])

    try:
        httpd = hgweb.create_server(repo)
    except socket.error, inst:
        raise util.Abort('cannot start server: ' + inst.args[1])

    if ui.verbose:
        addr, port = httpd.socket.getsockname()
        if addr == '0.0.0.0':
            addr = socket.gethostname()
        else:
            try:
                addr = socket.gethostbyaddr(addr)[0]
            except socket.error:
                pass
        if port != 80:
            ui.status('listening at http://%s:%d/\n' % (addr, port))
        else:
            ui.status('listening at http://%s/\n' % addr)
    httpd.serve_forever()

def status(ui, repo, *pats, **opts):
    '''show changed files in the working directory

    M = modified
    A = added
    R = removed
    ? = not tracked
    '''

    cwd = repo.getcwd()
    files, matchfn, anypats = matchpats(repo, cwd, pats, opts)
    (c, a, d, u) = [[util.pathto(cwd, x) for x in n]
                    for n in repo.changes(files=files, match=matchfn)]

    changetypes = [('modified', 'M', c),
                   ('added', 'A', a),
                   ('removed', 'R', d),
                   ('unknown', '?', u)]

    end = opts['print0'] and '\0' or '\n'

    for opt, char, changes in ([ct for ct in changetypes if opts[ct[0]]]
                               or changetypes):
        if opts['no_status']:
            format = "%%s%s" % end
        else:
            format = "%s %%s%s" % (char, end);

        for f in changes:
            ui.write(format % f)

def tag(ui, repo, name, rev=None, **opts):
    """add a tag for the current tip or a given revision"""
    if opts['text']:
        ui.warn("Warning: -t and --text is deprecated,"
                " please use -m or --message instead.\n")
    if name == "tip":
        ui.warn("abort: 'tip' is a reserved name!\n")
        return -1
    if rev:
        r = hex(repo.lookup(rev))
    else:
        r = hex(repo.changelog.tip())

    if name.find(revrangesep) >= 0:
        ui.warn("abort: '%s' cannot be used in a tag name\n" % revrangesep)
        return -1

    if opts['local']:
        repo.opener("localtags", "a").write("%s %s\n" % (r, name))
        return

    (c, a, d, u) = repo.changes()
    for x in (c, a, d, u):
        if ".hgtags" in x:
            ui.warn("abort: working copy of .hgtags is changed!\n")
            ui.status("(please commit .hgtags manually)\n")
            return -1

    repo.wfile(".hgtags", "ab").write("%s %s\n" % (r, name))
    if repo.dirstate.state(".hgtags") == '?':
        repo.add([".hgtags"])

    message = (opts['message'] or opts['text'] or
               "Added tag %s for changeset %s" % (name, r))
    repo.commit([".hgtags"], message, opts['user'], opts['date'])

def tags(ui, repo):
    """list repository tags"""

    l = repo.tagslist()
    l.reverse()
    for t, n in l:
        try:
            r = "%5d:%s" % (repo.changelog.rev(n), hex(n))
        except KeyError:
            r = "    ?:?"
        ui.write("%-30s %s\n" % (t, r))

def tip(ui, repo):
    """show the tip revision"""
    n = repo.changelog.tip()
    show_changeset(ui, repo, changenode=n)

def undo(ui, repo):
    """undo the last commit or pull

    Roll back the last pull or commit transaction on the
    repository, restoring the project to its earlier state.

    This command should be used with care. There is only one level of
    undo and there is no redo.

    This command is not intended for use on public repositories. Once
    a change is visible for pull by other users, undoing it locally is
    ineffective.
    """
    repo.undo()

def update(ui, repo, node=None, merge=False, clean=False, branch=None):
    '''update or merge working directory

    If there are no outstanding changes in the working directory and
    there is a linear relationship between the current version and the
    requested version, the result is the requested version.

    Otherwise the result is a merge between the contents of the
    current working directory and the requested version. Files that
    changed between either parent are marked as changed for the next
    commit and a commit must be performed before any further updates
    are allowed.
    '''
    if branch:
        br = repo.branchlookup(branch=branch)
        found = []
        for x in br:
            if branch in br[x]:
                found.append(x)
        if len(found) > 1:
            ui.warn("Found multiple heads for %s\n" % branch)
            for x in found:
                show_changeset(ui, repo, changenode=x, brinfo=br)
            return 1
        if len(found) == 1:
            node = found[0]
            ui.warn("Using head %s for branch %s\n" % (short(node), branch))
        else:
            ui.warn("branch %s not found\n" % (branch))
            return 1
    else:
        node = node and repo.lookup(node) or repo.changelog.tip()
    return repo.update(node, allow=merge, force=clean)

def verify(ui, repo):
    """verify the integrity of the repository"""
    return repo.verify()

# Command options and aliases are listed here, alphabetically

table = {
    "^add":
        (add,
         [('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         "hg add [OPTION]... [FILE]..."),
    "addremove":
        (addremove,
         [('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         "hg addremove [OPTION]... [FILE]..."),
    "^annotate":
        (annotate,
         [('r', 'rev', '', 'revision'),
          ('a', 'text', None, 'treat all files as text'),
          ('u', 'user', None, 'show user'),
          ('n', 'number', None, 'show revision number'),
          ('c', 'changeset', None, 'show changeset'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         'hg annotate [OPTION]... FILE...'),
    "cat":
        (cat,
         [('o', 'output', "", 'output to file')],
         'hg cat [-o OUTFILE] FILE [REV]'),
    "^clone":
        (clone,
         [('U', 'noupdate', None, 'skip update after cloning'),
          ('e', 'ssh', "", 'ssh command'),
          ('', 'remotecmd', "", 'remote hg command')],
         'hg clone [OPTION]... SOURCE [DEST]'),
    "^commit|ci":
        (commit,
         [('A', 'addremove', None, 'run add/remove during commit'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search'),
          ('m', 'message', "", 'commit message'),
          ('t', 'text', "", 'commit message (deprecated: use -m)'),
          ('l', 'logfile', "", 'commit message file'),
          ('d', 'date', "", 'date code'),
          ('u', 'user', "", 'user')],
         'hg commit [OPTION]... [FILE]...'),
    "copy": (copy, [], 'hg copy SOURCE DEST'),
    "debugcheckstate": (debugcheckstate, [], 'debugcheckstate'),
    "debugconfig": (debugconfig, [], 'debugconfig'),
    "debugstate": (debugstate, [], 'debugstate'),
    "debugdata": (debugdata, [], 'debugdata FILE REV'),
    "debugindex": (debugindex, [], 'debugindex FILE'),
    "debugindexdot": (debugindexdot, [], 'debugindexdot FILE'),
    "debugrename": (debugrename, [], 'debugrename FILE [REV]'),
    "debugwalk":
        (debugwalk,
         [('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         'debugwalk [OPTION]... [FILE]...'),
    "^diff":
        (diff,
         [('r', 'rev', [], 'revision'),
          ('a', 'text', None, 'treat all files as text'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         'hg diff [-a] [-I] [-X] [-r REV1 [-r REV2]] [FILE]...'),
    "^export":
        (export,
         [('o', 'output', "", 'output to file'),
          ('a', 'text', None, 'treat all files as text')],
         "hg export [-a] [-o OUTFILE] REV..."),
    "forget":
        (forget,
         [('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         "hg forget [OPTION]... FILE..."),
    "grep":
        (grep,
         [('0', 'print0', None, 'end fields with NUL'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'include path in search'),
          ('e', 'every-match', None, 'print every rev with matches'),
          ('i', 'ignore-case', None, 'ignore case when matching'),
          ('l', 'files-with-matches', None, 'print names of files and revs with matches'),
          ('n', 'line-number', None, 'print line numbers'),
          ('r', 'rev', [], 'search in revision rev'),
          ('u', 'user', None, 'print user who made change')],
         "hg grep [OPTION]... PATTERN [FILE]..."),
    "heads":
        (heads,
         [('b', 'branches', None, 'find branch info')],
         'hg heads [-b]'),
    "help": (help_, [], 'hg help [COMMAND]'),
    "identify|id": (identify, [], 'hg identify'),
    "import|patch":
        (import_,
         [('p', 'strip', 1, 'path strip'),
          ('f', 'force', None, 'skip check for outstanding changes'),
          ('b', 'base', "", 'base path'),
          ('m', 'mail-like', None, 'apply a patch that looks like email')],
         "hg import [-f] [-p NUM] [-b BASE] PATCH..."),
    "incoming|in": (incoming, [], 'hg incoming [SOURCE]'),
    "^init": (init, [], 'hg init [DEST]'),
    "locate":
        (locate,
         [('r', 'rev', '', 'revision'),
          ('0', 'print0', None, 'end filenames with NUL'),
          ('f', 'fullpath', None, 'print complete paths'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         'hg locate [OPTION]... [PATTERN]...'),
    "^log|history":
        (log,
         [('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search'),
          ('r', 'rev', [], 'revision'),
          ('p', 'patch', None, 'show patch')],
         'hg log [-I] [-X] [-r REV]... [-p] [FILE]'),
    "manifest": (manifest, [], 'hg manifest [REV]'),
    "outgoing|out": (outgoing, [], 'hg outgoing [DEST]'),
    "parents": (parents, [], 'hg parents [REV]'),
    "paths": (paths, [], 'hg paths [NAME]'),
    "^pull":
        (pull,
         [('u', 'update', None, 'update working directory'),
          ('e', 'ssh', "", 'ssh command'),
          ('', 'remotecmd', "", 'remote hg command')],
         'hg pull [-u] [-e FILE] [--remotecmd FILE] [SOURCE]'),
    "^push":
        (push,
         [('f', 'force', None, 'force push'),
          ('e', 'ssh', "", 'ssh command'),
          ('', 'remotecmd', "", 'remote hg command')],
         'hg push [-f] [-e FILE] [--remotecmd FILE] [DEST]'),
    "rawcommit":
        (rawcommit,
         [('p', 'parent', [], 'parent'),
          ('d', 'date', "", 'date code'),
          ('u', 'user', "", 'user'),
          ('F', 'files', "", 'file list'),
          ('m', 'message', "", 'commit message'),
          ('t', 'text', "", 'commit message (deprecated: use -m)'),
          ('l', 'logfile', "", 'commit message file')],
         'hg rawcommit [OPTION]... [FILE]...'),
    "recover": (recover, [], "hg recover"),
    "^remove|rm": (remove,
                   [('I', 'include', [], 'include path in search'),
                    ('X', 'exclude', [], 'exclude path from search')],
                   "hg remove [OPTION]... FILE..."),
    "^revert":
        (revert,
         [("n", "nonrecursive", None, "don't recurse into subdirs"),
          ("r", "rev", "", "revision")],
         "hg revert [-n] [-r REV] [NAME]..."),
    "root": (root, [], "hg root"),
    "^serve":
        (serve,
         [('A', 'accesslog', '', 'access log file'),
          ('E', 'errorlog', '', 'error log file'),
          ('p', 'port', 0, 'listen port'),
          ('a', 'address', '', 'interface address'),
          ('n', 'name', "", 'repository name'),
          ('', 'stdio', None, 'for remote clients'),
          ('t', 'templates', "", 'template directory'),
          ('', 'style', "", 'template style'),
          ('6', 'ipv6', None, 'use IPv6 in addition to IPv4')],
         "hg serve [OPTION]..."),
    "^status":
        (status,
         [('m', 'modified', None, 'show only modified files'),
          ('a', 'added', None, 'show only added files'),
          ('r', 'removed', None, 'show only removed files'),
          ('u', 'unknown', None, 'show only unknown (not tracked) files'),
          ('n', 'no-status', None, 'hide status prefix'),
          ('0', 'print0', None, 'end filenames with NUL'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         "hg status [OPTION]... [FILE]..."),
    "tag":
        (tag,
         [('l', 'local', None, 'make the tag local'),
          ('m', 'message', "", 'commit message'),
          ('t', 'text', "", 'commit message (deprecated: use -m)'),
          ('d', 'date', "", 'date code'),
          ('u', 'user', "", 'user')],
         'hg tag [OPTION]... NAME [REV]'),
    "tags": (tags, [], 'hg tags'),
    "tip": (tip, [], 'hg tip'),
    "undo": (undo, [], 'hg undo'),
    "^update|up|checkout|co":
        (update,
         [('b', 'branch', "", 'checkout the head of a specific branch'),
          ('m', 'merge', None, 'allow merging of conflicts'),
          ('C', 'clean', None, 'overwrite locally modified files')],
         'hg update [-b TAG] [-m] [-C] [REV]'),
    "verify": (verify, [], 'hg verify'),
    "version": (show_version, [], 'hg version'),
}

globalopts = [
    ('R', 'repository', "", 'repository root directory'),
    ('', 'cwd', '', 'change working directory'),
    ('y', 'noninteractive', None, 'run non-interactively'),
    ('q', 'quiet', None, 'quiet mode'),
    ('v', 'verbose', None, 'verbose mode'),
    ('', 'debug', None, 'debug mode'),
    ('', 'traceback', None, 'print traceback on exception'),
    ('', 'time', None, 'time how long the command takes'),
    ('', 'profile', None, 'profile'),
    ('', 'version', None, 'output version information and exit'),
    ('h', 'help', None, 'display help and exit'),
]

norepo = ("clone init version help debugconfig debugdata"
          " debugindex debugindexdot paths")

def find(cmd):
    for e in table.keys():
        if re.match("(%s)$" % e, cmd):
            return e, table[e]

    raise UnknownCommand(cmd)

class SignalInterrupt(Exception):
    """Exception raised on SIGTERM and SIGHUP."""

def catchterm(*args):
    raise SignalInterrupt

def run():
    sys.exit(dispatch(sys.argv[1:]))

class ParseError(Exception):
    """Exception raised on errors in parsing the command line."""

def parse(args):
    options = {}
    cmdoptions = {}

    try:
        args = fancyopts.fancyopts(args, globalopts, options)
    except fancyopts.getopt.GetoptError, inst:
        raise ParseError(None, inst)

    if args:
        cmd, args = args[0], args[1:]
        i = find(cmd)[1]
        c = list(i[1])
    else:
        cmd = None
        c = []

    # combine global options into local
    for o in globalopts:
        c.append((o[0], o[1], options[o[1]], o[3]))

    try:
        args = fancyopts.fancyopts(args, c, cmdoptions)
    except fancyopts.getopt.GetoptError, inst:
        raise ParseError(cmd, inst)

    # separate global options back out
    for o in globalopts:
        n = o[1]
        options[n] = cmdoptions[n]
        del cmdoptions[n]

    return (cmd, cmd and i[0] or None, args, options, cmdoptions)

def dispatch(args):
    signal.signal(signal.SIGTERM, catchterm)
    try:
        signal.signal(signal.SIGHUP, catchterm)
    except AttributeError:
        pass

    u = ui.ui()
    external = []
    for x in u.extensions():
        if x[1]:
            mod = imp.load_source(x[0], x[1])
        else:
            def importh(name):
                mod = __import__(name)
                components = name.split('.')
                for comp in components[1:]:
                    mod = getattr(mod, comp)
                return mod
            mod = importh(x[0])
        external.append(mod)
    for x in external:
        for t in x.cmdtable:
            if t in table:
                u.warn("module %s override %s\n" % (x.__name__, t))
        table.update(x.cmdtable)

    try:
        cmd, func, args, options, cmdoptions = parse(args)
    except ParseError, inst:
        if inst.args[0]:
            u.warn("hg %s: %s\n" % (inst.args[0], inst.args[1]))
            help_(u, inst.args[0])
        else:
            u.warn("hg: %s\n" % inst.args[1])
            help_(u, 'shortlist')
        sys.exit(-1)
    except UnknownCommand, inst:
        u.warn("hg: unknown command '%s'\n" % inst.args[0])
        help_(u, 'shortlist')
        sys.exit(1)

    if options["time"]:
        def get_times():
            t = os.times()
            if t[4] == 0.0: # Windows leaves this as zero, so use time.clock()
                t = (t[0], t[1], t[2], t[3], time.clock())
            return t
        s = get_times()
        def print_time():
            t = get_times()
            u.warn("Time: real %.3f secs (user %.3f+%.3f sys %.3f+%.3f)\n" %
                (t[4]-s[4], t[0]-s[0], t[2]-s[2], t[1]-s[1], t[3]-s[3]))
        atexit.register(print_time)

    u.updateopts(options["verbose"], options["debug"], options["quiet"],
              not options["noninteractive"])

    try:
        try:
            if options['help']:
                help_(u, cmd, options['version'])
                sys.exit(0)
            elif options['version']:
                show_version(u)
                sys.exit(0)
            elif not cmd:
                help_(u, 'shortlist')
                sys.exit(0)

            if options['cwd']:
                try:
                    os.chdir(options['cwd'])
                except OSError, inst:
                    u.warn('abort: %s: %s\n' % (options['cwd'], inst.strerror))
                    sys.exit(1)

            if cmd not in norepo.split():
                path = options["repository"] or ""
                repo = hg.repository(ui=u, path=path)
                for x in external:
                    x.reposetup(u, repo)
                d = lambda: func(u, repo, *args, **cmdoptions)
            else:
                d = lambda: func(u, *args, **cmdoptions)

            if options['profile']:
                import hotshot, hotshot.stats
                prof = hotshot.Profile("hg.prof")
                r = prof.runcall(d)
                prof.close()
                stats = hotshot.stats.load("hg.prof")
                stats.strip_dirs()
                stats.sort_stats('time', 'calls')
                stats.print_stats(40)
                return r
            else:
                return d()
        except:
            if options['traceback']:
                traceback.print_exc()
            raise
    except hg.RepoError, inst:
        u.warn("abort: ", inst, "!\n")
    except SignalInterrupt:
        u.warn("killed!\n")
    except KeyboardInterrupt:
        try:
            u.warn("interrupted!\n")
        except IOError, inst:
            if inst.errno == errno.EPIPE:
                if u.debugflag:
                    u.warn("\nbroken pipe\n")
            else:
                raise
    except IOError, inst:
        if hasattr(inst, "code"):
            u.warn("abort: %s\n" % inst)
        elif hasattr(inst, "reason"):
            u.warn("abort: error: %s\n" % inst.reason[1])
        elif hasattr(inst, "args") and inst[0] == errno.EPIPE:
            if u.debugflag:
                u.warn("broken pipe\n")
        else:
            raise
    except OSError, inst:
        if hasattr(inst, "filename"):
            u.warn("abort: %s: %s\n" % (inst.strerror, inst.filename))
        else:
            u.warn("abort: %s\n" % inst.strerror)
    except util.Abort, inst:
        u.warn('abort: ', inst.args[0] % inst.args[1:], '\n')
        sys.exit(1)
    except TypeError, inst:
        # was this an argument error?
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) > 2: # no
            raise
        u.debug(inst, "\n")
        u.warn("%s: invalid arguments\n" % cmd)
        help_(u, cmd)
    except UnknownCommand, inst:
        u.warn("hg: unknown command '%s'\n" % inst.args[0])
        help_(u, 'shortlist')

    sys.exit(-1)
