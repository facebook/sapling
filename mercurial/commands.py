# commands.py - command processing for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, re, sys, signal
import fancyopts, ui, hg, util
from demandload import *
demandload(globals(), "mdiff time hgweb traceback random signal errno version")

class UnknownCommand(Exception): pass

def filterfiles(filters, files):
    l = [ x for x in files if x in filters ]

    for t in filters:
        if t and t[-1] != "/": t += "/"
        l += [ x for x in files if x.startswith(t) ]
    return l

def relfilter(repo, files):
    if os.getcwd() != repo.root:
        p = os.getcwd()[len(repo.root) + 1: ]
        return filterfiles([util.pconvert(p)], files)
    return files

def relpath(repo, args):
    if os.getcwd() != repo.root:
        p = os.getcwd()[len(repo.root) + 1: ]
        return [ util.pconvert(os.path.normpath(os.path.join(p, x)))
                 for x in args ]
    return args

revrangesep = ':'

def revrange(ui, repo, revs = [], revlog = None):
    if revlog is None:
        revlog = repo.changelog
    revcount = revlog.count()
    def fix(val, defval):
        if not val: return defval
        try:
            num = int(val)
            if str(num) != val: raise ValueError
            if num < 0: num += revcount
            if not (0 <= num < revcount):
                raise ValueError
        except ValueError:
            try:
                num = repo.changelog.rev(repo.lookup(val))
            except KeyError:
                try:
                    num = revlog.rev(revlog.lookup(val))
                except KeyError:
                    ui.warn('abort: invalid revision identifier %s\n' % val)
                    sys.exit(1)
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

def dodiff(fp, ui, repo, files = None, node1 = None, node2 = None):
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
        def read(f): return repo.file(f).read(mmap2[f])
        date2 = date(change)
    else:
        date2 = time.asctime()
        if not node1:
            node1 = repo.dirstate.parents()[0]
        def read(f): return repo.wfile(f).read()

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

    parents = [(log.rev(parent), hg.hex(parent))
               for parent in log.parents(node)
               if ui.debugflag or parent != hg.nullid]
    if not ui.debugflag and len(parents) == 1 and parents[0][0] == rev-1:
        parents = []

    if filelog:
        ui.write("revision:    %d:%s\n" % (filerev, hg.hex(filenode)))
        for parent in parents:
            ui.write("parent:      %d:%s\n" % parent)
        ui.status("changeset:   %d:%s\n" % (changerev, hg.hex(changenode)))
    else:
        ui.write("changeset:   %d:%s\n" % (changerev, hg.hex(changenode)))
        for tag in repo.nodetags(changenode):
            ui.status("tag:         %s\n" % tag)
        for parent in parents:
            ui.write("parent:      %d:%s\n" % parent)
        ui.note("manifest:    %d:%s\n" % (repo.manifest.rev(changes[0]),
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

def help(ui, cmd=None):
    '''show help for a given command or all commands'''
    if cmd:
        try:
            i = find(cmd)
            ui.write("%s\n\n" % i[2])

            if i[1]:
                for s, l, d, c in i[1]:
                    opt=' '
                    if s: opt = opt + '-' + s + ' '
                    if l: opt = opt + '--' + l + ' '
                    if d: opt = opt + '(' + str(d) + ')'
                    ui.write(opt, "\n")
                    if c: ui.write('   %s\n' % c)
                ui.write("\n")

            ui.write(i[0].__doc__, "\n")
        except UnknownCommand:
            ui.warn("hg: unknown command %s\n" % cmd)
        sys.exit(0)
    else:
        if not ui.quiet:
            show_version(ui)
            ui.write('\n')
        ui.write('hg commands:\n\n')

        h = {}
        for c, e in table.items():
            f = c.split("|")[0]
            if f.startswith("debug"):
                continue
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

def add(ui, repo, file, *files):
    '''add the specified files on the next commit'''
    repo.add(relpath(repo, (file,) + files))

def addremove(ui, repo, *files):
    """add all new files, delete all missing files"""
    if files:
        files = relpath(repo, files)
        d = []
        u = []
        for f in files:
            p = repo.wjoin(f)
            s = repo.dirstate.state(f)
            isfile = os.path.isfile(p)
            if s != 'r' and not isfile:
                d.append(f)
            elif s not in 'nmai' and isfile:
                u.append(f)
    else:
        (c, a, d, u) = repo.changes(None, None)
    repo.add(u)
    repo.remove(d)

def annotate(u, repo, file, *files, **ops):
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

    bcache = {}
    opmap = [['user', getname], ['number', str], ['changeset', getnode]]
    if not ops['user'] and not ops['changeset']:
        ops['number'] = 1

    node = repo.dirstate.parents()[0]
    if ops['revision']:
        node = repo.changelog.lookup(ops['revision'])
    change = repo.changelog.read(node)
    mmap = repo.manifest.read(change[0])
    for f in relpath(repo, (file,) + files):
        lines = repo.file(f).annotate(mmap[f])
        pieces = []

        for o, f in opmap:
            if ops[o]:
                l = [ f(n) for n,t in lines ]
                m = max(map(len, l))
                pieces.append([ "%*s" % (m, x) for x in l])

        for p,l in zip(zip(*pieces), lines):
            u.write(" ".join(p) + ": " + l[1])

def cat(ui, repo, file, rev = []):
    """output the latest or given revision of a file"""
    r = repo.file(relpath(repo, [file])[0])
    n = r.tip()
    if rev: n = r.lookup(rev)
    sys.stdout.write(r.read(n))

def clone(ui, source, dest = None, **opts):
    """make a copy of an existing repository"""
    source = ui.expandpath(source)

    if dest is None:
        dest = os.path.basename(os.path.normpath(source))

    if os.path.exists(dest):
        ui.warn("abort: destination '%s' already exists\n" % dest)
        return 1

    class dircleanup:
        def __init__(self, dir):
            self.dir = dir
            os.mkdir(dir)
        def close(self):
            self.dir = None
        def __del__(self):
            if self.dir:
                import shutil
                shutil.rmtree(self.dir, True)

    d = dircleanup(dest)

    link = 0
    abspath = source
    if not (source.startswith("http://") or
            source.startswith("hg://") or
            source.startswith("old-http://")):
        abspath = os.path.abspath(source)
        d1 = os.stat(dest).st_dev
        d2 = os.stat(source).st_dev
        if d1 == d2: link = 1

    if link:
        ui.note("copying by hardlink\n")
        util.system("cp -al '%s'/.hg '%s'/.hg" % (source, dest))
        try:
            os.remove(os.path.join(dest, ".hg", "dirstate"))
        except: pass

        repo = hg.repository(ui, dest)

    else:
        repo = hg.repository(ui, dest, create=1)
        other = hg.repository(ui, source)
        fetch = repo.findincoming(other)
        if fetch:
            cg = other.changegroup(fetch)
            repo.addchangegroup(cg)

    f = repo.opener("hgrc", "w")
    f.write("[paths]\n")
    f.write("default = %s\n" % abspath)

    if not opts['noupdate']:
        update(ui, repo)

    d.close()

def commit(ui, repo, *files, **opts):
    """commit the specified files or all outstanding changes"""
    text = opts['text']
    if not text and opts['logfile']:
        try: text = open(opts['logfile']).read()
        except IOError: pass

    if opts['addremove']:
        addremove(ui, repo, *files)
    repo.commit(relpath(repo, files), text, opts['user'], opts['date'])

def copy(ui, repo, source, dest):
    """mark a file as copied or renamed for the next commit"""
    return repo.copy(*relpath(repo, (source, dest)))

def debugcheckdirstate(ui, repo):
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
        ui.warn(".hg/dirstate inconsistent with current parent's manifest\n")
        sys.exit(1)

def debugdumpdirstate(ui, repo):
    repo.dirstate.read()
    dc = repo.dirstate.map
    keys = dc.keys()
    keys.sort()
    for file in keys:
        ui.write("%c %s\n" % (dc[file][0], file))

def debugindex(ui, file):
    r = hg.revlog(hg.opener(""), file, "")
    ui.write("   rev    offset  length   base linkrev" +
             " p1           p2           nodeid\n")
    for i in range(r.count()):
        e = r.index[i]
        ui.write("% 6d % 9d % 7d % 6d % 7d %s.. %s.. %s..\n" % (
                i, e[0], e[1], e[2], e[3],
            hg.hex(e[4][:5]), hg.hex(e[5][:5]), hg.hex(e[6][:5])))

def debugindexdot(ui, file):
    r = hg.revlog(hg.opener(""), file, "")
    ui.write("digraph G {\n")
    for i in range(r.count()):
        e = r.index[i]
        ui.write("\t%d -> %d\n" % (r.rev(e[4]), i))
        if e[5] != hg.nullid:
            ui.write("\t%d -> %d\n" % (r.rev(e[5]), i))
    ui.write("}\n")

def diff(ui, repo, *files, **opts):
    """diff working directory (or selected files)"""
    revs = []
    if opts['rev']:
        revs = map(lambda x: repo.lookup(x), opts['rev'])

    if len(revs) > 2:
        ui.warn("too many revisions to diff\n")
        sys.exit(1)

    if files:
        files = relpath(repo, files)
    else:
        files = relpath(repo, [""])

    dodiff(sys.stdout, ui, repo, files, *revs)

def doexport(ui, repo, changeset, seqno, total, revwidth, opts):
    node = repo.lookup(changeset)
    prev, other = repo.changelog.parents(node)
    change = repo.changelog.read(node)

    def expand(name):
        expansions = {
            '%': lambda: '%',
            'H': lambda: hg.hex(node),
            'N': lambda: str(total),
            'R': lambda: str(repo.changelog.rev(node)),
            'b': lambda: os.path.basename(repo.root),
            'h': lambda: hg.short(node),
            'n': lambda: str(seqno).zfill(len(str(total))),
            'r': lambda: str(repo.changelog.rev(node)).zfill(revwidth),
            }
        newname = []
        namelen = len(name)
        i = 0
        while i < namelen:
            c = name[i]
            if c == '%':
                i += 1
                c = name[i]
                c = expansions[c]()
            newname.append(c)
            i += 1
        return ''.join(newname)

    if opts['output'] and opts['output'] != '-':
        try:
            fp = open(expand(opts['output']), 'w')
        except KeyError, inst:
            ui.warn("error: invalid format spec '%%%s' in output file name\n" %
                    inst.args[0])
            sys.exit(1)
    else:
        fp = sys.stdout

    fp.write("# HG changeset patch\n")
    fp.write("# User %s\n" % change[1])
    fp.write("# Node ID %s\n" % hg.hex(node))
    fp.write("# Parent  %s\n" % hg.hex(prev))
    if other != hg.nullid:
        fp.write("# Parent  %s\n" % hg.hex(other))
    fp.write(change[4].rstrip())
    fp.write("\n\n")

    dodiff(fp, ui, repo, None, prev, node)

def export(ui, repo, *changesets, **opts):
    """dump the header and diffs for one or more changesets"""
    seqno = 0
    revs = list(revrange(ui, repo, changesets))
    total = len(revs)
    revwidth = max(len(revs[0]), len(revs[-1]))
    for cset in revs:
        seqno += 1
        doexport(ui, repo, cset, seqno, total, revwidth, opts)

def forget(ui, repo, file, *files):
    """don't add the specified files on the next commit"""
    repo.forget(relpath(repo, (file,) + files))

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
    (c, a, d, u) = repo.changes(None, None)
    output = ["%s%s" % ('+'.join([hexfunc(parent) for parent in parents]),
                        (c or a or d) and "+" or "")]

    if not ui.quiet:
        # multiple tags for a single parent separated by '/'
        parenttags = ['/'.join(tags)
                      for tags in map(repo.nodetags, parents) if tags]
        # tags for multiple parents separated by ' + '
        output.append(' + '.join(parenttags))

    ui.write("%s\n" % ' '.join(output))

def import_(ui, repo, patch1, *patches, **opts):
    """import an ordered set of patches"""
    try:
        import psyco
        psyco.full()
    except:
        pass

    patches = (patch1,) + patches

    d = opts["base"]
    strip = opts["strip"]

    for patch in patches:
        ui.status("applying %s\n" % patch)
        pf = os.path.join(d, patch)

        text = ""
        for l in file(pf):
            if l[:4] == "--- ": break
            text += l

        # make sure text isn't empty
        if not text: text = "imported patch %s\n" % patch

        f = os.popen("patch -p%d < %s" % (strip, pf))
        files = []
        for l in f.read().splitlines():
            l.rstrip('\r\n');
            ui.status("%s\n" % l)
            if l[:14] == 'patching file ':
                pf = l[14:]
                if pf not in files:
                    files.append(pf)
        patcherr = f.close()
        if patcherr:
            sys.stderr.write("patch failed")
            sys.exit(1)

        if len(files) > 0:
            addremove(ui, repo, *files)
        repo.commit(files, text)

def init(ui, source=None):
    """create a new repository in the current directory"""

    if source:
        ui.warn("no longer supported: use \"hg clone\" instead\n")
        sys.exit(1)
    repo = hg.repository(ui, ".", create=1)

def log(ui, repo, f=None, **opts):
    """show the revision history of the repository or a single file"""
    if f:
        filelog = repo.file(relpath(repo, [f])[0])
        log = filelog
        lookup = filelog.lookup
    else:
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

def manifest(ui, repo, rev = []):
    """output the latest or given revision of the project manifest"""
    n = repo.manifest.tip()
    if rev:
        n = repo.manifest.lookup(rev)
    m = repo.manifest.read(n)
    mf = repo.manifest.readflags(n)
    files = m.keys()
    files.sort()

    for f in files:
        ui.write("%40s %3s %s\n" % (hg.hex(m[f]), mf[f] and "755" or "644", f))

def parents(ui, repo, node = None):
    '''show the parents of the current working dir'''
    if node:
        p = repo.changelog.parents(repo.lookup(hg.bin(node)))
    else:
        p = repo.dirstate.parents()

    for n in p:
        if n != hg.nullid:
            show_changeset(ui, repo, changenode=n)

def pull(ui, repo, source="default", **opts):
    """pull changes from the specified source"""
    source = ui.expandpath(source)

    ui.status('pulling from %s\n' % (source))

    other = hg.repository(ui, source)
    fetch = repo.findincoming(other)
    if not fetch:
        ui.status("no changes found\n")
        return

    cg = other.changegroup(fetch)
    r = repo.addchangegroup(cg)
    if cg and not r:
        if opts['update']:
            return update(ui, repo)
        else:
            ui.status("(run 'hg update' to get a working copy)\n")

    return r

def push(ui, repo, dest="default-push"):
    """push changes to the specified destination"""
    dest = ui.expandpath(dest)

    if not dest.startswith("ssh://"):
        ui.warn("abort: can only push to ssh:// destinations currently\n")
        return 1

    m = re.match(r'ssh://(([^@]+)@)?([^:/]+)(:(\d+))?(/(.*))?', dest)
    if not m:
        ui.warn("abort: couldn't parse destination %s\n" % dest)
        return 1

    user, host, port, path = map(m.group, (2, 3, 5, 7))
    uhost = user and ("%s@%s" % (user, host)) or host
    port = port and (" -p %s") % port or ""
    path = path or ""

    sport = random.randrange(30000, 60000)
    cmd = "ssh %s%s -R %d:localhost:%d 'cd %s; hg pull http://localhost:%d/'"
    cmd = cmd % (uhost, port, sport+1, sport, path, sport+1)

    child = os.fork()
    if not child:
        sys.stdout = file("/dev/null", "w")
        sys.stderr = sys.stdout
        hgweb.server(repo.root, "pull", "", "localhost", sport)
    else:
        ui.status("connecting to %s\n" % host)
        r = os.system(cmd)
        os.kill(child, signal.SIGTERM)
        return r

def rawcommit(ui, repo, *flist, **rc):
    "raw commit interface"

    text = rc['text']
    if not text and rc['logfile']:
        try: text = open(rc['logfile']).read()
        except IOError: pass
    if not text and not rc['logfile']:
        ui.warn("abort: missing commit text\n")
        return 1

    files = relpath(repo, list(flist))
    if rc['files']:
        files += open(rc['files']).read().splitlines()

    rc['parent'] = map(repo.lookup, rc['parent'])

    repo.rawcommit(files, text, rc['user'], rc['date'], *rc['parent'])

def recover(ui, repo):
    """roll back an interrupted transaction"""
    repo.recover()

def remove(ui, repo, file, *files):
    """remove the specified files on the next commit"""
    repo.remove(relpath(repo, (file,) + files))

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
                if not name.startswith(r): continue
                rest = name[len(r):]
                if not rest: return r, True
                depth = rest.count(os.sep)
                if not r:
                    if depth == 0 or not opts['nonrecursive']: return r, True
                elif rest[0] == os.sep:
                    if depth == 1 or not opts['nonrecursive']: return r, True
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
    hgweb.server(repo.root, opts["name"], opts["templates"],
                 opts["address"], opts["port"])

def status(ui, repo):
    '''show changed files in the working directory

    C = changed
    A = added
    R = removed
    ? = not tracked'''

    (c, a, d, u) = repo.changes(None, None)
    (c, a, d, u) = map(lambda x: relfilter(repo, x), (c, a, d, u))

    for f in c: ui.write("C ", f, "\n")
    for f in a: ui.write("A ", f, "\n")
    for f in d: ui.write("R ", f, "\n")
    for f in u: ui.write("? ", f, "\n")

def tag(ui, repo, name, rev = None, **opts):
    """add a tag for the current tip or a given revision"""

    if name == "tip":
        ui.warn("abort: 'tip' is a reserved name!\n")
        return -1
    if name.find(revrangesep) >= 0:
        ui.warn("abort: '%s' cannot be used in a tag name\n" % revrangesep)
        return -1

    (c, a, d, u) = repo.changes(None, None)
    for x in (c, a, d, u):
        if ".hgtags" in x:
            ui.warn("abort: working copy of .hgtags is changed!\n")
            ui.status("(please commit .hgtags manually)\n")
            return -1

    if rev:
        r = hg.hex(repo.lookup(rev))
    else:
        r = hg.hex(repo.changelog.tip())

    add = 0
    if not os.path.exists(repo.wjoin(".hgtags")): add = 1
    repo.wfile(".hgtags", "a").write("%s %s\n" % (r, name))
    if add: repo.add([".hgtags"])

    if not opts['text']:
        opts['text'] = "Added tag %s for changeset %s" % (name, r)

    repo.commit([".hgtags"], opts['text'], opts['user'], opts['date'])

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
    """undo the last transaction"""
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
    "add": (add, [], "hg add [files]"),
    "addremove": (addremove, [], "hg addremove [files]"),
    "annotate": (annotate,
                     [('r', 'revision', '', 'revision'),
                      ('u', 'user', None, 'show user'),
                      ('n', 'number', None, 'show revision number'),
                      ('c', 'changeset', None, 'show changeset')],
                     'hg annotate [-u] [-c] [-n] [-r id] [files]'),
    "cat": (cat, [], 'hg cat <file> [rev]'),
    "clone": (clone, [('U', 'noupdate', None, 'skip update after cloning')],
              'hg clone [options] <source> [dest]'),
    "commit|ci": (commit,
                  [('t', 'text', "", 'commit text'),
                   ('A', 'addremove', None, 'run add/remove during commit'),
                   ('l', 'logfile', "", 'commit text file'),
                   ('d', 'date', "", 'data'),
                   ('u', 'user', "", 'user')],
                  'hg commit [files]'),
    "copy": (copy, [], 'hg copy <source> <dest>'),
    "debugcheckdirstate": (debugcheckdirstate, [], 'debugcheckdirstate'),
    "debugdumpdirstate": (debugdumpdirstate, [], 'debugdumpdirstate'),
    "debugindex": (debugindex, [], 'debugindex <file>'),
    "debugindexdot": (debugindexdot, [], 'debugindexdot <file>'),
    "diff": (diff, [('r', 'rev', [], 'revision')],
             'hg diff [-r A] [-r B] [files]'),
    "export": (export, [('o', 'output', "", 'output to file')],
               "hg export [-o file] <changeset> ..."),
    "forget": (forget, [], "hg forget [files]"),
    "heads": (heads, [], 'hg heads'),
    "help": (help, [], 'hg help [command]'),
    "identify|id": (identify, [], 'hg identify'),
    "import|patch": (import_,
                     [('p', 'strip', 1, 'path strip'),
                      ('b', 'base', "", 'base path')],
                     "hg import [options] <patches>"),
    "init": (init, [], 'hg init'),
    "log|history": (log,
                    [('r', 'rev', [], 'revision')],
                    'hg log [-r A] [-r B] [file]'),
    "manifest": (manifest, [], 'hg manifest [rev]'),
    "parents": (parents, [], 'hg parents [node]'),
    "pull": (pull,
                  [('u', 'update', None, 'update working directory')],
                  'hg pull [options] [source]'),
    "push": (push, [], 'hg push <destination>'),
    "rawcommit": (rawcommit,
                  [('p', 'parent', [], 'parent'),
                   ('d', 'date', "", 'data'),
                   ('u', 'user', "", 'user'),
                   ('F', 'files', "", 'file list'),
                   ('t', 'text', "", 'commit text'),
                   ('l', 'logfile', "", 'commit text file')],
                  'hg rawcommit [options] [files]'),
    "recover": (recover, [], "hg recover"),
    "remove|rm": (remove, [], "hg remove [files]"),
    "revert": (revert,
               [("n", "nonrecursive", None, "don't recurse into subdirs"),
                ("r", "rev", "", "revision")],
               "hg revert [files|dirs]"),
    "root": (root, [], "hg root"),
    "serve": (serve, [('p', 'port', 8000, 'listen port'),
                      ('a', 'address', '', 'interface address'),
                      ('n', 'name', os.getcwd(), 'repository name'),
                      ('t', 'templates', "", 'template map')],
              "hg serve [options]"),
    "status": (status, [], 'hg status'),
    "tag": (tag,  [('t', 'text', "", 'commit text'),
                   ('d', 'date', "", 'date'),
                   ('u', 'user', "", 'user')],
            'hg tag [options] <name> [rev]'),
    "tags": (tags, [], 'hg tags'),
    "tip": (tip, [], 'hg tip'),
    "undo": (undo, [], 'hg undo'),
    "update|up|checkout|co":
            (update,
             [('m', 'merge', None, 'allow merging of conflicts'),
              ('C', 'clean', None, 'overwrite locally modified files')],
             'hg update [options] [node]'),
    "verify": (verify, [], 'hg verify'),
    "version": (show_version, [], 'hg version'),
    }

norepo = "clone init version help debugindex debugindexdot"

def find(cmd):
    for e in table.keys():
        if re.match("(%s)$" % e, cmd):
            return table[e]

    raise UnknownCommand(cmd)

class SignalInterrupt(Exception): pass

def catchterm(*args):
    raise SignalInterrupt

def run():
    sys.exit(dispatch(sys.argv[1:]))

def dispatch(args):
    options = {}
    opts = [('v', 'verbose', None, 'verbose'),
            ('d', 'debug', None, 'debug'),
            ('q', 'quiet', None, 'quiet'),
            ('', 'profile', None, 'profile'),
            ('R', 'repository', "", 'repository root directory'),
            ('', 'traceback', None, 'print traceback on exception'),
            ('y', 'noninteractive', None, 'run non-interactively'),
            ('', 'version', None, 'output version information and exit'),
            ]

    args = fancyopts.fancyopts(args, opts, options,
                               'hg [options] <command> [options] [files]')

    if not args:
        cmd = "help"
    else:
        cmd, args = args[0], args[1:]

    u = ui.ui(options["verbose"], options["debug"], options["quiet"],
           not options["noninteractive"])

    if options["version"]:
        show_version(u)
        sys.exit(0)

    try:
        i = find(cmd)
    except UnknownCommand:
        u.warn("hg: unknown command '%s'\n" % cmd)
        help(u)
        sys.exit(1)

    signal.signal(signal.SIGTERM, catchterm)

    cmdoptions = {}
    try:
        args = fancyopts.fancyopts(args, i[1], cmdoptions, i[2])
    except fancyopts.getopt.GetoptError, inst:
        u.warn("hg %s: %s\n" % (cmd, inst))
        help(u, cmd)
        sys.exit(-1)

    try:
        try:
            if cmd not in norepo.split():
                path = options["repository"] or ""
                repo = hg.repository(ui=u, path=path)
                d = lambda: i[0](u, repo, *args, **cmdoptions)
            else:
                d = lambda: i[0](u, *args, **cmdoptions)

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
            u.warn("abort: error %d: %s\n" % (inst.reason[0], inst.reason[1]))
        elif hasattr(inst, "args") and inst[0] == errno.EPIPE:
            u.warn("broken pipe\n")
        else:
            raise
    except OSError, inst:
        if hasattr(inst, "filename"):
            u.warn("abort: %s: %s\n" % (inst.strerror, inst.filename))
        else:
            u.warn("abort: %s\n" % inst.strerror)
    except TypeError, inst:
        # was this an argument error?
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) > 2: # no
            raise
        u.debug(inst, "\n")
        u.warn("%s: invalid arguments\n" % i[0].__name__)
        help(u, cmd)

    sys.exit(-1)
