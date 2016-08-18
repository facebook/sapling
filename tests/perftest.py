# perf-test.py - simple extension for perf testing manifest performance
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from mercurial import cmdutil, error, manifest, scmutil
from remotefilelog import datapack, contentstore
import pdb, hashlib, time
from fastmanifest.implementation import fastmanifestcache
from fastmanifest import cachemanager
import ctreemanifest

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = ''

@command('testtree', [
    ('', 'build', '', ''),
    ('', 'test', '', ''),
    ('', 'kind', '', ''),
    ('i', 'iterations', '10', ''),
    ], '')
def testpackedtrees(ui, repo, *args, **opts):
    opener = scmutil.vfs('/tmp/durhampack')
    if opts.get('build'):
        with datapack.mutabledatapack(opener) as newpack:
            buildtreepack(repo, newpack, opts.get('build'))
            newpack.close()

    packstore = datapack.datapackstore(opener.base,
            usecdatapack=ui.configbool('remotefilelog', 'fastdatapack'))
    unionstore = contentstore.unioncontentstore(packstore)

    # TODO: make these revs command options
    profiletreepack(repo, unionstore, 'master~50000' , 'master~45000', opts)

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
    packstore = datapack.datapackstore(pack.opener.base)
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
    def exectest(name, count, prep, func):
        elapsed = 0
        elapsedprep = 0
        for i in xrange(0, count):
            args = []
            if prep:
                startprep = time.time()
                args = prep()
                elapsedprep += time.time() - startprep
            import gc
            gc.disable()
            start = time.time()
            func(*args)
            elapsed += time.time() - start
            gc.enable()
            gc.collect()
            repo.ui.progress(name, i, total=count)
        repo.ui.progress(name, None)

        total = elapsed + elapsedprep
        repo.ui.status(("%0.2f (%0.2f)" % (elapsedprep, elapsedprep / (count *
            1.0))).ljust(15))
        repo.ui.status(("%0.2f (%0.2f)" % (elapsed, elapsed / (count *
            1.0))).ljust(15))
        repo.ui.status(("%0.2f (%0.2f)" % (total, total / (count *
            1.0))).ljust(15))

    ctx1 = list(repo.set(rev1))[0]
    ctx2 = list(repo.set(rev2))[0]

    cachemanager.cachemanifestfillandtrim(
        repo.ui, repo, ['%s + %s' % (ctx1.rev(), ctx2.rev())])

    def treeconstructor(mfnode):
        return read(store, '', mfnode)
    def flatconstructor(mfnode):
        repo.manifest.clearcaches()
        return repo.manifest.read(mfnode)._flatmanifest()

    cacheopener = scmutil.vfs('/tmp/durhampack')
    manager = fastmanifestcache.getinstance(cacheopener, repo.ui)
    def fastconstructor(mfnode):
        repo.manifest.clearcaches()
        manager.inmemorycache.clear()
        return repo.manifest.read(mfnode)._cachedmanifest()

    class FakeStore(object):
        def __init__(self):
            self._cache = {}

        def get(self, filename, node):
            try:
                return self._cache[(filename, node)]
            except KeyError:
                result = store.get(filename, node)
                self._cache[(filename, node)] = result
                return result
    fakestore = FakeStore()
    def ctreeconstructor(mfnode):
        # Enable the fake store to remove blob lookup time
        #treemf = ctreemanifest.treemanifest(fakestore, mfnode)
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
        if len(diff) < 10000:
            raise error.Abort("diff only found %s items" % len(diff))
    def find(m1):
        findresult = m1.find('fbcode/hphp/test/run')
        if findresult == (None, None):
            raise error.Abort("None find result")
    def fulliter(m1):
        count = 0
        for x in m1:
            count += 1

        if count < 900000:
            raise error.Abort("fulliter only found %s files" % count)

    kindconstructor = {
        'tree': treeconstructor,
        'flat': flatconstructor,
        'fast': fastconstructor,
        'ctree': ctreeconstructor,
    }

    iterations = int(opts.get('iterations'))
    testopts = {
        'diff': (preptwo, diff, iterations),
        'find': (prepone, find, iterations),
        'fulliter': (prepone, fulliter, max(1, iterations / 2)),
    }

    tests = opts.get('test', 'all').split(',')
    if tests[0] in ['all', '']:
        tests = testopts.keys()
    kinds = opts.get('kind', 'all').split(',')
    if kinds[0] in ['all', '']:
        kinds = kindconstructor.keys()

    # Prime any caches
    for kind in kinds:
        preptwo(kindconstructor[kind])

    for test in tests:
        prepfunc, func, iterations = testopts[test]
        teststr = ('%s (%s)' % (test, iterations)).ljust(14)
        repo.ui.status("\n%sPrep           Run            Total\n" %
                        (teststr))
        for kind in kinds:
            repo.ui.status(("%s" % (kind)).ljust(14))
            def prep():
                return prepfunc(kindconstructor[kind])
            exectest(test, iterations, prep, func)
            repo.ui.status("\n")
        repo.ui.status("\n")
