# churn.py - create a graph showing who changed the most lines
#
# Copyright 2006 Josef "Jeff" Sipek <jeffpc@josefsipek.net>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
#
# Aliases map file format is simple one alias per line in the following
# format:
#
# <alias email> <actual email>

from mercurial.demandload import *
demandload(globals(), 'time sys signal os')
demandload(globals(), 'mercurial:hg,mdiff,fancyopts,commands,ui,util,templater')

def __gather(ui, repo, node1, node2):
    def dirtywork(f, mmap1, mmap2):
        lines = 0

        to = mmap1 and repo.file(f).read(mmap1[f]) or None
        tn = mmap2 and repo.file(f).read(mmap2[f]) or None

        diff = mdiff.unidiff(to, "", tn, "", f).split("\n")

        for line in diff:
            if not line:
                continue # skip EOF
            if line.startswith(" "):
                continue # context line
            if line.startswith("--- ") or line.startswith("+++ "):
                continue # begining of diff
            if line.startswith("@@ "):
                continue # info line

            # changed lines
            lines += 1

        return lines

    ##

    lines = 0

    changes = repo.changes(node1, node2, None, util.always)

    modified, added, removed, deleted, unknown = changes

    who = repo.changelog.read(node2)[1]
    who = templater.email(who) # get the email of the person

    mmap1 = repo.manifest.read(repo.changelog.read(node1)[0])
    mmap2 = repo.manifest.read(repo.changelog.read(node2)[0])
    for f in modified:
        lines += dirtywork(f, mmap1, mmap2)

    for f in added:
        lines += dirtywork(f, None, mmap2)
        
    for f in removed:
        lines += dirtywork(f, mmap1, None)

    for f in deleted:
        lines += dirtywork(f, mmap1, mmap2)

    for f in unknown:
        lines += dirtywork(f, mmap1, mmap2)

    return (who, lines)

def gather_stats(ui, repo, amap):
    stats = {}
    
    cl    = repo.changelog

    for rev in range(0,cl.count()):
        node2    = cl.node(rev)
        node1    = cl.parents(node2)[0]

        who, lines = __gather(ui, repo, node1, node2)

        # remap the owner if possible
        if amap.has_key(who):
            ui.note("using '%s' alias for '%s'\n" % (amap[who], who))
            who = amap[who]

        if not stats.has_key(who):
            stats[who] = 0
        stats[who] += lines

        ui.note("rev %d: %d lines by %s\n" % (rev, lines, who))

    return stats

def churn(ui, repo, aliases):
    "Graphs the number of lines changed"
    
    def pad(s, l):
        if len(s) < l:
            return s + " " * (l-len(s))
        return s[0:l]

    def graph(n, maximum, width, char):
        n = int(n * width / float(maximum))
        
        return char * (n)

    def get_aliases(f):
        aliases = {}

        for l in f.readlines():
            l = l.strip()
            alias, actual = l.split(" ")
            aliases[alias] = actual

        return aliases
    
    amap = {}
    if aliases:
        try:
            f = open(aliases,"r")
        except OSError, e:
            print "Error: " + e
            return

        amap = get_aliases(f)
        f.close()
    
    stats = gather_stats(ui, repo, amap)

    # make a list of tuples (name, lines) and sort it in descending order
    ordered = stats.items()
    ordered.sort(cmp=lambda x, y: cmp(y[1], x[1]))

    maximum = ordered[0][1]

    ui.note("Assuming 80 character terminal\n")
    width = 80 - 1

    for i in ordered:
        person = i[0]
        lines = i[1]
        print "%s %6d %s" % (pad(person, 20), lines,
                graph(lines, maximum, width - 20 - 1 - 6 - 2 - 2, '*'))

cmdtable = {
    "churn":
    (churn,
     [('', 'aliases', '', 'file with email aliases')],
    'hg churn [-a file]'),
}

def reposetup(ui, repo):
    pass

