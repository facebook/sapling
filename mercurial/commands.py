# commands.py - command processing for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
demandload(globals(), "os re sys signal shutil")
demandload(globals(), "fancyopts ui hg util")
demandload(globals(), "fnmatch hgweb mdiff random signal time traceback")
demandload(globals(), "errno socket version struct atexit")

class UnknownCommand(Exception):
    """Exception raised if command is not in the command table."""

class Abort(Exception):
    """Raised if a command needs to print an error and exit."""

def filterfiles(filters, files):
    l = [x for x in files if x in filters]

    for t in filters:
        if t and t[-1] != "/":
            t += "/"
        l += [x for x in files if x.startswith(t)]
    return l

def relfilter(repo, files):
    cwd = repo.getcwd()
    if cwd:
        return filterfiles([util.pconvert(cwd)], files)
    return files

def relpath(repo, args):
    cwd = repo.getcwd()
    if cwd:
        return [util.pconvert(os.path.normpath(os.path.join(cwd, x)))
                for x in args]
    return args

def matchpats(cwd, pats = [], opts = {}, head = ''):
    return util.matcher(cwd, pats or ['.'], opts.get('include'),
                        opts.get('exclude'), head)

def pathto(n1, n2):
    '''return the relative path from one place to another'''
    if not n1: return n2
    a, b = n1.split(os.sep), n2.split(os.sep)
    a.reverse(), b.reverse()
    while a and b and a[-1] == b[-1]:
        a.pop(), b.pop()
    b.reverse()
    return os.sep.join((['..'] * len(a)) + b)

def walk(repo, pats, opts, head = ''):
    cwd = repo.getcwd()
    files, matchfn = matchpats(cwd, pats, opts, head)
    for src, fn in repo.walk(files = files, match = matchfn):
        yield src, fn, pathto(cwd, fn)

revrangesep = ':'

def revrange(ui, repo, revs, revlog=None):
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
                    raise Abort('invalid revision identifier %s', val)
        return num
    for spec in revs:
        if spec.find(revrangesep) >= 0:
            start, end = spec.split(revrangesep, 1)
            start = fix(start, 0)
            end = fix(end, revcount - 1)
            if end > start:
                end += 1
                step = 1
            else:
                end -= 1
                step = -1
            for rev in xrange(start, end, step):
                yield str(rev)
        else:
            yield spec

def make_filename(repo, r, pat, node=None,
                  total=None, seqno=None, revwidth=None):
    node_expander = {
        'H': lambda: hg.hex(node),
        'R': lambda: str(r.rev(node)),
        'h': lambda: hg.short(node),
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
        raise Abort("invalid format spec '%%%s' in output file name",
                    inst.args[0])

def make_file(repo, r, pat, node=None,
              total=None, seqno=None, revwidth=None, mode='wb'):
    if not pat or pat == '-':
        if 'w' in mode: return sys.stdout
        else: return sys.stdin
    if hasattr(pat, 'write') and 'w' in mode:
        return pat
    if hasattr(pat, 'read') and 'r' in mode:
        return pat
    return open(make_filename(repo, r, pat, node, total, seqno, revwidth),
                mode)

def dodiff(fp, ui, repo, files=None, node1=None, node2=None):
    def date(c):
        return time.asctime(time.gmtime(float(c[2].split(' ')[0])))

    (c, a, d, u) = repo.changes(node1, node2, files)
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
        hexfunc = ui.verbose and hg.hex or hg.short
        r = [hexfunc(node) for node in [node1, node2] if node]

    change = repo.changelog.read(node1)
    mmap = repo.manifest.read(change[0])
    date1 = date(change)

    for f in c:
        to = None
        if f in mmap:
            to = repo.file(f).read(mmap[f])
        tn = read(f)
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r))
    for f in a:
        to = None
        tn = read(f)
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r))
    for f in d:
        to = repo.file(f).read(mmap[f])
        tn = None
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r))

def show_changeset(ui, repo, rev=0, changenode=None, filelog=None):
    """show a single changeset or file revision"""
    changelog = repo.changelog
    if filelog:
        log = filelog
        filerev = rev
        node = filenode = filelog.node(filerev)
        changerev = filelog.linkrev(filenode)
        changenode = changenode or changelog.node(changerev)
    else:
        log = changelog
        changerev = rev
        if changenode is None:
            changenode = changelog.node(changerev)
        elif not changerev:
            rev = changerev = changelog.rev(changenode)
        node = changenode

    if ui.quiet:
        ui.write("%d:%s\n" % (rev, hg.hex(node)))
        return

    changes = changelog.read(changenode)

    parents = [(log.rev(p), ui.verbose and hg.hex(p) or hg.short(p))
               for p in log.parents(node)
               if ui.debugflag or p != hg.nullid]
    if not ui.debugflag and len(parents) == 1 and parents[0][0] == rev-1:
        parents = []

    if ui.verbose:
        ui.write("changeset:   %d:%s\n" % (changerev, hg.hex(changenode)))
    else:
        ui.write("changeset:   %d:%s\n" % (changerev, hg.short(changenode)))

    for tag in repo.nodetags(changenode):
        ui.status("tag:         %s\n" % tag)
    for parent in parents:
        ui.write("parent:      %d:%s\n" % parent)
    if filelog:
        ui.debug("file rev:    %d:%s\n" % (filerev, hg.hex(filenode)))

    ui.debug("manifest:    %d:%s\n" % (repo.manifest.rev(changes[0]),
                                      hg.hex(changes[0])))
    ui.status("user:        %s\n" % changes[1])
    ui.status("date:        %s\n" % time.asctime(
        time.localtime(float(changes[2].split(' ')[0]))))

    if ui.debugflag:
        files = repo.changes(changelog.parents(changenode)[0], changenode)
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
    ui.write("Mercurial version %s\n" % version.get_version())
    ui.status(
        "\nCopyright (C) 2005 Matt Mackall <mpm@selenic.com>\n"
        "This is free software; see the source for copying conditions. "
        "There is NO\nwarranty; "
        "not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n"
    )

def help_(ui, cmd=None):
    """show help for a given command or all commands"""
    if cmd:
        try:
            i = find(cmd)
            ui.write("%s\n\n" % i[2])

            if i[1]:
                for s, l, d, c in i[1]:
                    opt = ' '
                    if s:
                        opt = opt + '-' + s + ' '
                    if l:
                        opt = opt + '--' + l + ' '
                    if d:
                        opt = opt + '(' + str(d) + ')'
                    ui.write(opt, "\n")
                    if c:
                        ui.write('   %s\n' % c)
                ui.write("\n")

            ui.write(i[0].__doc__, "\n")
        except UnknownCommand:
            ui.warn("hg: unknown command %s\n" % cmd)
        sys.exit(0)
    else:
        if ui.verbose:
            show_version(ui)
            ui.write('\n')
        if ui.verbose:
            ui.write('hg commands:\n\n')
        else:
            ui.write('basic hg commands (use "hg help -v" for more):\n\n')

        h = {}
        for c, e in table.items():
            f = c.split("|")[0]
            if not ui.verbose and not f.startswith("^"):
                continue
            if not ui.debugflag and f.startswith("debug"):
                continue
            f = f.lstrip("^")
            d = ""
            if e[0].__doc__:
                d = e[0].__doc__.splitlines(0)[0].rstrip()
            h[f] = d

        fns = h.keys()
        fns.sort()
        m = max(map(len, fns))
        for f in fns:
            ui.write(' %-*s   %s\n' % (m, f, h[f]))

# Commands start here, listed alphabetically

def add(ui, repo, *pats, **opts):
    '''add the specified files on the next commit'''
    names = []
    q = dict(zip(pats, pats))
    for src, abs, rel in walk(repo, pats, opts):
        if rel in q or abs in q:
            names.append(abs)
        elif repo.dirstate.state(abs) == '?':
            ui.status('adding %s\n' % rel)
            names.append(abs)
    repo.add(names)

def addremove(ui, repo, *pats, **opts):
    """add all new files, delete all missing files"""
    q = dict(zip(pats, pats))
    add, remove = [], []
    for src, abs, rel in walk(repo, pats, opts):
        if src == 'f':
            if repo.dirstate.state(abs) == '?':
                add.append(abs)
                if rel not in q: ui.status('adding ', rel, '\n')
        elif repo.dirstate.state(abs) != 'r' and not os.path.exists(rel):
            remove.append(abs)
            if rel not in q: ui.status('removing ', rel, '\n')
    repo.add(add)
    repo.remove(remove)

def annotate(ui, repo, *pats, **opts):
    """show changeset information per file line"""
    def getnode(rev):
        return hg.short(repo.changelog.node(rev))

    def getname(rev):
        try:
            return bcache[rev]
        except KeyError:
            cl = repo.changelog.read(repo.changelog.node(rev))
            name = cl[1]
            f = name.find('@')
            if f >= 0:
                name = name[:f]
            f = name.find('<')
            if f >= 0:
                name = name[f+1:]
            bcache[rev] = name
            return name

    if not pats:
        raise Abort('at least one file name or pattern required')

    bcache = {}
    opmap = [['user', getname], ['number', str], ['changeset', getnode]]
    if not opts['user'] and not opts['changeset']:
        opts['number'] = 1

    if opts['rev']:
        node = repo.changelog.lookup(opts['rev'])
    else:
        node = repo.dirstate.parents()[0]
    change = repo.changelog.read(node)
    mmap = repo.manifest.read(change[0])
    for src, abs, rel in walk(repo, pats, opts):
        if abs not in mmap:
            ui.warn("warning: %s is not in the repository!\n" % rel)
            continue

        lines = repo.file(abs).annotate(mmap[abs])
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

    d = Dircleanup(dest)
    abspath = source
    source = ui.expandpath(source)
    other = hg.repository(ui, source)

    if other.dev() != -1:
        abspath = os.path.abspath(source)
        copyfile = (os.stat(dest).st_dev == other.dev()
                    and getattr(os, 'link', None) or shutil.copy2)
        if copyfile is not shutil.copy2:
            ui.note("cloning by hardlink\n")
        util.copytree(os.path.join(source, ".hg"), os.path.join(dest, ".hg"),
                      copyfile)
        try:
            os.unlink(os.path.join(dest, ".hg", "dirstate"))
        except OSError:
            pass

        repo = hg.repository(ui, dest)

    else:
        repo = hg.repository(ui, dest, create=1)
        repo.pull(other)

    f = repo.opener("hgrc", "w")
    f.write("[paths]\n")
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
            message = open(logfile).read()
        except IOError, why:
            ui.warn("Can't read commit message %s: %s\n" % (logfile, why))

    if opts['addremove']:
        addremove(ui, repo, *pats, **opts)
    cwd = repo.getcwd()
    if not pats and cwd:
        opts['include'] = [os.path.join(cwd, i) for i in opts['include']]
        opts['exclude'] = [os.path.join(cwd, x) for x in opts['exclude']]
    fns, match = matchpats((pats and repo.getcwd()) or '', pats, opts)
    if pats:
        c, a, d, u = repo.changes(files = fns, match = match)
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
        raise Abort(".hg/dirstate inconsistent with current parent's manifest")

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

def debugindex(ui, file_):
    """dump the contents of an index file"""
    r = hg.revlog(hg.opener(""), file_, "")
    ui.write("   rev    offset  length   base linkrev" +
             " p1           p2           nodeid\n")
    for i in range(r.count()):
        e = r.index[i]
        ui.write("% 6d % 9d % 7d % 6d % 7d %s.. %s.. %s..\n" % (
                i, e[0], e[1], e[2], e[3],
            hg.hex(e[4][:5]), hg.hex(e[5][:5]), hg.hex(e[6][:5])))

def debugindexdot(ui, file_):
    """dump an index DAG as a .dot file"""
    r = hg.revlog(hg.opener(""), file_, "")
    ui.write("digraph G {\n")
    for i in range(r.count()):
        e = r.index[i]
        ui.write("\t%d -> %d\n" % (r.rev(e[4]), i))
        if e[5] != hg.nullid:
            ui.write("\t%d -> %d\n" % (r.rev(e[5]), i))
    ui.write("}\n")

def debugwalk(ui, repo, *pats, **opts):
    items = list(walk(repo, pats, opts))
    fmt = '%%s  %%-%ds  %%s' % max([len(abs) for (src, abs, rel) in items])
    for i in items: print fmt % i

def diff(ui, repo, *pats, **opts):
    """diff working directory (or selected files)"""
    revs = []
    if opts['rev']:
        revs = map(lambda x: repo.lookup(x), opts['rev'])

    if len(revs) > 2:
        raise Abort("too many revisions to diff")

    files = []
    for src, abs, rel in walk(repo, pats, opts):
        files.append(abs)
    dodiff(sys.stdout, ui, repo, files, *revs)

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
    fp.write("# Node ID %s\n" % hg.hex(node))
    fp.write("# Parent  %s\n" % hg.hex(prev))
    if other != hg.nullid:
        fp.write("# Parent  %s\n" % hg.hex(other))
    fp.write(change[4].rstrip())
    fp.write("\n\n")

    dodiff(fp, ui, repo, None, prev, node)
    if fp != sys.stdout: fp.close()

def export(ui, repo, *changesets, **opts):
    """dump the header and diffs for one or more changesets"""
    if not changesets:
        raise Abort("export requires at least one changeset")
    seqno = 0
    revs = list(revrange(ui, repo, changesets))
    total = len(revs)
    revwidth = max(len(revs[0]), len(revs[-1]))
    ui.note(len(revs) > 1 and "Exporting patches:\n" or "Exporting patch:\n")
    for cset in revs:
        seqno += 1
        doexport(ui, repo, cset, seqno, total, revwidth, opts)

def forget(ui, repo, *pats, **opts):
    """don't add the specified files on the next commit"""
    q = dict(zip(pats, pats))
    forget = []
    for src, abs, rel in walk(repo, pats, opts):
        if repo.dirstate.state(abs) == 'a':
            forget.append(abs)
            if rel not in q: ui.status('forgetting ', rel, '\n')
    repo.forget(forget)

def heads(ui, repo):
    """show current repository heads"""
    for n in repo.changelog.heads():
        show_changeset(ui, repo, changenode=n)

def identify(ui, repo):
    """print information about the working copy"""
    parents = [p for p in repo.dirstate.parents() if p != hg.nullid]
    if not parents:
        ui.write("unknown\n")
        return

    hexfunc = ui.verbose and hg.hex or hg.short
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

    d = opts["base"]
    strip = opts["strip"]

    for patch in patches:
        ui.status("applying %s\n" % patch)
        pf = os.path.join(d, patch)

        message = []
        user = None
        hgpatch = False
        for line in file(pf):
            line = line.rstrip()
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
            raise Abort("patch failed")

        if len(files) > 0:
            addremove(ui, repo, *files)
        repo.commit(files, message, user)

def init(ui, source=None):
    """create a new repository in the current directory"""

    if source:
        raise Abort("no longer supported: use \"hg clone\" instead")
    hg.repository(ui, ".", create=1)

def locate(ui, repo, *pats, **opts):
    """locate files matching specific patterns"""
    end = '\n'
    if opts['print0']: end = '\0'

    for src, abs, rel in walk(repo, pats, opts, '(?:.*/|)'):
        if repo.dirstate.state(abs) == '?': continue
        if opts['fullpath']:
            ui.write(os.path.join(repo.root, abs), end)
        else:
            ui.write(rel, end)

def log(ui, repo, f=None, **opts):
    """show the revision history of the repository or a single file"""
    if f:
        files = relpath(repo, [f])
        filelog = repo.file(files[0])
        log = filelog
        lookup = filelog.lookup
    else:
        files = None
        filelog = None
        log = repo.changelog
        lookup = repo.lookup
    revlist = []
    revs = [log.rev(lookup(rev)) for rev in opts['rev']]
    while revs:
        if len(revs) == 1:
            revlist.append(revs.pop(0))
        else:
            a = revs.pop(0)
            b = revs.pop(0)
            off = a > b and -1 or 1
            revlist.extend(range(a, b + off, off))

    for i in revlist or range(log.count() - 1, -1, -1):
        show_changeset(ui, repo, filelog=filelog, rev=i)
        if opts['patch']:
            if filelog:
                filenode = filelog.node(i)
                i = filelog.linkrev(filenode)
            changenode = repo.changelog.node(i)
            prev, other = repo.changelog.parents(changenode)
            dodiff(sys.stdout, ui, repo, files, prev, changenode)
            ui.write("\n\n")

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
        ui.write("%40s %3s %s\n" % (hg.hex(m[f]), mf[f] and "755" or "644", f))

def parents(ui, repo, rev=None):
    """show the parents of the working dir or revision"""
    if rev:
        p = repo.changelog.parents(repo.lookup(rev))
    else:
        p = repo.dirstate.parents()

    for n in p:
        if n != hg.nullid:
            show_changeset(ui, repo, changenode=n)

def paths(ui, repo, search = None):
    """show path or list of available paths"""
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

    other = hg.repository(ui, source)
    r = repo.pull(other)
    if not r:
        if opts['update']:
            return update(ui, repo)
        else:
            ui.status("(run 'hg update' to get a working copy)\n")

    return r

def push(ui, repo, dest="default-push", force=False):
    """push changes to the specified destination"""
    dest = ui.expandpath(dest)
    ui.status('pushing to %s\n' % (dest))

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

def remove(ui, repo, file1, *files):
    """remove the specified files on the next commit"""
    repo.remove(relpath(repo, (file1,) + files))

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
                respond(" ".join(map(hg.hex, h)) + "\n")
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
                nodes = map(hg.bin, nodes.split(" "))
                r = []
                for b in repo.branches(nodes):
                    r.append(" ".join(map(hg.hex, b)) + "\n")
                respond("".join(r))
            elif cmd == "between":
                arg, pairs = getarg()
                pairs = [map(hg.bin, p.split("-")) for p in pairs.split(" ")]
                r = []
                for b in repo.between(pairs):
                    r.append(" ".join(map(hg.hex, b)) + "\n")
                respond("".join(r))
            elif cmd == "changegroup":
                nodes = []
                arg, roots = getarg()
                nodes = map(hg.bin, roots.split(" "))

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

    def openlog(opt, default):
        if opts[opt] and opts[opt] != '-':
            return open(opts[opt], 'w')
        else:
            return default

    httpd = hgweb.create_server(repo.root, opts["name"], opts["templates"],
                                opts["address"], opts["port"], opts["ipv6"],
                                openlog('accesslog', sys.stdout),
                                openlog('errorlog', sys.stderr))
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
    ? = not tracked'''

    cwd = repo.getcwd()
    files, matchfn = matchpats(cwd, pats, opts)
    (c, a, d, u) = [[pathto(cwd, x) for x in n]
                    for n in repo.changes(files=files, match=matchfn)]

    for f in c:
        ui.write("M ", f, "\n")
    for f in a:
        ui.write("A ", f, "\n")
    for f in d:
        ui.write("R ", f, "\n")
    for f in u:
        ui.write("? ", f, "\n")

def tag(ui, repo, name, rev=None, **opts):
    """add a tag for the current tip or a given revision"""
    if opts['text']:
        ui.warn("Warning: -t and --text is deprecated,"
                " please use -m or --message instead.\n")
    if name == "tip":
        ui.warn("abort: 'tip' is a reserved name!\n")
        return -1
    if rev:
        r = hg.hex(repo.lookup(rev))
    else:
        r = hg.hex(repo.changelog.tip())

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
            r = "%5d:%s" % (repo.changelog.rev(n), hg.hex(n))
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

def update(ui, repo, node=None, merge=False, clean=False):
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
    node = node and repo.lookup(node) or repo.changelog.tip()
    return repo.update(node, allow=merge, force=clean)

def verify(ui, repo):
    """verify the integrity of the repository"""
    return repo.verify()

# Command options and aliases are listed here, alphabetically

table = {
    "^add": (add,
             [('I', 'include', [], 'include path in search'),
              ('X', 'exclude', [], 'exclude path from search')],
             "hg add [FILE]..."),
    "addremove": (addremove,
                  [('I', 'include', [], 'include path in search'),
                   ('X', 'exclude', [], 'exclude path from search')],
                  "hg addremove [OPTION]... [FILE]..."),
    "^annotate":
        (annotate,
         [('r', 'rev', '', 'revision'),
          ('u', 'user', None, 'show user'),
          ('n', 'number', None, 'show revision number'),
          ('c', 'changeset', None, 'show changeset'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         'hg annotate [-r REV] [-u] [-n] [-c] FILE...'),
    "cat":
        (cat,
         [('o', 'output', "", 'output to file')],
         'hg cat [-o OUTFILE] FILE [REV]'),
    "^clone":
        (clone,
         [('U', 'noupdate', None, 'skip update after cloning')],
         'hg clone [-U] SOURCE [DEST]'),
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
    "debugstate": (debugstate, [], 'debugstate'),
    "debugindex": (debugindex, [], 'debugindex FILE'),
    "debugindexdot": (debugindexdot, [], 'debugindexdot FILE'),
    "debugwalk": (debugwalk,
                  [('I', 'include', [], 'include path in search'),
                   ('X', 'exclude', [], 'exclude path from search')],
                  'debugwalk [OPTIONS]... [FILE]...'),
    "^diff":
        (diff,
         [('r', 'rev', [], 'revision'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         'hg diff [-r REV1 [-r REV2]] [FILE]...'),
    "^export":
        (export,
         [('o', 'output', "", 'output to file')],
         "hg export [-o OUTFILE] REV..."),
    "forget": (forget,
               [('I', 'include', [], 'include path in search'),
                ('X', 'exclude', [], 'exclude path from search')],
               "hg forget FILE..."),
    "heads": (heads, [], 'hg heads'),
    "help": (help_, [], 'hg help [COMMAND]'),
    "identify|id": (identify, [], 'hg identify'),
    "import|patch":
        (import_,
         [('p', 'strip', 1, 'path strip'),
          ('b', 'base', "", 'base path')],
         "hg import [-p NUM] [-b BASE] PATCH..."),
    "^init": (init, [], 'hg init'),
    "locate":
        (locate,
         [('r', 'rev', '', 'revision'),
          ('0', 'print0', None, 'end records with NUL'),
          ('f', 'fullpath', None, 'print complete paths'),
          ('I', 'include', [], 'include path in search'),
          ('X', 'exclude', [], 'exclude path from search')],
         'hg locate [-r REV] [-f] [-0] [PATTERN]...'),
    "^log|history":
        (log,
         [('r', 'rev', [], 'revision'),
          ('p', 'patch', None, 'show patch')],
         'hg log [-r REV1 [-r REV2]] [-p] [FILE]'),
    "manifest": (manifest, [], 'hg manifest [REV]'),
    "parents": (parents, [], 'hg parents [REV]'),
    "paths": (paths, [], 'hg paths [name]'),
    "^pull":
        (pull,
         [('u', 'update', None, 'update working directory')],
         'hg pull [-u] [SOURCE]'),
    "^push":
        (push,
         [('f', 'force', None, 'force push')],
         'hg push [DEST]'),
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
    "^remove|rm": (remove, [], "hg remove FILE..."),
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
          ('p', 'port', 8000, 'listen port'),
          ('a', 'address', '', 'interface address'),
          ('n', 'name', os.getcwd(), 'repository name'),
          ('', 'stdio', None, 'for remote clients'),
          ('t', 'templates', "", 'template map'),
          ('6', 'ipv6', None, 'use IPv6 in addition to IPv4')],
         "hg serve [OPTION]..."),
    "^status": (status,
                [('I', 'include', [], 'include path in search'),
                 ('X', 'exclude', [], 'exclude path from search')],
                'hg status [FILE]...'),
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
         [('m', 'merge', None, 'allow merging of conflicts'),
          ('C', 'clean', None, 'overwrite locally modified files')],
         'hg update [-m] [-C] [REV]'),
    "verify": (verify, [], 'hg verify'),
    "version": (show_version, [], 'hg version'),
    }

globalopts = [('v', 'verbose', None, 'verbose'),
              ('', 'debug', None, 'debug'),
              ('q', 'quiet', None, 'quiet'),
              ('', 'profile', None, 'profile'),
              ('R', 'repository', "", 'repository root directory'),
              ('', 'traceback', None, 'print traceback on exception'),
              ('y', 'noninteractive', None, 'run non-interactively'),
              ('', 'version', None, 'output version information and exit'),
              ('', 'time', None, 'time how long the command takes'),
             ]

norepo = "clone init version help debugindex debugindexdot"

def find(cmd):
    for e in table.keys():
        if re.match("(%s)$" % e, cmd):
            return table[e]

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

    if options["version"]:
        return ("version", show_version, [], options, cmdoptions)
    elif not args:
        return ("help", help_, [], options, cmdoptions)
    else:
        cmd, args = args[0], args[1:]

    i = find(cmd)

    # combine global options into local
    c = list(i[1])
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

    return (cmd, i[0], args, options, cmdoptions)

def dispatch(args):
    signal.signal(signal.SIGTERM, catchterm)
    try:
        signal.signal(signal.SIGHUP, catchterm)
    except AttributeError:
        pass

    try:
        cmd, func, args, options, cmdoptions = parse(args)
    except ParseError, inst:
        u = ui.ui()
        if inst.args[0]:
            u.warn("hg %s: %s\n" % (inst.args[0], inst.args[1]))
            help_(u, inst.args[0])
        else:
            u.warn("hg: %s\n" % inst.args[1])
            help_(u)
        sys.exit(-1)
    except UnknownCommand, inst:
        u = ui.ui()
        u.warn("hg: unknown command '%s'\n" % inst.args[0])
        help_(u)
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
            u = ui.ui()
            u.warn("Time: real %.3f secs (user %.3f+%.3f sys %.3f+%.3f)\n" %
                (t[4]-s[4], t[0]-s[0], t[2]-s[2], t[1]-s[1], t[3]-s[3]))
        atexit.register(print_time)

    u = ui.ui(options["verbose"], options["debug"], options["quiet"],
              not options["noninteractive"])

    try:
        try:
            if cmd not in norepo.split():
                path = options["repository"] or ""
                repo = hg.repository(ui=u, path=path)
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
    except util.CommandError, inst:
        u.warn("abort: %s\n" % inst.args)
    except hg.RepoError, inst:
        u.warn("abort: ", inst, "!\n")
    except SignalInterrupt:
        u.warn("killed!\n")
    except KeyboardInterrupt:
        u.warn("interrupted!\n")
    except IOError, inst:
        if hasattr(inst, "code"):
            u.warn("abort: %s\n" % inst)
        elif hasattr(inst, "reason"):
            u.warn("abort: error: %s\n" % inst.reason[1])
        elif hasattr(inst, "args") and inst[0] == errno.EPIPE:
            if u.debugflag: u.warn("broken pipe\n")
        else:
            raise
    except OSError, inst:
        if hasattr(inst, "filename"):
            u.warn("abort: %s: %s\n" % (inst.strerror, inst.filename))
        else:
            u.warn("abort: %s\n" % inst.strerror)
    except Abort, inst:
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

    sys.exit(-1)
