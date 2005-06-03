import os, re, traceback, sys, signal, time
from mercurial import fancyopts, ui, hg

class UnknownCommand(Exception): pass

def filterfiles(list, files):
    l = [ x for x in list if x in files ]

    for f in files:
        if f[-1] != os.sep: f += os.sep
        l += [ x for x in list if x.startswith(f) ]
    return l

def relfilter(repo, args):
    if os.getcwd() != repo.root:
        p = os.getcwd()[len(repo.root) + 1: ]
        return filterfiles(p, args)
    return args

def relpath(repo, args):
    if os.getcwd() != repo.root:
        p = os.getcwd()[len(repo.root) + 1: ]
        return [ os.path.join(p, x) for x in args ]
    return args
    
def help(ui, cmd=None):
    '''show help'''
    if cmd:
        try:
            i = find(cmd)
            ui.write("%s\n\n" % i[2])
            ui.write(i[0].__doc__, "\n")
        except UnknownCommand:
            ui.warn("unknown command %s", cmd)
        sys.exit(0)
    
    ui.status("""\
 hg commands:

 add [files...]        add the given files in the next commit
 addremove             add all new files, delete all missing files
 annotate [files...]   show changeset number per file line
 branch <path>         create a branch of <path> in this directory
 checkout [changeset]  checkout the latest or given changeset
 commit                commit all changes to the repository
 diff [files...]       diff working directory (or selected files)
 dump <file> [rev]     dump the latest or given revision of a file
 dumpmanifest [rev]    dump the latest or given revision of the manifest
 export <rev>          dump the changeset header and diffs for a revision
 history               show changeset history
 init                  create a new repository in this directory
 log <file>            show revision history of a single file
 merge <path>          merge changes from <path> into local repository
 recover               rollback an interrupted transaction
 remove [files...]     remove the given files in the next commit
 serve                 export the repository via HTTP
 status                show new, missing, and changed files in working dir
 tags                  show current changeset tags
 undo                  undo the last transaction
""")

def init(ui):
    """create a repository"""
    hg.repository(ui, ".", create=1)

def branch(ui, path):
    '''branch from a local repository'''
    # this should eventually support remote repos
    os.system("cp -al %s/.hg .hg" % path)

def checkout(ui, repo, changeset=None):
    '''checkout a given changeset or the current tip'''
    (c, a, d, u) = repo.diffdir(repo.root, repo.current)
    if c or a or d:
        ui.warn("aborting (outstanding changes in working directory)\n")
        sys.exit(1)

    node = repo.changelog.tip()
    if changeset:
        node = repo.lookup(changeset)
    repo.checkout(node)

def annotate(u, repo, *args, **ops):
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

    args = relpath(repo, args)
    node = repo.dirstate.parents()[0]
    if ops['revision']:
        node = repo.changelog.lookup(ops['revision'])
    change = repo.changelog.read(node)
    mmap = repo.manifest.read(change[0])
    maxuserlen = 0
    maxchangelen = 0
    for f in args:
        lines = repo.file(f).annotate(mmap[f])
        pieces = []

        for o, f in opmap:
            if ops[o]:
                l = [ f(n) for n,t in lines ]
                m = max(map(len, l))
                pieces.append([ "%*s" % (m, x) for x in l])

        for p,l in zip(zip(*pieces), lines):
            u.write(" ".join(p) + ": " + l[1])

def heads(ui, repo):
    '''show current repository heads'''
    for n in repo.changelog.heads():
        i = repo.changelog.rev(n)
        changes = repo.changelog.read(n)
        (p1, p2) = repo.changelog.parents(n)
        (h, h1, h2) = map(hg.hex, (n, p1, p2))
        (i1, i2) = map(repo.changelog.rev, (p1, p2))
        print "rev:      %4d:%s" % (i, h)
        print "parents:  %4d:%s" % (i1, h1)
        if i2: print "          %4d:%s" % (i2, h2)
        print "manifest: %4d:%s" % (repo.manifest.rev(changes[0]),
                                    hg.hex(changes[0]))
        print "user:", changes[1]
        print "date:", time.asctime(
            time.localtime(float(changes[2].split(' ')[0])))
        if ui.verbose: print "files:", " ".join(changes[3])
        print "description:"
        print changes[4]

def parents(ui, repo, node = None):
    '''show the parents of the current working dir'''
    if node:
        p = repo.changelog.parents(repo.lookup(hg.bin(node)))
    else:
        p = repo.dirstate.parents()

    for n in p:
        if n != hg.nullid:
            ui.write("%d:%s\n" % (repo.changelog.rev(n), hg.hex(n)))

def status(ui, repo):
    '''show changed files in the working directory

C = changed
A = added
R = removed
? = not tracked'''
    (c, a, d, u) = repo.diffdir(repo.root, repo.current)
    (c, a, d, u) = map(lambda x: relfilter(repo, x), (c, a, d, u))

    for f in c: print "C", f
    for f in a: print "A", f
    for f in d: print "R", f
    for f in u: print "?", f

def undo(ui, repo):
    repo.undo()

table = {
    "init": (init, [], 'hg init'),
    "branch|clone": (branch, [], 'hg branch [path]'),
    "heads": (heads, [], 'hg heads'),
    "help": (help, [], 'hg help [command]'),
    "checkout|co": (checkout, [], 'hg checkout [changeset]'),
    "ann|annotate": (annotate,
                     [('r', 'revision', '', 'revision'),
                      ('u', 'user', None, 'show user'),
                      ('n', 'number', None, 'show revision number'),
                      ('c', 'changeset', None, 'show changeset')],
                     'hg annotate [-u] [-c] [-n] [-r id] [files]'),
    "parents": (parents, [], 'hg parents [node]'),
    "status": (status, [], 'hg status'),
    "undo": (undo, [], 'hg undo'),
    }

norepo = "init branch help"

def find(cmd):
    i = None
    for e in table.keys():
        if re.match(e + "$", cmd):
            return table[e]

    raise UnknownCommand(cmd)

class SignalInterrupt(Exception): pass

def catchterm(*args):
    raise SignalInterrupt

def dispatch(args):
    options = {}
    opts = [('v', 'verbose', None, 'verbose'),
            ('d', 'debug', None, 'debug'),
            ('q', 'quiet', None, 'quiet'),
            ('y', 'noninteractive', None, 'run non-interactively'),
            ]

    args = fancyopts.fancyopts(args, opts, options,
                               'hg [options] <command> [options] [files]')

    if not args:
        cmd = "help"
    else:
        cmd, args = args[0], args[1:]

    u = ui.ui(options["verbose"], options["debug"], options["quiet"],
           not options["noninteractive"])

    # deal with unfound commands later
    i = find(cmd)

    signal.signal(signal.SIGTERM, catchterm)

    cmdoptions = {}
    args = fancyopts.fancyopts(args, i[1], cmdoptions, i[2])

    if cmd not in norepo.split():
        repo = hg.repository(ui = u)
        d = lambda: i[0](u, repo, *args, **cmdoptions)
    else:
        d = lambda: i[0](u, *args, **cmdoptions)

    try:
        d()
    except SignalInterrupt:
        u.warn("killed!\n")
    except KeyboardInterrupt:
        u.warn("interrupted!\n")
    except TypeError, inst:
        # was this an argument error?
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) > 2: # no
            raise
        u.warn("%s: invalid arguments\n" % i[0].__name__)
        u.warn("syntax: %s\n" % i[2])
        sys.exit(-1)
