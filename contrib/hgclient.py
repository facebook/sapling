# A minimal client for Mercurial's command server

import sys, struct, subprocess, cStringIO

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
    print '*** runcommand', ' '.join(args)
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
            ret, = struct.unpack('>i', data)
            if ret != 0:
                print ' [%d]' % ret
            return ret
        else:
            print "unexpected channel %c: %r" % (ch, data)
            if ch.isupper():
                return

def check(func):
    sys.stdout.flush()
    server = connect()
    try:
        return func(server)
    finally:
        server.stdin.close()
        server.wait()
