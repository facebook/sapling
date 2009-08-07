# perf.py - performance test routines
'''helper extension to measure performance'''

from mercurial import cmdutil, match, commands
import time, os, sys

def timer(func):
    results = []
    begin = time.time()
    count = 0
    while 1:
        ostart = os.times()
        cstart = time.time()
        r = func()
        cstop = time.time()
        ostop = os.times()
        count += 1
        a, b = ostart, ostop
        results.append((cstop - cstart, b[0] - a[0], b[1]-a[1]))
        if cstop - begin > 3 and count >= 100:
            break
        if cstop - begin > 10 and count >= 3:
            break
    if r:
        sys.stderr.write("! result: %s\n" % r)
    m = min(results)
    sys.stderr.write("! wall %f comb %f user %f sys %f (best of %d)\n"
                     % (m[0], m[1] + m[2], m[1], m[2], count))

def perfwalk(ui, repo, *pats):
    try:
        m = cmdutil.match(repo, pats, {})
        timer(lambda: len(list(repo.dirstate.walk(m, True, False))))
    except:
        try:
            m = cmdutil.match(repo, pats, {})
            timer(lambda: len([b for a,b,c in repo.dirstate.statwalk([], m)]))
        except:
            timer(lambda: len(list(cmdutil.walk(repo, pats, {}))))

def perfstatus(ui, repo, *pats):
    #m = match.always(repo.root, repo.getcwd())
    #timer(lambda: sum(map(len, repo.dirstate.status(m, False, False, False))))
    timer(lambda: sum(map(len, repo.status())))

def perfheads(ui, repo):
    timer(lambda: len(repo.changelog.heads()))

def perftags(ui, repo):
    import mercurial.changelog, mercurial.manifest
    def t():
        repo.changelog = mercurial.changelog.changelog(repo.sopener)
        repo.manifest = mercurial.manifest.manifest(repo.sopener)
        repo._tags = None
        return len(repo.tags())
    timer(t)

def perfdirstate(ui, repo):
    "a" in repo.dirstate
    def d():
        repo.dirstate.invalidate()
        "a" in repo.dirstate
    timer(d)

def perfdirstatedirs(ui, repo):
    "a" in repo.dirstate
    def d():
        "a" in repo.dirstate._dirs
        del repo.dirstate._dirs
    timer(d)

def perfmanifest(ui, repo):
    def d():
        t = repo.manifest.tip()
        m = repo.manifest.read(t)
        repo.manifest.mapcache = None
        repo.manifest._cache = None
    timer(d)

def perfindex(ui, repo):
    import mercurial.changelog
    def d():
        t = repo.changelog.tip()
        repo.changelog = mercurial.changelog.changelog(repo.sopener)
        repo.changelog._loadindexmap()
    timer(d)

def perfstartup(ui, repo):
    cmd = sys.argv[0]
    def d():
        os.system("HGRCPATH= %s version -q > /dev/null" % cmd)
    timer(d)

def perfparents(ui, repo):
    nl = [repo.changelog.node(i) for i in xrange(1000)]
    def d():
        for n in nl:
            repo.changelog.parents(n)
    timer(d)

def perflookup(ui, repo, rev):
    timer(lambda: len(repo.lookup(rev)))

def perflog(ui, repo):
    ui.pushbuffer()
    timer(lambda: commands.log(ui, repo, rev=[], date='', user=''))
    ui.popbuffer()

def perftemplating(ui, repo):
    ui.pushbuffer()
    timer(lambda: commands.log(ui, repo, rev=[], date='', user='',
                               template='{date|shortdate} [{rev}:{node|short}]'
                               ' {author|person}: {desc|firstline}\n'))
    ui.popbuffer()

cmdtable = {
    'perflookup': (perflookup, []),
    'perfparents': (perfparents, []),
    'perfstartup': (perfstartup, []),
    'perfstatus': (perfstatus, []),
    'perfwalk': (perfwalk, []),
    'perfmanifest': (perfmanifest, []),
    'perfindex': (perfindex, []),
    'perfheads': (perfheads, []),
    'perftags': (perftags, []),
    'perfdirstate': (perfdirstate, []),
    'perfdirstatedirs': (perfdirstate, []),
    'perflog': (perflog, []),
    'perftemplating': (perftemplating, []),
}

