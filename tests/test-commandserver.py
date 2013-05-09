import sys, os, struct, subprocess, cStringIO, re, shutil

def connect(path=None):
    cmdline = ['hg', 'serve', '--cmdserver', 'pipe']
    if path:
        cmdline += ['-R', path]

    server = subprocess.Popen(cmdline, stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE)

    return server

def writeblock(server, data):
    server.stdin.write(struct.pack('>I', len(data)))
    server.stdin.write(data)
    server.stdin.flush()

def readchannel(server):
    data = server.stdout.read(5)
    if not data:
        raise EOFError
    channel, length = struct.unpack('>cI', data)
    if channel in 'IL':
        return channel, length
    else:
        return channel, server.stdout.read(length)

def sep(text):
    return text.replace('\\', '/')

def runcommand(server, args, output=sys.stdout, error=sys.stderr, input=None,
               outfilter=lambda x: x):
    print ' runcommand', ' '.join(args)
    sys.stdout.flush()
    server.stdin.write('runcommand\n')
    writeblock(server, '\0'.join(args))

    if not input:
        input = cStringIO.StringIO()

    while True:
        ch, data = readchannel(server)
        if ch == 'o':
            output.write(outfilter(data))
            output.flush()
        elif ch == 'e':
            error.write(data)
            error.flush()
        elif ch == 'I':
            writeblock(server, input.read(data))
        elif ch == 'L':
            writeblock(server, input.readline(data))
        elif ch == 'r':
            return struct.unpack('>i', data)[0]
        else:
            print "unexpected channel %c: %r" % (ch, data)
            if ch.isupper():
                return

def check(func, repopath=None):
    print
    print 'testing %s:' % func.__name__
    print
    sys.stdout.flush()
    server = connect(repopath)
    try:
        return func(server)
    finally:
        server.stdin.close()
        server.wait()

def unknowncommand(server):
    server.stdin.write('unknowncommand\n')

def hellomessage(server):
    ch, data = readchannel(server)
    # escaping python tests output not supported
    print '%c, %r' % (ch, re.sub('encoding: [a-zA-Z0-9-]+', 'encoding: ***',
                                 data))

    # run an arbitrary command to make sure the next thing the server sends
    # isn't part of the hello message
    runcommand(server, ['id'])

def checkruncommand(server):
    # hello block
    readchannel(server)

    # no args
    runcommand(server, [])

    # global options
    runcommand(server, ['id', '--quiet'])

    # make sure global options don't stick through requests
    runcommand(server, ['id'])

    # --config
    runcommand(server, ['id', '--config', 'ui.quiet=True'])

    # make sure --config doesn't stick
    runcommand(server, ['id'])

def inputeof(server):
    readchannel(server)
    server.stdin.write('runcommand\n')
    # close stdin while server is waiting for input
    server.stdin.close()

    # server exits with 1 if the pipe closed while reading the command
    print 'server exit code =', server.wait()

def serverinput(server):
    readchannel(server)

    patch = """
# HG changeset patch
# User test
# Date 0 0
# Node ID c103a3dec114d882c98382d684d8af798d09d857
# Parent  0000000000000000000000000000000000000000
1

diff -r 000000000000 -r c103a3dec114 a
--- /dev/null	Thu Jan 01 00:00:00 1970 +0000
+++ b/a	Thu Jan 01 00:00:00 1970 +0000
@@ -0,0 +1,1 @@
+1
"""

    runcommand(server, ['import', '-'], input=cStringIO.StringIO(patch))
    runcommand(server, ['log'])

def cwd(server):
    """ check that --cwd doesn't persist between requests """
    readchannel(server)
    os.mkdir('foo')
    f = open('foo/bar', 'wb')
    f.write('a')
    f.close()
    runcommand(server, ['--cwd', 'foo', 'st', 'bar'])
    runcommand(server, ['st', 'foo/bar'])
    os.remove('foo/bar')

def localhgrc(server):
    """ check that local configs for the cached repo aren't inherited when -R
    is used """
    readchannel(server)

    # the cached repo local hgrc contains ui.foo=bar, so showconfig should
    # show it
    runcommand(server, ['showconfig'])

    # but not for this repo
    runcommand(server, ['init', 'foo'])
    runcommand(server, ['-R', 'foo', 'showconfig', 'ui', 'defaults'])
    shutil.rmtree('foo')

def hook(**args):
    print 'hook talking'
    print 'now try to read something: %r' % sys.stdin.read()

def hookoutput(server):
    readchannel(server)
    runcommand(server, ['--config',
                        'hooks.pre-identify=python:test-commandserver.hook',
                        'id'],
               input=cStringIO.StringIO('some input'))

def outsidechanges(server):
    readchannel(server)
    f = open('a', 'ab')
    f.write('a\n')
    f.close()
    runcommand(server, ['status'])
    os.system('hg ci -Am2')
    runcommand(server, ['tip'])
    runcommand(server, ['status'])

def bookmarks(server):
    readchannel(server)
    runcommand(server, ['bookmarks'])

    # changes .hg/bookmarks
    os.system('hg bookmark -i bm1')
    os.system('hg bookmark -i bm2')
    runcommand(server, ['bookmarks'])

    # changes .hg/bookmarks.current
    os.system('hg upd bm1 -q')
    runcommand(server, ['bookmarks'])

    runcommand(server, ['bookmarks', 'bm3'])
    f = open('a', 'ab')
    f.write('a\n')
    f.close()
    runcommand(server, ['commit', '-Amm'])
    runcommand(server, ['bookmarks'])

def tagscache(server):
    readchannel(server)
    runcommand(server, ['id', '-t', '-r', '0'])
    os.system('hg tag -r 0 foo')
    runcommand(server, ['id', '-t', '-r', '0'])

def setphase(server):
    readchannel(server)
    runcommand(server, ['phase', '-r', '.'])
    os.system('hg phase -r . -p')
    runcommand(server, ['phase', '-r', '.'])

def rollback(server):
    readchannel(server)
    runcommand(server, ['phase', '-r', '.', '-p'])
    f = open('a', 'ab')
    f.write('a\n')
    f.close()
    runcommand(server, ['commit', '-Am.'])
    runcommand(server, ['rollback'])
    runcommand(server, ['phase', '-r', '.'])

def branch(server):
    readchannel(server)
    runcommand(server, ['branch'])
    os.system('hg branch foo')
    runcommand(server, ['branch'])
    os.system('hg branch default')

def hgignore(server):
    readchannel(server)
    f = open('.hgignore', 'ab')
    f.write('')
    f.close()
    runcommand(server, ['commit', '-Am.'])
    f = open('ignored-file', 'ab')
    f.write('')
    f.close()
    f = open('.hgignore', 'ab')
    f.write('ignored-file')
    f.close()
    runcommand(server, ['status', '-i', '-u'])

def phasecacheafterstrip(server):
    readchannel(server)

    # create new head, 5:731265503d86
    runcommand(server, ['update', '-C', '0'])
    f = open('a', 'ab')
    f.write('a\n')
    f.close()
    runcommand(server, ['commit', '-Am.', 'a'])
    runcommand(server, ['log', '-Gq'])

    # make it public; draft marker moves to 4:7966c8e3734d
    runcommand(server, ['phase', '-p', '.'])
    # load _phasecache.phaseroots
    runcommand(server, ['phase', '.'], outfilter=sep)

    # strip 1::4 outside server
    os.system('hg -q --config extensions.mq= strip 1')

    # shouldn't raise "7966c8e3734d: no node!"
    runcommand(server, ['branches'])

if __name__ == '__main__':
    os.system('hg init')

    check(hellomessage)
    check(unknowncommand)
    check(checkruncommand)
    check(inputeof)
    check(serverinput)
    check(cwd)

    hgrc = open('.hg/hgrc', 'a')
    hgrc.write('[ui]\nfoo=bar\n')
    hgrc.close()
    check(localhgrc)
    check(hookoutput)
    check(outsidechanges)
    check(bookmarks)
    check(tagscache)
    check(setphase)
    check(rollback)
    check(branch)
    check(hgignore)
    check(phasecacheafterstrip)
