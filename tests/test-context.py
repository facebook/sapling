import os
from mercurial import hg, ui, context, encoding

u = ui.ui()

repo = hg.repository(u, 'test1', create=1)
os.chdir('test1')

# create 'foo' with fixed time stamp
f = open('foo', 'wb')
f.write('foo\n')
f.close()
os.utime('foo', (1000, 1000))

# add+commit 'foo'
repo[None].add(['foo'])
repo.commit(text='commit1', date="0 0")

print "workingfilectx.date =", repo[None]['foo'].date()

# test memctx with non-ASCII commit message

def filectxfn(repo, memctx, path):
    return context.memfilectx(repo, "foo", "")

ctx = context.memctx(repo, ['tip', None],
                     encoding.tolocal("Gr\xc3\xbcezi!"),
                     ["foo"], filectxfn)
ctx.commit()
for enc in "ASCII", "Latin-1", "UTF-8":
    encoding.encoding = enc
    print "%-8s: %s" % (enc, repo["tip"].description())

# test performing a status

def getfilectx(repo, memctx, f):
    fctx = memctx.parents()[0][f]
    data, flags = fctx.data(), fctx.flags()
    if f == 'foo':
        data += 'bar\n'
    return context.memfilectx(repo, f, data, 'l' in flags, 'x' in flags)

ctxa = repo.changectx(0)
ctxb = context.memctx(repo, [ctxa.node(), None], "test diff", ["foo"],
                      getfilectx, ctxa.user(), ctxa.date())

print ctxb.status(ctxa)

# test performing a diff on a memctx

for d in ctxb.diff(ctxa, git=True):
    print d

# test safeness and correctness of "ctx.status()"
print '= checking context.status():'

# ancestor "wcctx ~ 2"
actx2 = repo['.']

repo.wwrite('bar-m', 'bar-m\n', '')
repo.wwrite('bar-r', 'bar-r\n', '')
repo[None].add(['bar-m', 'bar-r'])
repo.commit(text='add bar-m, bar-r', date="0 0")

# ancestor "wcctx ~ 1"
actx1 = repo['.']

repo.wwrite('bar-m', 'bar-m bar-m\n', '')
repo.wwrite('bar-a', 'bar-a\n', '')
repo[None].add(['bar-a'])
repo[None].forget(['bar-r'])

# status at this point:
#   M bar-m
#   A bar-a
#   R bar-r
#   C foo

from mercurial import scmutil

print '== checking workingctx.status:'

wctx = repo[None]
print 'wctx._status=%s' % (str(wctx._status))

print '=== with "pattern match":'
print actx1.status(other=wctx,
                   match=scmutil.matchfiles(repo, ['bar-m', 'foo']))
print 'wctx._status=%s' % (str(wctx._status))
print actx2.status(other=wctx,
                   match=scmutil.matchfiles(repo, ['bar-m', 'foo']))
print 'wctx._status=%s' % (str(wctx._status))

print '=== with "always match" and "listclean=True":'
print actx1.status(other=wctx, listclean=True)
print 'wctx._status=%s' % (str(wctx._status))
print actx2.status(other=wctx, listclean=True)
print 'wctx._status=%s' % (str(wctx._status))

print "== checking workingcommitctx.status:"

wcctx = context.workingcommitctx(repo,
                                 scmutil.status(['bar-m'],
                                                ['bar-a'],
                                                [],
                                                [], [], [], []),
                                 text='', date='0 0')
print 'wcctx._status=%s' % (str(wcctx._status))

print '=== with "always match":'
print actx1.status(other=wcctx)
print 'wcctx._status=%s' % (str(wcctx._status))
print actx2.status(other=wcctx)
print 'wcctx._status=%s' % (str(wcctx._status))

print '=== with "always match" and "listclean=True":'
print actx1.status(other=wcctx, listclean=True)
print 'wcctx._status=%s' % (str(wcctx._status))
print actx2.status(other=wcctx, listclean=True)
print 'wcctx._status=%s' % (str(wcctx._status))

print '=== with "pattern match":'
print actx1.status(other=wcctx,
                   match=scmutil.matchfiles(repo, ['bar-m', 'foo']))
print 'wcctx._status=%s' % (str(wcctx._status))
print actx2.status(other=wcctx,
                   match=scmutil.matchfiles(repo, ['bar-m', 'foo']))
print 'wcctx._status=%s' % (str(wcctx._status))

print '=== with "pattern match" and "listclean=True":'
print actx1.status(other=wcctx,
                   match=scmutil.matchfiles(repo, ['bar-r', 'foo']),
                   listclean=True)
print 'wcctx._status=%s' % (str(wcctx._status))
print actx2.status(other=wcctx,
                   match=scmutil.matchfiles(repo, ['bar-r', 'foo']),
                   listclean=True)
print 'wcctx._status=%s' % (str(wcctx._status))
