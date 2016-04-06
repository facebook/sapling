# wireproto.py - generic wire protocol support functions
#
# Copyright 2005-2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import itertools
import os
import sys
import tempfile

from .i18n import _
from .node import (
    bin,
    hex,
)

from . import (
    bundle2,
    changegroup as changegroupmod,
    encoding,
    error,
    exchange,
    peer,
    pushkey as pushkeymod,
    streamclone,
    util,
)

urlerr = util.urlerr
urlreq = util.urlreq

bundle2required = _(
    'incompatible Mercurial client; bundle2 required\n'
    '(see https://www.mercurial-scm.org/wiki/IncompatibleClient)\n')

class abstractserverproto(object):
    """abstract class that summarizes the protocol API

    Used as reference and documentation.
    """

    def getargs(self, args):
        """return the value for arguments in <args>

        returns a list of values (same order as <args>)"""
        raise NotImplementedError()

    def getfile(self, fp):
        """write the whole content of a file into a file like object

        The file is in the form::

            (<chunk-size>\n<chunk>)+0\n

        chunk size is the ascii version of the int.
        """
        raise NotImplementedError()

    def redirect(self):
        """may setup interception for stdout and stderr

        See also the `restore` method."""
        raise NotImplementedError()

    # If the `redirect` function does install interception, the `restore`
    # function MUST be defined. If interception is not used, this function
    # MUST NOT be defined.
    #
    # left commented here on purpose
    #
    #def restore(self):
    #    """reinstall previous stdout and stderr and return intercepted stdout
    #    """
    #    raise NotImplementedError()

    def groupchunks(self, cg):
        """return 4096 chunks from a changegroup object

        Some protocols may have compressed the contents."""
        raise NotImplementedError()

class remotebatch(peer.batcher):
    '''batches the queued calls; uses as few roundtrips as possible'''
    def __init__(self, remote):
        '''remote must support _submitbatch(encbatch) and
        _submitone(op, encargs)'''
        peer.batcher.__init__(self)
        self.remote = remote
    def submit(self):
        req, rsp = [], []
        for name, args, opts, resref in self.calls:
            mtd = getattr(self.remote, name)
            batchablefn = getattr(mtd, 'batchable', None)
            if batchablefn is not None:
                batchable = batchablefn(mtd.im_self, *args, **opts)
                encargsorres, encresref = batchable.next()
                if encresref:
                    req.append((name, encargsorres,))
                    rsp.append((batchable, encresref, resref,))
                else:
                    resref.set(encargsorres)
            else:
                if req:
                    self._submitreq(req, rsp)
                    req, rsp = [], []
                resref.set(mtd(*args, **opts))
        if req:
            self._submitreq(req, rsp)
    def _submitreq(self, req, rsp):
        encresults = self.remote._submitbatch(req)
        for encres, r in zip(encresults, rsp):
            batchable, encresref, resref = r
            encresref.set(encres)
            resref.set(batchable.next())

class remoteiterbatcher(peer.iterbatcher):
    def __init__(self, remote):
        super(remoteiterbatcher, self).__init__()
        self._remote = remote

    def __getattr__(self, name):
        if not getattr(self._remote, name, False):
            raise AttributeError(
                'Attempted to iterbatch non-batchable call to %r' % name)
        return super(remoteiterbatcher, self).__getattr__(name)

    def submit(self):
        """Break the batch request into many patch calls and pipeline them.

        This is mostly valuable over http where request sizes can be
        limited, but can be used in other places as well.
        """
        req, rsp = [], []
        for name, args, opts, resref in self.calls:
            mtd = getattr(self._remote, name)
            batchable = mtd.batchable(mtd.im_self, *args, **opts)
            encargsorres, encresref = batchable.next()
            assert encresref
            req.append((name, encargsorres))
            rsp.append((batchable, encresref))
        if req:
            self._resultiter = self._remote._submitbatch(req)
        self._rsp = rsp

    def results(self):
        for (batchable, encresref), encres in itertools.izip(
                self._rsp, self._resultiter):
            encresref.set(encres)
            yield batchable.next()

# Forward a couple of names from peer to make wireproto interactions
# slightly more sensible.
batchable = peer.batchable
future = peer.future

# list of nodes encoding / decoding

def decodelist(l, sep=' '):
    if l:
        return map(bin, l.split(sep))
    return []

def encodelist(l, sep=' '):
    try:
        return sep.join(map(hex, l))
    except TypeError:
        raise

# batched call argument encoding

def escapearg(plain):
    return (plain
            .replace(':', ':c')
            .replace(',', ':o')
            .replace(';', ':s')
            .replace('=', ':e'))

def unescapearg(escaped):
    return (escaped
            .replace(':e', '=')
            .replace(':s', ';')
            .replace(':o', ',')
            .replace(':c', ':'))

# mapping of options accepted by getbundle and their types
#
# Meant to be extended by extensions. It is extensions responsibility to ensure
# such options are properly processed in exchange.getbundle.
#
# supported types are:
#
# :nodes: list of binary nodes
# :csv:   list of comma-separated values
# :scsv:  list of comma-separated values return as set
# :plain: string with no transformation needed.
gboptsmap = {'heads':  'nodes',
             'common': 'nodes',
             'obsmarkers': 'boolean',
             'bundlecaps': 'scsv',
             'listkeys': 'csv',
             'cg': 'boolean',
             'cbattempted': 'boolean'}

# client side

class wirepeer(peer.peerrepository):
    """Client-side interface for communicating with a peer repository.

    Methods commonly call wire protocol commands of the same name.

    See also httppeer.py and sshpeer.py for protocol-specific
    implementations of this interface.
    """
    def batch(self):
        if self.capable('batch'):
            return remotebatch(self)
        else:
            return peer.localbatch(self)
    def _submitbatch(self, req):
        """run batch request <req> on the server

        Returns an iterator of the raw responses from the server.
        """
        cmds = []
        for op, argsdict in req:
            args = ','.join('%s=%s' % (escapearg(k), escapearg(v))
                            for k, v in argsdict.iteritems())
            cmds.append('%s %s' % (op, args))
        rsp = self._callstream("batch", cmds=';'.join(cmds))
        # TODO this response parsing is probably suboptimal for large
        # batches with large responses.
        work = rsp.read(1024)
        chunk = work
        while chunk:
            while ';' in work:
                one, work = work.split(';', 1)
                yield unescapearg(one)
            chunk = rsp.read(1024)
            work += chunk
        yield unescapearg(work)

    def _submitone(self, op, args):
        return self._call(op, **args)

    def iterbatch(self):
        return remoteiterbatcher(self)

    @batchable
    def lookup(self, key):
        self.requirecap('lookup', _('look up remote revision'))
        f = future()
        yield {'key': encoding.fromlocal(key)}, f
        d = f.value
        success, data = d[:-1].split(" ", 1)
        if int(success):
            yield bin(data)
        self._abort(error.RepoError(data))

    @batchable
    def heads(self):
        f = future()
        yield {}, f
        d = f.value
        try:
            yield decodelist(d[:-1])
        except ValueError:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    @batchable
    def known(self, nodes):
        f = future()
        yield {'nodes': encodelist(nodes)}, f
        d = f.value
        try:
            yield [bool(int(b)) for b in d]
        except ValueError:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    @batchable
    def branchmap(self):
        f = future()
        yield {}, f
        d = f.value
        try:
            branchmap = {}
            for branchpart in d.splitlines():
                branchname, branchheads = branchpart.split(' ', 1)
                branchname = encoding.tolocal(urlreq.unquote(branchname))
                branchheads = decodelist(branchheads)
                branchmap[branchname] = branchheads
            yield branchmap
        except TypeError:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    def branches(self, nodes):
        n = encodelist(nodes)
        d = self._call("branches", nodes=n)
        try:
            br = [tuple(decodelist(b)) for b in d.splitlines()]
            return br
        except ValueError:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    def between(self, pairs):
        batch = 8 # avoid giant requests
        r = []
        for i in xrange(0, len(pairs), batch):
            n = " ".join([encodelist(p, '-') for p in pairs[i:i + batch]])
            d = self._call("between", pairs=n)
            try:
                r.extend(l and decodelist(l) or [] for l in d.splitlines())
            except ValueError:
                self._abort(error.ResponseError(_("unexpected response:"), d))
        return r

    @batchable
    def pushkey(self, namespace, key, old, new):
        if not self.capable('pushkey'):
            yield False, None
        f = future()
        self.ui.debug('preparing pushkey for "%s:%s"\n' % (namespace, key))
        yield {'namespace': encoding.fromlocal(namespace),
               'key': encoding.fromlocal(key),
               'old': encoding.fromlocal(old),
               'new': encoding.fromlocal(new)}, f
        d = f.value
        d, output = d.split('\n', 1)
        try:
            d = bool(int(d))
        except ValueError:
            raise error.ResponseError(
                _('push failed (unexpected response):'), d)
        for l in output.splitlines(True):
            self.ui.status(_('remote: '), l)
        yield d

    @batchable
    def listkeys(self, namespace):
        if not self.capable('pushkey'):
            yield {}, None
        f = future()
        self.ui.debug('preparing listkeys for "%s"\n' % namespace)
        yield {'namespace': encoding.fromlocal(namespace)}, f
        d = f.value
        self.ui.debug('received listkey for "%s": %i bytes\n'
                      % (namespace, len(d)))
        yield pushkeymod.decodekeys(d)

    def stream_out(self):
        return self._callstream('stream_out')

    def changegroup(self, nodes, kind):
        n = encodelist(nodes)
        f = self._callcompressable("changegroup", roots=n)
        return changegroupmod.cg1unpacker(f, 'UN')

    def changegroupsubset(self, bases, heads, kind):
        self.requirecap('changegroupsubset', _('look up remote changes'))
        bases = encodelist(bases)
        heads = encodelist(heads)
        f = self._callcompressable("changegroupsubset",
                                   bases=bases, heads=heads)
        return changegroupmod.cg1unpacker(f, 'UN')

    def getbundle(self, source, **kwargs):
        self.requirecap('getbundle', _('look up remote changes'))
        opts = {}
        bundlecaps = kwargs.get('bundlecaps')
        if bundlecaps is not None:
            kwargs['bundlecaps'] = sorted(bundlecaps)
        else:
            bundlecaps = () # kwargs could have it to None
        for key, value in kwargs.iteritems():
            if value is None:
                continue
            keytype = gboptsmap.get(key)
            if keytype is None:
                assert False, 'unexpected'
            elif keytype == 'nodes':
                value = encodelist(value)
            elif keytype in ('csv', 'scsv'):
                value = ','.join(value)
            elif keytype == 'boolean':
                value = '%i' % bool(value)
            elif keytype != 'plain':
                raise KeyError('unknown getbundle option type %s'
                               % keytype)
            opts[key] = value
        f = self._callcompressable("getbundle", **opts)
        if any((cap.startswith('HG2') for cap in bundlecaps)):
            return bundle2.getunbundler(self.ui, f)
        else:
            return changegroupmod.cg1unpacker(f, 'UN')

    def unbundle(self, cg, heads, source):
        '''Send cg (a readable file-like object representing the
        changegroup to push, typically a chunkbuffer object) to the
        remote server as a bundle.

        When pushing a bundle10 stream, return an integer indicating the
        result of the push (see localrepository.addchangegroup()).

        When pushing a bundle20 stream, return a bundle20 stream.'''

        if heads != ['force'] and self.capable('unbundlehash'):
            heads = encodelist(['hashed',
                                util.sha1(''.join(sorted(heads))).digest()])
        else:
            heads = encodelist(heads)

        if util.safehasattr(cg, 'deltaheader'):
            # this a bundle10, do the old style call sequence
            ret, output = self._callpush("unbundle", cg, heads=heads)
            if ret == "":
                raise error.ResponseError(
                    _('push failed:'), output)
            try:
                ret = int(ret)
            except ValueError:
                raise error.ResponseError(
                    _('push failed (unexpected response):'), ret)

            for l in output.splitlines(True):
                self.ui.status(_('remote: '), l)
        else:
            # bundle2 push. Send a stream, fetch a stream.
            stream = self._calltwowaystream('unbundle', cg, heads=heads)
            ret = bundle2.getunbundler(self.ui, stream)
        return ret

    def debugwireargs(self, one, two, three=None, four=None, five=None):
        # don't pass optional arguments left at their default value
        opts = {}
        if three is not None:
            opts['three'] = three
        if four is not None:
            opts['four'] = four
        return self._call('debugwireargs', one=one, two=two, **opts)

    def _call(self, cmd, **args):
        """execute <cmd> on the server

        The command is expected to return a simple string.

        returns the server reply as a string."""
        raise NotImplementedError()

    def _callstream(self, cmd, **args):
        """execute <cmd> on the server

        The command is expected to return a stream. Note that if the
        command doesn't return a stream, _callstream behaves
        differently for ssh and http peers.

        returns the server reply as a file like object.
        """
        raise NotImplementedError()

    def _callcompressable(self, cmd, **args):
        """execute <cmd> on the server

        The command is expected to return a stream.

        The stream may have been compressed in some implementations. This
        function takes care of the decompression. This is the only difference
        with _callstream.

        returns the server reply as a file like object.
        """
        raise NotImplementedError()

    def _callpush(self, cmd, fp, **args):
        """execute a <cmd> on server

        The command is expected to be related to a push. Push has a special
        return method.

        returns the server reply as a (ret, output) tuple. ret is either
        empty (error) or a stringified int.
        """
        raise NotImplementedError()

    def _calltwowaystream(self, cmd, fp, **args):
        """execute <cmd> on server

        The command will send a stream to the server and get a stream in reply.
        """
        raise NotImplementedError()

    def _abort(self, exception):
        """clearly abort the wire protocol connection and raise the exception
        """
        raise NotImplementedError()

# server side

# wire protocol command can either return a string or one of these classes.
class streamres(object):
    """wireproto reply: binary stream

    The call was successful and the result is a stream.
    Iterate on the `self.gen` attribute to retrieve chunks.
    """
    def __init__(self, gen):
        self.gen = gen

class pushres(object):
    """wireproto reply: success with simple integer return

    The call was successful and returned an integer contained in `self.res`.
    """
    def __init__(self, res):
        self.res = res

class pusherr(object):
    """wireproto reply: failure

    The call failed. The `self.res` attribute contains the error message.
    """
    def __init__(self, res):
        self.res = res

class ooberror(object):
    """wireproto reply: failure of a batch of operation

    Something failed during a batch call. The error message is stored in
    `self.message`.
    """
    def __init__(self, message):
        self.message = message

def dispatch(repo, proto, command):
    repo = repo.filtered("served")
    func, spec = commands[command]
    args = proto.getargs(spec)
    return func(repo, proto, *args)

def options(cmd, keys, others):
    opts = {}
    for k in keys:
        if k in others:
            opts[k] = others[k]
            del others[k]
    if others:
        sys.stderr.write("warning: %s ignored unexpected arguments %s\n"
                         % (cmd, ",".join(others)))
    return opts

def bundle1allowed(repo, action):
    """Whether a bundle1 operation is allowed from the server.

    Priority is:

    1. server.bundle1gd.<action> (if generaldelta active)
    2. server.bundle1.<action>
    3. server.bundle1gd (if generaldelta active)
    4. server.bundle1
    """
    ui = repo.ui
    gd = 'generaldelta' in repo.requirements

    if gd:
        v = ui.configbool('server', 'bundle1gd.%s' % action, None)
        if v is not None:
            return v

    v = ui.configbool('server', 'bundle1.%s' % action, None)
    if v is not None:
        return v

    if gd:
        v = ui.configbool('server', 'bundle1gd', None)
        if v is not None:
            return v

    return ui.configbool('server', 'bundle1', True)

# list of commands
commands = {}

def wireprotocommand(name, args=''):
    """decorator for wire protocol command"""
    def register(func):
        commands[name] = (func, args)
        return func
    return register

@wireprotocommand('batch', 'cmds *')
def batch(repo, proto, cmds, others):
    repo = repo.filtered("served")
    res = []
    for pair in cmds.split(';'):
        op, args = pair.split(' ', 1)
        vals = {}
        for a in args.split(','):
            if a:
                n, v = a.split('=')
                vals[n] = unescapearg(v)
        func, spec = commands[op]
        if spec:
            keys = spec.split()
            data = {}
            for k in keys:
                if k == '*':
                    star = {}
                    for key in vals.keys():
                        if key not in keys:
                            star[key] = vals[key]
                    data['*'] = star
                else:
                    data[k] = vals[k]
            result = func(repo, proto, *[data[k] for k in keys])
        else:
            result = func(repo, proto)
        if isinstance(result, ooberror):
            return result
        res.append(escapearg(result))
    return ';'.join(res)

@wireprotocommand('between', 'pairs')
def between(repo, proto, pairs):
    pairs = [decodelist(p, '-') for p in pairs.split(" ")]
    r = []
    for b in repo.between(pairs):
        r.append(encodelist(b) + "\n")
    return "".join(r)

@wireprotocommand('branchmap')
def branchmap(repo, proto):
    branchmap = repo.branchmap()
    heads = []
    for branch, nodes in branchmap.iteritems():
        branchname = urlreq.quote(encoding.fromlocal(branch))
        branchnodes = encodelist(nodes)
        heads.append('%s %s' % (branchname, branchnodes))
    return '\n'.join(heads)

@wireprotocommand('branches', 'nodes')
def branches(repo, proto, nodes):
    nodes = decodelist(nodes)
    r = []
    for b in repo.branches(nodes):
        r.append(encodelist(b) + "\n")
    return "".join(r)

@wireprotocommand('clonebundles', '')
def clonebundles(repo, proto):
    """Server command for returning info for available bundles to seed clones.

    Clients will parse this response and determine what bundle to fetch.

    Extensions may wrap this command to filter or dynamically emit data
    depending on the request. e.g. you could advertise URLs for the closest
    data center given the client's IP address.
    """
    return repo.opener.tryread('clonebundles.manifest')

wireprotocaps = ['lookup', 'changegroupsubset', 'branchmap', 'pushkey',
                 'known', 'getbundle', 'unbundlehash', 'batch']

def _capabilities(repo, proto):
    """return a list of capabilities for a repo

    This function exists to allow extensions to easily wrap capabilities
    computation

    - returns a lists: easy to alter
    - change done here will be propagated to both `capabilities` and `hello`
      command without any other action needed.
    """
    # copy to prevent modification of the global list
    caps = list(wireprotocaps)
    if streamclone.allowservergeneration(repo.ui):
        if repo.ui.configbool('server', 'preferuncompressed', False):
            caps.append('stream-preferred')
        requiredformats = repo.requirements & repo.supportedformats
        # if our local revlogs are just revlogv1, add 'stream' cap
        if not requiredformats - set(('revlogv1',)):
            caps.append('stream')
        # otherwise, add 'streamreqs' detailing our local revlog format
        else:
            caps.append('streamreqs=%s' % ','.join(sorted(requiredformats)))
    if repo.ui.configbool('experimental', 'bundle2-advertise', True):
        capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo))
        caps.append('bundle2=' + urlreq.quote(capsblob))
    caps.append('unbundle=%s' % ','.join(bundle2.bundlepriority))
    caps.append(
        'httpheader=%d' % repo.ui.configint('server', 'maxhttpheaderlen', 1024))
    if repo.ui.configbool('experimental', 'httppostargs', False):
        caps.append('httppostargs')
    return caps

# If you are writing an extension and consider wrapping this function. Wrap
# `_capabilities` instead.
@wireprotocommand('capabilities')
def capabilities(repo, proto):
    return ' '.join(_capabilities(repo, proto))

@wireprotocommand('changegroup', 'roots')
def changegroup(repo, proto, roots):
    nodes = decodelist(roots)
    cg = changegroupmod.changegroup(repo, nodes, 'serve')
    return streamres(proto.groupchunks(cg))

@wireprotocommand('changegroupsubset', 'bases heads')
def changegroupsubset(repo, proto, bases, heads):
    bases = decodelist(bases)
    heads = decodelist(heads)
    cg = changegroupmod.changegroupsubset(repo, bases, heads, 'serve')
    return streamres(proto.groupchunks(cg))

@wireprotocommand('debugwireargs', 'one two *')
def debugwireargs(repo, proto, one, two, others):
    # only accept optional args from the known set
    opts = options('debugwireargs', ['three', 'four'], others)
    return repo.debugwireargs(one, two, **opts)

# List of options accepted by getbundle.
#
# Meant to be extended by extensions. It is the extension's responsibility to
# ensure such options are properly processed in exchange.getbundle.
gboptslist = ['heads', 'common', 'bundlecaps']

@wireprotocommand('getbundle', '*')
def getbundle(repo, proto, others):
    opts = options('getbundle', gboptsmap.keys(), others)
    for k, v in opts.iteritems():
        keytype = gboptsmap[k]
        if keytype == 'nodes':
            opts[k] = decodelist(v)
        elif keytype == 'csv':
            opts[k] = list(v.split(','))
        elif keytype == 'scsv':
            opts[k] = set(v.split(','))
        elif keytype == 'boolean':
            # Client should serialize False as '0', which is a non-empty string
            # so it evaluates as a True bool.
            if v == '0':
                opts[k] = False
            else:
                opts[k] = bool(v)
        elif keytype != 'plain':
            raise KeyError('unknown getbundle option type %s'
                           % keytype)

    if not bundle1allowed(repo, 'pull'):
        if not exchange.bundle2requested(opts.get('bundlecaps')):
            return ooberror(bundle2required)

    cg = exchange.getbundle(repo, 'serve', **opts)
    return streamres(proto.groupchunks(cg))

@wireprotocommand('heads')
def heads(repo, proto):
    h = repo.heads()
    return encodelist(h) + "\n"

@wireprotocommand('hello')
def hello(repo, proto):
    '''the hello command returns a set of lines describing various
    interesting things about the server, in an RFC822-like format.
    Currently the only one defined is "capabilities", which
    consists of a line in the form:

    capabilities: space separated list of tokens
    '''
    return "capabilities: %s\n" % (capabilities(repo, proto))

@wireprotocommand('listkeys', 'namespace')
def listkeys(repo, proto, namespace):
    d = repo.listkeys(encoding.tolocal(namespace)).items()
    return pushkeymod.encodekeys(d)

@wireprotocommand('lookup', 'key')
def lookup(repo, proto, key):
    try:
        k = encoding.tolocal(key)
        c = repo[k]
        r = c.hex()
        success = 1
    except Exception as inst:
        r = str(inst)
        success = 0
    return "%s %s\n" % (success, r)

@wireprotocommand('known', 'nodes *')
def known(repo, proto, nodes, others):
    return ''.join(b and "1" or "0" for b in repo.known(decodelist(nodes)))

@wireprotocommand('pushkey', 'namespace key old new')
def pushkey(repo, proto, namespace, key, old, new):
    # compatibility with pre-1.8 clients which were accidentally
    # sending raw binary nodes rather than utf-8-encoded hex
    if len(new) == 20 and new.encode('string-escape') != new:
        # looks like it could be a binary node
        try:
            new.decode('utf-8')
            new = encoding.tolocal(new) # but cleanly decodes as UTF-8
        except UnicodeDecodeError:
            pass # binary, leave unmodified
    else:
        new = encoding.tolocal(new) # normal path

    if util.safehasattr(proto, 'restore'):

        proto.redirect()

        try:
            r = repo.pushkey(encoding.tolocal(namespace), encoding.tolocal(key),
                             encoding.tolocal(old), new) or False
        except error.Abort:
            r = False

        output = proto.restore()

        return '%s\n%s' % (int(r), output)

    r = repo.pushkey(encoding.tolocal(namespace), encoding.tolocal(key),
                     encoding.tolocal(old), new)
    return '%s\n' % int(r)

@wireprotocommand('stream_out')
def stream(repo, proto):
    '''If the server supports streaming clone, it advertises the "stream"
    capability with a value representing the version and flags of the repo
    it is serving. Client checks to see if it understands the format.
    '''
    if not streamclone.allowservergeneration(repo.ui):
        return '1\n'

    def getstream(it):
        yield '0\n'
        for chunk in it:
            yield chunk

    try:
        # LockError may be raised before the first result is yielded. Don't
        # emit output until we're sure we got the lock successfully.
        it = streamclone.generatev1wireproto(repo)
        return streamres(getstream(it))
    except error.LockError:
        return '2\n'

@wireprotocommand('unbundle', 'heads')
def unbundle(repo, proto, heads):
    their_heads = decodelist(heads)

    try:
        proto.redirect()

        exchange.check_heads(repo, their_heads, 'preparing changes')

        # write bundle data to temporary file because it can be big
        fd, tempname = tempfile.mkstemp(prefix='hg-unbundle-')
        fp = os.fdopen(fd, 'wb+')
        r = 0
        try:
            proto.getfile(fp)
            fp.seek(0)
            gen = exchange.readbundle(repo.ui, fp, None)
            if (isinstance(gen, changegroupmod.cg1unpacker)
                and not bundle1allowed(repo, 'push')):
                return ooberror(bundle2required)

            r = exchange.unbundle(repo, gen, their_heads, 'serve',
                                  proto._client())
            if util.safehasattr(r, 'addpart'):
                # The return looks streamable, we are in the bundle2 case and
                # should return a stream.
                return streamres(r.getchunks())
            return pushres(r)

        finally:
            fp.close()
            os.unlink(tempname)

    except (error.BundleValueError, error.Abort, error.PushRaced) as exc:
        # handle non-bundle2 case first
        if not getattr(exc, 'duringunbundle2', False):
            try:
                raise
            except error.Abort:
                # The old code we moved used sys.stderr directly.
                # We did not change it to minimise code change.
                # This need to be moved to something proper.
                # Feel free to do it.
                sys.stderr.write("abort: %s\n" % exc)
                return pushres(0)
            except error.PushRaced:
                return pusherr(str(exc))

        bundler = bundle2.bundle20(repo.ui)
        for out in getattr(exc, '_bundle2salvagedoutput', ()):
            bundler.addpart(out)
        try:
            try:
                raise
            except error.PushkeyFailed as exc:
                # check client caps
                remotecaps = getattr(exc, '_replycaps', None)
                if (remotecaps is not None
                        and 'pushkey' not in remotecaps.get('error', ())):
                    # no support remote side, fallback to Abort handler.
                    raise
                part = bundler.newpart('error:pushkey')
                part.addparam('in-reply-to', exc.partid)
                if exc.namespace is not None:
                    part.addparam('namespace', exc.namespace, mandatory=False)
                if exc.key is not None:
                    part.addparam('key', exc.key, mandatory=False)
                if exc.new is not None:
                    part.addparam('new', exc.new, mandatory=False)
                if exc.old is not None:
                    part.addparam('old', exc.old, mandatory=False)
                if exc.ret is not None:
                    part.addparam('ret', exc.ret, mandatory=False)
        except error.BundleValueError as exc:
            errpart = bundler.newpart('error:unsupportedcontent')
            if exc.parttype is not None:
                errpart.addparam('parttype', exc.parttype)
            if exc.params:
                errpart.addparam('params', '\0'.join(exc.params))
        except error.Abort as exc:
            manargs = [('message', str(exc))]
            advargs = []
            if exc.hint is not None:
                advargs.append(('hint', exc.hint))
            bundler.addpart(bundle2.bundlepart('error:abort',
                                               manargs, advargs))
        except error.PushRaced as exc:
            bundler.newpart('error:pushraced', [('message', str(exc))])
        return streamres(bundler.getchunks())
