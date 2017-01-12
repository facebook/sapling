# treemanifest_correctness.py - simple extension for testing treemanifest
# correctness
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from mercurial import (
    cmdutil,
    manifest,
    mdiff,
    scmutil,
)
from mercurial.node import nullid
from remotefilelog import datapack, contentstore, shallowutil
import difflib, hashlib, os, time
from fastmanifest import cachemanager
import ctreemanifest

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'ships-with-fb-hgext'

@command('testtree', [
    ('', 'build', '', ''),
    ('', 'revs', 'master + master~5', ''),
    ], '')
def testpackedtrees(ui, repo, *args, **opts):
    packpath = shallowutil.getcachepackpath(repo, 'manifest')
    if not os.path.exists(packpath):
        os.mkdir(packpath)
    opener = scmutil.vfs(packpath)
    if opts.get('build'):
        with datapack.mutabledatapack(opener) as newpack:
            buildtreepack(repo, newpack, opts.get('build'))
            newpack.close()

    packstore = datapack.datapackstore(ui, opener.base,
            usecdatapack=ui.configbool('remotefilelog', 'fastdatapack'))
    unionstore = contentstore.unioncontentstore(packstore)

    ctxs = list(repo.set(opts.get('revs')))
    profiletreepack(repo, unionstore, ctxs[0].hex(), ctxs[1].hex(), opts)

class cachestore(object):
    def __init__(self):
        self._cache = {}

    def get(self, name, node):
        return self._cache[(name, node)]

    def getdeltachain(self, name, node):
        data = self._cache[(name, node)]
        return [(name, node, name, nullid, data)]

    def add(self, name, node, data):
        self._cache[(name, node)] = data

def buildtreepack(repo, pack, revs):
    mf = repo.manifest
    cache = cachestore()
    packstore = datapack.datapackstore(repo.ui, pack.opener.base)
    store = contentstore.unioncontentstore(cache, packstore)
    ctxs = list(repo.set(revs))
    for count, ctx in enumerate(ctxs):
        repo.ui.progress(('manifests'), count, total=len(ctxs))
        mfnode = ctx.manifestnode()
        try:
            store.get('', mfnode)
            continue
        except KeyError:
            pass

        try:
            # if we have the parent tree, let's use it
            p1, p2 = mf.parents(mfnode)
            p1rev = mf.rev(p1)
            mfrev = mf.rev(mfnode)
            store.get('', p1)

            if p2 == nullid and mf.deltaparent(mfrev) == p1rev:
                mfdelta = mf.readdelta(mfnode)
                adds = list((filename, n, f)
                            for (filename, n, f) in mfdelta.iterentries())
                deletes = set(ctx.files()).difference(
                    filename
                    for filename, n, f in adds)
            else:
                mfctx = ctx.manifest()
                mfdiff = mf.read(p1).diff(mfctx)
                adds = list((f, bn, bf) for f, ((an, af), (bn, bf)) in
                            mfdiff.iteritems() if bn is not None)
                deletes = list(f for f, ((an, af), (bn, bf)) in
                               mfdiff.iteritems() if bn is None)

            tmfctx = read(store, '', p1).copy()
            for filename in deletes:
                del tmfctx[filename]
            for filename, n, f in adds:
                tmfctx[filename] = n
                tmfctx.setflag(filename, f)
        except KeyError:
            mfctx = ctx.manifest()
            tmfctx = manifest.treemanifest(text=mfctx.text())

        p1, p2 = mf.parents(mfnode)
        add(store, cache, pack, tmfctx, ctx.rev(), p1, p2,
                forcenode=ctx.manifestnode())
    repo.ui.progress(('manifests'), None)

def add(store, cache, pack, mf, linkrev, p1, p2, forcenode=False):
    try:
        store.get(mf._dir, p1)
        p1mf = read(store, mf._dir, p1)
    except KeyError:
        p1mf = manifest.treemanifest()

    try:
        store.get(mf._dir, p2)
        p2mf = read(store, mf._dir, p2)
    except KeyError:
        p2mf = manifest.treemanifest()

    return _addtree(store, cache, pack, mf, linkrev,
                    p1mf, p2mf, forcenode=forcenode)

def read(store, dir, node):
    def gettext():
        return store.get(dir, node)
    def readsubtree(dir, subm):
        return read(store, dir, subm)
    m = manifest.treemanifest(dir=dir)
    m.read(gettext, readsubtree)
    m.setnode(node)
    return m

def _addtree(store, cache, pack, m, linkrev, m1, m2, forcenode=False):
    # If the manifest is unchanged compared to one parent,
    # don't write a new revision
    if m.unmodifiedsince(m1) or m.unmodifiedsince(m2):
        return m.node()
    def writesubtree(subm, subp1, subp2):
        add(store, cache, pack, subm, linkrev, subp1, subp2)

    usemfv2 = False
    m1._load()
    m2._load()
    m.writesubtrees(m1, m2, writesubtree)
    text = m.dirtext(usemfv2)
    # Double-check whether contents are unchanged to one parent
    if text == m1.dirtext(usemfv2):
        n = m1.node()
    elif text == m2.dirtext(usemfv2):
        n = m2.node()
    else:
        n = hashlib.sha1(m1.node() + m2.node() + text).digest()
        # Save nodeid so parent manifest can calculate its nodeid
        if forcenode:
            n = forcenode
        try:
            store.get(m._dir, n)
        except KeyError:
            deltabase = nullid
            delta = text
            if False and m1.node() != nullid:
                deltabase = m1.node()
                delta = mdiff.textdiff(m1.dirtext(usemfv2), text)
            pack.add(m._dir, n, deltabase, delta)
        cache.add(m._dir, n, text)
    m.setnode(n)
    return n

def profiletreepack(repo, store, rev1, rev2, opts):
    def exectest(name, prep, func):
        elapsed = 0
        elapsedprep = 0
        args = []
        if prep:
            startprep = time.time()
            args = prep()
            elapsedprep += time.time() - startprep
        import gc
        gc.disable()
        start = time.time()
        result = func(*args)
        elapsed += time.time() - start
        gc.enable()
        gc.collect()
        repo.ui.progress(name, None)

        total = elapsed + elapsedprep
        repo.ui.status(("%0.2f" % (elapsedprep,)).ljust(15))
        repo.ui.status(("%0.2f" % (elapsed,)).ljust(15))
        repo.ui.status(("%0.2f" % (total,)).ljust(15))

        return result

    ctx1 = list(repo.set(rev1))[0]
    ctx2 = list(repo.set(rev2))[0]

    cachemanager.cachemanifestfillandtrim(
        repo.ui, repo, ['%s + %s' % (ctx1.rev(), ctx2.rev())])

    def flatconstructor(mfnode):
        repo.manifest.clearcaches()
        return repo.manifest.read(mfnode)._flatmanifest()

    def ctreeconstructor(mfnode):
        treemf = ctreemanifest.treemanifest(store, mfnode)
        return treemf

    # Test bodies
    def prepone(new):
        return [new(ctx1.manifestnode())]
    def preptwo(new):
        m1 = new(ctx1.manifestnode())
        m2 = new(ctx2.manifestnode())
        return m1, m2

    def diff(m1, m2):
        diff = m1.diff(m2)

        result = []
        for fp in sorted(diff.keys()):
            result.append((fp, diff[fp]))
        return result
    def fulliter(m1):
        entries = [x for x in m1]
        return entries

    tests = {
        'diff': (preptwo, diff),
        'fulliter': (prepone, fulliter),
    }

    kinds = {
        'flat': flatconstructor,
        'ctree': ctreeconstructor,
    }

    for testname, testopts in tests.items():
        prepfunc, func = testopts
        teststr = ('%s' % (testname,)).ljust(14)
        repo.ui.status(("\n%sPrep           Run            Total\n") %
                        (teststr))

        results = {}
        for kind, kindconstructor in kinds.items():
            repo.ui.status(("%s" % (kind)).ljust(14))
            def prep():
                return prepfunc(kindconstructor)
            results[kind] = exectest(testname, prep, func)
            repo.ui.status("\n")
        repo.ui.status("\n")

        correct = results['flat']
        uut = results['ctree']

        s = difflib.SequenceMatcher(None, uut, correct)

        for tag, i1, i2, j1, j2 in s.get_opcodes():
            if tag != 'equal':
                repo.ui.status(("%7s a[%d:%d] (%s) b[%d:%d] (%s)\n" %
                                (tag, i1, i2, uut[i1:i2],
                                 j1, j2, correct[j1:j2])))
            else:
                repo.ui.status(("%7s a[%d:%d] b[%d:%d]\n" %
                                (tag, i1, i2, j1, j2)))
