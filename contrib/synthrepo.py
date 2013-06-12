# synthrepo.py - repo synthesis
#
# Copyright 2012 Facebook
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''synthesize structurally interesting change history

This extension is useful for creating a repository with properties
that are statistically similar to an existing repository. During
analysis, a simple probability table is constructed from the history
of an existing repository.  During synthesis, these properties are
reconstructed.

Properties that are analyzed and synthesized include the following:

- Lines added or removed when an existing file is modified
- Number and sizes of files added
- Number of files removed
- Line lengths
- Topological distance to parent changeset(s)
- Probability of a commit being a merge
- Probability of a newly added file being added to a new directory
- Interarrival time, and time zone, of commits

A few obvious properties that are not currently handled realistically:

- Merges are treated as regular commits with two parents, which is not
  realistic
- Modifications are not treated as operations on hunks of lines, but
  as insertions and deletions of randomly chosen single lines
- Committer ID (always random)
- Executability of files
- Symlinks and binary files are ignored
'''

import bisect, collections, json, os, random, time, sys
from mercurial import cmdutil, context, patch, scmutil, util, hg
from mercurial.i18n import _
from mercurial.node import nullrev, nullid

testedwith = 'internal'

cmdtable = {}
command = cmdutil.command(cmdtable)

newfile = set(('new fi', 'rename', 'copy f', 'copy t'))

def zerodict():
    return collections.defaultdict(lambda: 0)

def roundto(x, k):
    if x > k * 2:
        return int(round(x / float(k)) * k)
    return int(round(x))

def parsegitdiff(lines):
    filename, mar, lineadd, lineremove = None, None, zerodict(), 0
    binary = False
    for line in lines:
        start = line[:6]
        if start == 'diff -':
            if filename:
                yield filename, mar, lineadd, lineremove, binary
            mar, lineadd, lineremove, binary = 'm', zerodict(), 0, False
            filename = patch.gitre.match(line).group(1)
        elif start in newfile:
            mar = 'a'
        elif start == 'GIT bi':
            binary = True
        elif start == 'delete':
            mar = 'r'
        elif start:
            s = start[0]
            if s == '-' and not line.startswith('--- '):
                lineremove += 1
            elif s == '+' and not line.startswith('+++ '):
                lineadd[roundto(len(line) - 1, 5)] += 1
    if filename:
        yield filename, mar, lineadd, lineremove, binary

@command('analyze',
         [('o', 'output', [], _('write output to given file'), _('FILE')),
          ('r', 'rev', [], _('analyze specified revisions'), _('REV'))],
         _('hg analyze'))
def analyze(ui, repo, *revs, **opts):
    '''create a simple model of a repository to use for later synthesis

    This command examines every changeset in the given range (or all
    of history if none are specified) and creates a simple statistical
    model of the history of the repository.

    The model is written out to a JSON file, and can be used by
    :hg:`synthesize` to create or augment a repository with synthetic
    commits that have a structure that is statistically similar to the
    analyzed repository.
    '''

    revs = list(revs)
    revs.extend(opts['rev'])
    if not revs:
        revs = [':']

    output = opts['output']
    if not output:
        output = os.path.basename(repo.root) + '.json'

    if output == '-':
        fp = sys.stdout
    else:
        fp = open(output, 'w')

    revs = scmutil.revrange(repo, revs)
    revs.sort()

    lineschanged = zerodict()
    children = zerodict()
    p1distance = zerodict()
    p2distance = zerodict()
    linesinfilesadded = zerodict()
    fileschanged = zerodict()
    filesadded = zerodict()
    filesremoved = zerodict()
    linelengths = zerodict()
    interarrival = zerodict()
    parents = zerodict()
    dirsadded = zerodict()
    tzoffset = zerodict()

    progress = ui.progress
    _analyzing = _('analyzing')
    _changesets = _('changesets')
    _total = len(revs)

    for i, rev in enumerate(revs):
        progress(_analyzing, i, unit=_changesets, total=_total)
        ctx = repo[rev]
        pl = ctx.parents()
        pctx = pl[0]
        prev = pctx.rev()
        children[prev] += 1
        p1distance[rev - prev] += 1
        parents[len(pl)] += 1
        tzoffset[ctx.date()[1]] += 1
        if len(pl) > 1:
            p2distance[rev - pl[1].rev()] += 1
        if prev == rev - 1:
            lastctx = pctx
        else:
            lastctx = repo[rev - 1]
        if lastctx.rev() != nullrev:
            interarrival[roundto(ctx.date()[0] - lastctx.date()[0], 300)] += 1
        diff = sum((d.splitlines()
                    for d in ctx.diff(pctx, opts=dict(git=True))), [])
        fileadds, diradds, fileremoves, filechanges = 0, 0, 0, 0
        for filename, mar, lineadd, lineremove, binary in parsegitdiff(diff):
            if binary:
                continue
            added = sum(lineadd.itervalues(), 0)
            if mar == 'm':
                if added and lineremove:
                    lineschanged[roundto(added, 5), roundto(lineremove, 5)] += 1
                    filechanges += 1
            elif mar == 'a':
                fileadds += 1
                if '/' in filename:
                    filedir = filename.rsplit('/', 1)[0]
                    if filedir not in pctx.dirs():
                        diradds += 1
                linesinfilesadded[roundto(added, 5)] += 1
            elif mar == 'r':
                fileremoves += 1
            for length, count in lineadd.iteritems():
                linelengths[length] += count
        fileschanged[filechanges] += 1
        filesadded[fileadds] += 1
        dirsadded[diradds] += 1
        filesremoved[fileremoves] += 1

    invchildren = zerodict()

    for rev, count in children.iteritems():
        invchildren[count] += 1

    if output != '-':
        ui.status(_('writing output to %s\n') % output)

    def pronk(d):
        return sorted(d.iteritems(), key=lambda x: x[1], reverse=True)

    json.dump(dict(revs=len(revs),
                   lineschanged=pronk(lineschanged),
                   children=pronk(invchildren),
                   fileschanged=pronk(fileschanged),
                   filesadded=pronk(filesadded),
                   linesinfilesadded=pronk(linesinfilesadded),
                   dirsadded=pronk(dirsadded),
                   filesremoved=pronk(filesremoved),
                   linelengths=pronk(linelengths),
                   parents=pronk(parents),
                   p1distance=pronk(p1distance),
                   p2distance=pronk(p2distance),
                   interarrival=pronk(interarrival),
                   tzoffset=pronk(tzoffset),
                   ),
              fp)
    fp.close()

@command('synthesize',
         [('c', 'count', 0, _('create given number of commits'), _('COUNT')),
          ('', 'dict', '', _('path to a dictionary of words'), _('FILE'))],
         _('hg synthesize [OPTION].. DESCFILE'))
def synthesize(ui, repo, descpath, **opts):
    '''synthesize commits based on a model of an existing repository

    The model must have been generated by :hg:`analyze`. Commits will
    be generated randomly according to the probabilities described in
    the model.

    When synthesizing new content, commit descriptions, and user
    names, words will be chosen randomly from a dictionary that is
    presumed to contain one word per line. Use --dict to specify the
    path to an alternate dictionary to use.
    '''
    try:
        fp = hg.openpath(ui, descpath)
    except Exception, err:
        raise util.Abort('%s: %s' % (descpath, err[0].strerror))
    desc = json.load(fp)
    fp.close()

    def cdf(l):
        if not l:
            return [], []
        vals, probs = zip(*sorted(l, key=lambda x: x[1], reverse=True))
        t = float(sum(probs, 0))
        s, cdfs = 0, []
        for v in probs:
            s += v
            cdfs.append(s / t)
        return vals, cdfs

    lineschanged = cdf(desc['lineschanged'])
    fileschanged = cdf(desc['fileschanged'])
    filesadded = cdf(desc['filesadded'])
    dirsadded = cdf(desc['dirsadded'])
    filesremoved = cdf(desc['filesremoved'])
    linelengths = cdf(desc['linelengths'])
    parents = cdf(desc['parents'])
    p1distance = cdf(desc['p1distance'])
    p2distance = cdf(desc['p2distance'])
    interarrival = cdf(desc['interarrival'])
    linesinfilesadded = cdf(desc['linesinfilesadded'])
    tzoffset = cdf(desc['tzoffset'])

    dictfile = opts.get('dict') or '/usr/share/dict/words'
    try:
        fp = open(dictfile, 'rU')
    except IOError, err:
        raise util.Abort('%s: %s' % (dictfile, err.strerror))
    words = fp.read().splitlines()
    fp.close()

    def pick(cdf):
        return cdf[0][bisect.bisect_left(cdf[1], random.random())]

    def makeline(minimum=0):
        total = max(minimum, pick(linelengths))
        c, l = 0, []
        while c < total:
            w = random.choice(words)
            c += len(w) + 1
            l.append(w)
        return ' '.join(l)

    wlock = repo.wlock()
    lock = repo.lock()

    nevertouch = set(('.hgsub', '.hgignore', '.hgtags'))

    progress = ui.progress
    _synthesizing = _('synthesizing')
    _changesets = _('changesets')

    count = int(opts['count'])
    heads = set(map(repo.changelog.rev, repo.heads()))
    for i in xrange(count):
        progress(_synthesizing, i, unit=_changesets, total=count)

        node = repo.changelog.node
        revs = len(repo)

        def pickhead(heads, distance):
            if heads:
                lheads = sorted(heads)
                rev = revs - min(pick(distance), revs)
                if rev < lheads[-1]:
                    rev = lheads[bisect.bisect_left(lheads, rev)]
                else:
                    rev = lheads[-1]
                return rev, node(rev)
            return nullrev, nullid

        r1 = revs - min(pick(p1distance), revs)
        p1 = node(r1)

        # the number of heads will grow without bound if we use a pure
        # model, so artificially constrain their proliferation
        if pick(parents) == 2 or len(heads) > random.randint(1, 20):
            r2, p2 = pickhead(heads.difference([r1]), p2distance)
        else:
            r2, p2 = nullrev, nullid

        pl = [p1, p2]
        pctx = repo[r1]
        mf = pctx.manifest()
        mfk = mf.keys()
        changes = {}
        if mfk:
            for __ in xrange(pick(fileschanged)):
                for __ in xrange(10):
                    fctx = pctx.filectx(random.choice(mfk))
                    path = fctx.path()
                    if not (path in nevertouch or fctx.isbinary() or
                            'l' in fctx.flags()):
                        break
                lines = fctx.data().splitlines()
                add, remove = pick(lineschanged)
                for __ in xrange(remove):
                    if not lines:
                        break
                    del lines[random.randrange(0, len(lines))]
                for __ in xrange(add):
                    lines.insert(random.randint(0, len(lines)), makeline())
                path = fctx.path()
                changes[path] = context.memfilectx(path,
                                                   '\n'.join(lines) + '\n')
            for __ in xrange(pick(filesremoved)):
                path = random.choice(mfk)
                for __ in xrange(10):
                    path = random.choice(mfk)
                    if path not in changes:
                        changes[path] = None
                        break
        if filesadded:
            dirs = list(pctx.dirs())
            dirs.append('')
        for __ in xrange(pick(filesadded)):
            path = [random.choice(dirs)]
            if pick(dirsadded):
                path.append(random.choice(words))
            path.append(random.choice(words))
            path = '/'.join(filter(None, path))
            data = '\n'.join(makeline()
                             for __ in xrange(pick(linesinfilesadded))) + '\n'
            changes[path] = context.memfilectx(path, data)
        def filectxfn(repo, memctx, path):
            data = changes[path]
            if data is None:
                raise IOError
            return data
        if not changes:
            continue
        if revs:
            date = repo['tip'].date()[0] + pick(interarrival)
        else:
            date = time.time() - (86400 * count)
        user = random.choice(words) + '@' + random.choice(words)
        mc = context.memctx(repo, pl, makeline(minimum=2),
                            sorted(changes.iterkeys()),
                            filectxfn, user, '%d %d' % (date, pick(tzoffset)))
        newnode = mc.commit()
        heads.add(repo.changelog.rev(newnode))
        heads.discard(r1)
        heads.discard(r2)

    lock.release()
    wlock.release()
