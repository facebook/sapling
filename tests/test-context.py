import os
from mercurial import hg, ui, context, encoding

u = ui.ui()

repo = hg.repository(u, 'test1', create=1)
os.chdir('test1')

# create 'foo' with fixed time stamp
f = open('foo', 'w')
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
