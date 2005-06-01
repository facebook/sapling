import os, re
from mercurial import fancyopts, ui, hg

class UnknownCommand(Exception): pass

def relpath(repo, args):
    if os.getcwd() != repo.root:
        p = os.getcwd()[len(repo.root) + 1: ]
        return [ os.path.join(p, x) for x in args ]
    return args
    
def help(ui, args):
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

def init(ui, args):
    """create a repository"""
    hg.repository(ui, ".", create=1)

def checkout(u, repo, args):
    node = repo.changelog.tip()
    if args:
        node = repo.lookup(args[0])
    repo.checkout(node)

def annotate(u, repo, args, **ops):
    if not args:
        return
        
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
    node = repo.current
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

table = {
    "init": (init, [], 'hg init'),
    "help": (help, [], 'hg init'),
    "checkout|co": (checkout, [], 'hg init'),
    "ann|annotate": (annotate,
                     [('r', 'revision', '', 'revision'),
                      ('u', 'user', None, 'show user'),
                      ('n', 'number', None, 'show revision number'),
                      ('c', 'changeset', None, 'show changeset')],
                     'hg annotate [-u] [-c] [-n] [-r id] [files]'),
    }

norepo = "init branch help"

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

    i = None
    for e in table.keys():
        if re.match(e + "$", cmd):
            i = table[e]

    # deal with this internally later
    if not i: raise UnknownCommand(cmd)

    cmdoptions = {}
    args = fancyopts.fancyopts(args, i[1], cmdoptions, i[2])

    if cmd not in norepo.split():
        repo = hg.repository(ui = u)
        d = lambda: i[0](u, repo, args, **cmdoptions)
    else:
        d = lambda: i[0](u, args, **cmdoptions)

    try:
        d()
    except KeyboardInterrupt:
        u.warn("interrupted!\n")
