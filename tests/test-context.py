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
    return context.memfilectx("foo", "")

ctx = context.memctx(repo, ['tip', None],
                     encoding.tolocal("Gr\xc3\xbcezi!"),
                     ["foo"], filectxfn)
ctx.commit()
for enc in "ASCII", "Latin-1", "UTF-8":
    encoding.encoding = enc
    print "%-8s: %s" % (enc, repo["tip"].description())
