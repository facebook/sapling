from __future__ import absolute_import, print_function
import os
from mercurial.node import hex
from mercurial import (
    context,
    encoding,
    hg,
    scmutil,
    ui as uimod,
)

u = uimod.ui.load()

repo = hg.repository(u, 'test1', create=1)
os.chdir('test1')

# create 'foo' with fixed time stamp
f = open('foo', 'wb')
f.write(b'foo\n')
f.close()
os.utime('foo', (1000, 1000))

# add+commit 'foo'
repo[None].add(['foo'])
repo.commit(text='commit1', date="0 0")

d = repo[None]['foo'].date()
if os.name == 'nt':
    d = d[:2]
print("workingfilectx.date = (%d, %d)" % d)

# test memctx with non-ASCII commit message

def filectxfn(repo, memctx, path):
    return context.memfilectx(repo, memctx, "foo", "")

ctx = context.memctx(repo, ['tip', None],
                     encoding.tolocal("Gr\xc3\xbcezi!"),
                     ["foo"], filectxfn)
ctx.commit()
for enc in "ASCII", "Latin-1", "UTF-8":
    encoding.encoding = enc
    print("%-8s: %s" % (enc, repo["tip"].description()))

# test performing a status

def getfilectx(repo, memctx, f):
    fctx = memctx.parents()[0][f]
    data, flags = fctx.data(), fctx.flags()
    if f == 'foo':
        data += 'bar\n'
    return context.memfilectx(repo, memctx, f, data, 'l' in flags, 'x' in flags)

ctxa = repo.changectx(0)
ctxb = context.memctx(repo, [ctxa.node(), None], "test diff", ["foo"],
                      getfilectx, ctxa.user(), ctxa.date())

print(ctxb.status(ctxa))

# test performing a diff on a memctx

for d in ctxb.diff(ctxa, git=True):
    print(d, end='')

# test safeness and correctness of "ctx.status()"
print('= checking context.status():')

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

print('== checking workingctx.status:')

wctx = repo[None]
print('wctx._status=%s' % (str(wctx._status)))

print('=== with "pattern match":')
print(actx1.status(other=wctx,
                   match=scmutil.matchfiles(repo, ['bar-m', 'foo'])))
print('wctx._status=%s' % (str(wctx._status)))
print(actx2.status(other=wctx,
                   match=scmutil.matchfiles(repo, ['bar-m', 'foo'])))
print('wctx._status=%s' % (str(wctx._status)))

print('=== with "always match" and "listclean=True":')
print(actx1.status(other=wctx, listclean=True))
print('wctx._status=%s' % (str(wctx._status)))
print(actx2.status(other=wctx, listclean=True))
print('wctx._status=%s' % (str(wctx._status)))

print("== checking workingcommitctx.status:")

wcctx = context.workingcommitctx(repo,
                                 scmutil.status(['bar-m'],
                                                ['bar-a'],
                                                [],
                                                [], [], [], []),
                                 text='', date='0 0')
print('wcctx._status=%s' % (str(wcctx._status)))

print('=== with "always match":')
print(actx1.status(other=wcctx))
print('wcctx._status=%s' % (str(wcctx._status)))
print(actx2.status(other=wcctx))
print('wcctx._status=%s' % (str(wcctx._status)))

print('=== with "always match" and "listclean=True":')
print(actx1.status(other=wcctx, listclean=True))
print('wcctx._status=%s' % (str(wcctx._status)))
print(actx2.status(other=wcctx, listclean=True))
print('wcctx._status=%s' % (str(wcctx._status)))

print('=== with "pattern match":')
print(actx1.status(other=wcctx,
                   match=scmutil.matchfiles(repo, ['bar-m', 'foo'])))
print('wcctx._status=%s' % (str(wcctx._status)))
print(actx2.status(other=wcctx,
                   match=scmutil.matchfiles(repo, ['bar-m', 'foo'])))
print('wcctx._status=%s' % (str(wcctx._status)))

print('=== with "pattern match" and "listclean=True":')
print(actx1.status(other=wcctx,
                   match=scmutil.matchfiles(repo, ['bar-r', 'foo']),
                   listclean=True))
print('wcctx._status=%s' % (str(wcctx._status)))
print(actx2.status(other=wcctx,
                   match=scmutil.matchfiles(repo, ['bar-r', 'foo']),
                   listclean=True))
print('wcctx._status=%s' % (str(wcctx._status)))

os.chdir('..')

# test manifestlog being changed
print('== commit with manifestlog invalidated')

repo = hg.repository(u, 'test2', create=1)
os.chdir('test2')

# make some commits
for i in [b'1', b'2', b'3']:
    with open(i, 'wb') as f:
        f.write(i)
    status = scmutil.status([], [i], [], [], [], [], [])
    ctx = context.workingcommitctx(repo, status, text=i, user=b'test@test.com',
                                   date=(0, 0))
    ctx.p1().manifest() # side effect: cache manifestctx
    n = repo.commitctx(ctx)
    print('commit %s: %s' % (i, hex(n)))

    # touch 00manifest.i mtime so storecache could expire.
    # repo.__dict__['manifestlog'] is deleted by transaction releasefn.
    st = repo.svfs.stat('00manifest.i')
    repo.svfs.utime('00manifest.i', (st.st_mtime + 1, st.st_mtime + 1))

    # read the file just committed
    try:
        if repo[n][i].data() != i:
            print('data mismatch')
    except Exception as ex:
        print('cannot read data: %r' % ex)

with repo.wlock(), repo.lock(), repo.transaction('test'):
    with open(b'4', 'wb') as f:
        f.write(b'4')
    repo.dirstate.normal('4')
    repo.commit('4')
    revsbefore = len(repo.changelog)
    repo.invalidate(clearfilecache=True)
    revsafter = len(repo.changelog)
    if revsbefore != revsafter:
        print('changeset lost by repo.invalidate()')
