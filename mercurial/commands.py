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
        return [ util.pconvert(os.path.normpath(os.path.join(p, x))) for x in args ]
    return args

def dodiff(ui, repo, path, files = None, node1 = None, node2 = None):
    def date(c):
        return time.asctime(time.gmtime(float(c[2].split(' ')[0])))

    if node2:
        change = repo.changelog.read(node2)
        mmap2 = repo.manifest.read(change[0])
        (c, a, d) = repo.diffrevs(node1, node2)
        def read(f): return repo.file(f).read(mmap2[f])
        date2 = date(change)
    else:
        date2 = time.asctime()
        (c, a, d, u) = repo.diffdir(path, node1)
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

    if files:
        c, a, d = map(lambda x: filterfiles(files, x), (c, a, d))

    for f in c:
        to = None
        if f in mmap:
            to = repo.file(f).read(mmap[f])
        tn = read(f)
        sys.stdout.write(mdiff.unidiff(to, date1, tn, date2, f, r))
    for f in a:
        to = None
        tn = read(f)
        sys.stdout.write(mdiff.unidiff(to, date1, tn, date2, f, r))
    for f in d:
        to = repo.file(f).read(mmap[f])
        tn = None
        sys.stdout.write(mdiff.unidiff(to, date1, tn, date2, f, r))

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
        files = repo.diffrevs(changelog.parents(changenode)[0], changenode)
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
            ui.status("\n")
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
        (c, a, d, u) = repo.diffdir(repo.root)
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

    success = created = False

    if dest is None:
        dest = os.getcwd()
    elif not os.path.exists(dest):
        os.mkdir(dest)
        created = True

    try:
        dest = os.path.realpath(dest)

        link = 0
        if not source.startswith("http://"):
            source = os.path.realpath(source)
            d1 = os.stat(dest).st_dev
            d2 = os.stat(source).st_dev
            if d1 == d2: link = 1

        os.chdir(dest)

        if link:
            ui.debug("copying by hardlink\n")
            util.system("cp -al %s/.hg .hg" % source)
            try:
                os.remove(".hg/dirstate")
            except: pass

            repo = hg.repository(ui, ".")

        else:
            repo = hg.repository(ui, ".", create=1)
            other = hg.repository(ui, source)
            fetch = repo.findincoming(other)
            if fetch:
                cg = other.changegroup(fetch)
                repo.addchangegroup(cg)

        f = repo.opener("hgrc", "w")
        f.write("[paths]\n")
        f.write("default = %s\n" % source)

        if not opts['noupdate']:
            update(ui, repo)

        success = True

    finally:
        if created and not success:
            import shutil
            shutil.rmtree(dest, True)

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
    dc = repo.dirstate.dup()
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
            print "%s in state %s, but not listed in manifest1" % (f, state)
            errors += 1
        if state in "a" and f in m1:
            print "%s in state %s, but also listed in manifest1" % (f, state)
            errors += 1
        if state in "m" and f not in m1 and f not in m2:
            print "%s in state %s, but not listed in either manifest" % (f, state)
            errors += 1
    for f in m1:
        state = repo.dirstate.state(f)
        if state not in "nrm":
            print "%s in manifest1, but listed as state %s" % (f, state)
            errors += 1
    if errors:
        print ".hg/dirstate inconsistent with current parent's manifest, aborting"
        sys.exit(1)

def debugdumpdirstate(ui, repo):
    dc = repo.dirstate.dup()
    keys = dc.keys()
    keys.sort()
    for file in keys:
        print "%s => %c" % (file, dc[file][0])

def debugindex(ui, file):
    r = hg.revlog(hg.opener(""), file, "")
    print "   rev    offset  length   base linkrev"+\
          " p1           p2           nodeid"
    for i in range(r.count()):
        e = r.index[i]
        print "% 6d % 9d % 7d % 6d % 7d %s.. %s.. %s.." % (
            i, e[0], e[1], e[2], e[3],
            hg.hex(e[4][:5]), hg.hex(e[5][:5]), hg.hex(e[6][:5]))

def debugindexdot(ui, file):
    r = hg.revlog(hg.opener(""), file, "")
    print "digraph G {"
    for i in range(r.count()):
        e = r.index[i]
        print "\t%d -> %d" % (r.rev(e[4]), i)
        if e[5] != hg.nullid:
            print "\t%d -> %d" % (r.rev(e[5]), i)
    print "}"

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

    dodiff(ui, repo, os.getcwd(), files, *revs)

def export(ui, repo, changeset):
    """dump the changeset header and diffs for a revision"""
    node = repo.lookup(changeset)
    prev, other = repo.changelog.parents(node)
    change = repo.changelog.read(node)
    print "# HG changeset patch"
    print "# User %s" % change[1]
    print "# Node ID %s" % hg.hex(node)
    print "# Parent  %s" % hg.hex(prev)
    print
    if other != hg.nullid:
        print "# Parent  %s" % hg.hex(other)
    print change[4].rstrip()
    print

    dodiff(ui, repo, "", None, prev, node)

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
    (c, a, d, u) = repo.diffdir(repo.root)
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

def log(ui, repo, f = None):
    """show the revision history of the repository or a single file"""
    if f:
        f = relpath(repo, [f])[0]
        r = repo.file(f)
        for i in range(r.count() - 1, -1, -1):
            show_changeset(ui, repo, filelog=r, rev=i)
    else:
        for i in range(repo.changelog.count() - 1, -1, -1):
            show_changeset(ui, repo, rev=i)

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
    host = user and ("%s@%s" % (user, host)) or host
    port = port and (" -p %s") % port or ""
    path = path or ""

    sport = random.randrange(30000, 60000)
    cmd = "ssh %s%s -R %d:localhost:%d 'cd %s; hg pull http://localhost:%d/'"
    cmd = cmd % (host, port, sport+1, sport, path, sport+1)

    child = os.fork()
    if not child:
        sys.stdout = file("/dev/null", "w")
        sys.stderr = sys.stdout
        hgweb.server(repo.root, "pull", "", "localhost", sport)
    else:
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
        print "missing commit text"
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

    (c, a, d, u) = repo.diffdir(os.getcwd())
    (c, a, d, u) = map(lambda x: relfilter(repo, x), (c, a, d, u))

    for f in c: print "C", f
    for f in a: print "A", f
    for f in d: print "R", f
    for f in u: print "?", f

def tag(ui, repo, name, rev = None, **opts):
    """add a tag for the current tip or a given revision"""

    if name == "tip":
	ui.warn("abort: 'tip' is a reserved name!\n")
	return -1

    (c, a, d, u) = repo.diffdir(repo.root)
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
    "export": (export, [], "hg export <changeset>"),
    "forget": (forget, [], "hg forget [files]"),
    "heads": (heads, [], 'hg heads'),
    "help": (help, [], 'hg help [command]'),
    "identify|id": (identify, [], 'hg identify'),
    "import|patch": (import_,
                     [('p', 'strip', 1, 'path strip'),
                      ('b', 'base', "", 'base path')],
                     "hg import [options] <patches>"),
    "init": (init, [], 'hg init'),
    "log|history": (log, [], 'hg log [file]'),
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
            ('p', 'profile', None, 'profile'),
            ('R', 'repository', "", 'repository root directory'),
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
    except TypeError, inst:
        # was this an argument error?
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) > 2: # no
            raise
        u.debug(inst, "\n")
        u.warn("%s: invalid arguments\n" % i[0].__name__)
        help(u, cmd)

    sys.exit(-1)

