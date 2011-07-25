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
        raise EOFError()
    channel, length = struct.unpack('>cI', data)
    if channel in 'IL':
        return channel, length
    else:
        return channel, server.stdout.read(length)

def runcommand(server, args, output=sys.stdout, error=sys.stderr, input=None):
    server.stdin.write('runcommand\n')
    writeblock(server, '\0'.join(args))

    if not input:
        input = cStringIO.StringIO()

    while True:
        ch, data = readchannel(server)
        if ch == 'o':
            output.write(data)
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
    print '%c, %r' % (ch, re.sub('encoding: [a-zA-Z0-9-]+', 'encoding: ***', data))

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
    f = open('foo/bar', 'w')
    f.write('a')
    f.close()
    runcommand(server, ['--cwd', 'foo', 'st', 'bar'])
    runcommand(server, ['st', 'foo/bar'])
    os.remove('foo/bar')

def localhgrc(server):
    """ check that local configs for the cached repo aren't inherited when -R
    is used """
    readchannel(server)

    # the cached repo local hgrc contains ui.foo=bar, so showconfig should show it
    runcommand(server, ['showconfig'])

    # but not for this repo
    runcommand(server, ['init', 'foo'])
    runcommand(server, ['-R', 'foo', 'showconfig'])
    shutil.rmtree('foo')

def hook(**args):
    print 'hook talking'
    print 'now try to read something: %r' % sys.stdin.read()

def hookoutput(server):
    readchannel(server)
    runcommand(server, ['--config',
                        'hooks.pre-identify=python:test-commandserver.hook', 'id'],
               input=cStringIO.StringIO('some input'))

def outsidechanges(server):
    readchannel(server)
    os.system('echo a >> a && hg ci -Am2')
    runcommand(server, ['tip'])

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

def tagscache(server):
    readchannel(server)
    runcommand(server, ['id', '-t', '-r', '0'])
    os.system('hg tag -r 0 foo')
    runcommand(server, ['id', '-t', '-r', '0'])

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
