# wireproto.py - generic wire protocol support functions
#
# Copyright 2005-2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import functools
import hashlib
import json
import os
import tempfile
import time

from . import (
    bundle2,
    changegroup as changegroupmod,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    peer,
    pushkey as pushkeymod,
    pycompat,
    replay,
    repository,
    streamclone,
    util,
)
from .i18n import _
from .node import bin, hex, nullid


urlerr = util.urlerr
urlreq = util.urlreq

bundle2requiredmain = _("incompatible Mercurial client; bundle2 required")
bundle2requiredhint = _("see https://www.mercurial-scm.org/wiki/" "IncompatibleClient")
bundle2required = "%s\n(%s)\n" % (bundle2requiredmain, bundle2requiredhint)

try:
    xrange(0)
except NameError:
    xrange = range


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
    # def restore(self):
    #    """reinstall previous stdout and stderr and return intercepted stdout
    #    """
    #    raise NotImplementedError()


class remoteiterbatcher(peer.iterbatcher):
    def __init__(self, remote):
        super(remoteiterbatcher, self).__init__()
        self._remote = remote

    def __getattr__(self, name):
        # Validate this method is batchable, since submit() only supports
        # batchable methods.
        fn = getattr(self._remote, name)
        if not getattr(fn, "batchable", None):
            raise error.ProgrammingError(
                "Attempted to batch a non-batchable " "call to %r" % name
            )

        return super(remoteiterbatcher, self).__getattr__(name)

    def submit(self):
        """Break the batch request into many patch calls and pipeline them.

        This is mostly valuable over http where request sizes can be
        limited, but can be used in other places as well.
        """
        # 2-tuple of (command, arguments) that represents what will be
        # sent over the wire.
        requests = []

        # 4-tuple of (command, final future, @batchable generator, remote
        # future).
        results = []

        for command, args, opts, finalfuture in self.calls:
            mtd = getattr(self._remote, command)
            batchable = mtd.batchable(mtd.__self__, *args, **opts)

            commandargs, fremote = next(batchable)
            assert fremote
            requests.append((command, commandargs))
            results.append((command, finalfuture, batchable, fremote))

        if requests:
            self._resultiter = self._remote._submitbatch(requests)

        self._results = results

    def results(self):
        for command, finalfuture, batchable, remotefuture in self._results:
            # Get the raw result, set it in the remote future, feed it
            # back into the @batchable generator so it can be decoded, and
            # set the result on the final future to this value.
            remoteresult = next(self._resultiter)
            remotefuture.set(remoteresult)
            finalfuture.set(next(batchable))

            # Verify our @batchable generators only emit 2 values.
            try:
                next(batchable)
            except StopIteration:
                pass
            else:
                raise error.ProgrammingError(
                    "%s @batchable generator emitted "
                    "unexpected value count" % command
                )

            yield finalfuture.value


# Forward a couple of names from peer to make wireproto interactions
# slightly more sensible.
batchable = peer.batchable
future = peer.future

# list of nodes encoding / decoding


def decodelist(l, sep=" "):
    if l:
        return [bin(v) for v in l.split(sep)]
    return []


def encodelist(l, sep=" "):
    try:
        return sep.join(map(hex, l))
    except TypeError:
        raise


# batched call argument encoding


def escapearg(plain):
    if isinstance(plain, bytearray):
        plain = bytes(plain)
    return (
        plain.replace(":", ":c")
        .replace(",", ":o")
        .replace(";", ":s")
        .replace("=", ":e")
    )


def unescapearg(escaped):
    return (
        escaped.replace(":e", "=")
        .replace(":s", ";")
        .replace(":o", ",")
        .replace(":c", ":")
    )


def encodebatchcmds(req):
    """Return a ``cmds`` argument value for the ``batch`` command."""
    cmds = []
    for op, argsdict in req:
        # Old servers didn't properly unescape argument names. So prevent
        # the sending of argument names that may not be decoded properly by
        # servers.
        assert all(escapearg(k) == k for k in argsdict)

        args = ",".join(
            "%s=%s" % (escapearg(k), escapearg(v)) for k, v in argsdict.iteritems()
        )
        cmds.append("%s %s" % (op, args))

    return ";".join(cmds)


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
gboptsmap = {
    "heads": "nodes",
    "bookmarks": "boolean",
    "common": "nodes",
    "obsmarkers": "boolean",
    "phases": "boolean",
    "bundlecaps": "scsv",
    "listkeys": "csv",
    "cg": "boolean",
    "cbattempted": "boolean",
}

# client side


class wirepeer(repository.legacypeer):
    """Client-side interface for communicating with a peer repository.

    Methods commonly call wire protocol commands of the same name.

    See also httppeer.py and sshpeer.py for protocol-specific
    implementations of this interface.
    """

    # Begin of basewirepeer interface.

    def iterbatch(self):
        return remoteiterbatcher(self)

    @batchable
    def lookup(self, key):
        self.requirecap("lookup", _("look up remote revision"))
        f = future()
        yield {"key": encoding.fromlocal(key)}, f
        d = f.value
        success, data = d[:-1].split(" ", 1)
        if int(success):
            yield bin(data)
        else:
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
        yield {"nodes": encodelist(nodes)}, f
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
                branchname, branchheads = branchpart.split(" ", 1)
                branchname = encoding.tolocal(urlreq.unquote(branchname))
                branchheads = decodelist(branchheads)
                branchmap[branchname] = branchheads
            yield branchmap
        except TypeError:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    @batchable
    def listkeys(self, namespace):
        if not self.capable("pushkey"):
            yield {}, None
        f = future()
        self.ui.debug('preparing listkeys for "%s"\n' % namespace)
        yield {"namespace": encoding.fromlocal(namespace)}, f
        d = f.value
        self.ui.debug('received listkey for "%s": %i bytes\n' % (namespace, len(d)))
        yield pushkeymod.decodekeys(d)

    @batchable
    def pushkey(self, namespace, key, old, new):
        if not self.capable("pushkey"):
            yield False, None
        f = future()
        self.ui.debug('preparing pushkey for "%s:%s"\n' % (namespace, key))
        yield {
            "namespace": encoding.fromlocal(namespace),
            "key": encoding.fromlocal(key),
            "old": encoding.fromlocal(old),
            "new": encoding.fromlocal(new),
        }, f
        d = f.value
        d, output = d.split("\n", 1)
        try:
            d = bool(int(d))
        except ValueError:
            raise error.ResponseError(_("push failed (unexpected response):"), d)
        for l in output.splitlines(True):
            self.ui.status(_("remote: "), l)
        yield d

    def stream_out(self):
        if self.capable("stream_option"):
            fullclone = self.ui.configbool("clone", "requestfullclone")
            args = pycompat.strkwargs({"fullclone": str(fullclone)})
            return self._callstream("stream_out_option", **args)
        return self._callstream("stream_out")

    def getbundle(self, source, **kwargs):
        kwargs = pycompat.byteskwargs(kwargs)
        self.requirecap("getbundle", _("look up remote changes"))
        opts = {}
        bundlecaps = kwargs.get("bundlecaps")
        if bundlecaps is not None:
            kwargs["bundlecaps"] = sorted(bundlecaps)
        else:
            bundlecaps = ()  # kwargs could have it to None
        for key, value in kwargs.iteritems():
            if value is None:
                continue
            keytype = gboptsmap.get(key)
            if keytype is None:
                raise error.ProgrammingError(
                    "Unexpectedly None keytype for key %s" % key
                )
            elif keytype == "nodes":
                value = encodelist(value)
            elif keytype in ("csv", "scsv"):
                value = ",".join(value)
            elif keytype == "boolean":
                value = "%i" % bool(value)
            elif keytype != "plain":
                raise KeyError("unknown getbundle option type %s" % keytype)
            opts[key] = value
        f = self._callcompressable("getbundle", **pycompat.strkwargs(opts))
        if any((cap.startswith("HG2") for cap in bundlecaps)):
            return bundle2.getunbundler(self.ui, f)
        else:
            return changegroupmod.cg1unpacker(f, "UN")

    def unbundle(self, cg, heads, url):
        """Send cg (a readable file-like object representing the
        changegroup to push, typically a chunkbuffer object) to the
        remote server as a bundle.

        When pushing a bundle10 stream, return an integer indicating the
        result of the push (see changegroup.apply()).

        When pushing a bundle20 stream, return a bundle20 stream.

        `url` is the url the client thinks it's pushing to, which is
        visible to hooks.
        """

        if heads != ["force"] and self.capable("unbundlehash"):
            heads = encodelist(
                ["hashed", hashlib.sha1("".join(sorted(heads))).digest()]
            )
        else:
            heads = encodelist(heads)

        if util.safehasattr(cg, "deltaheader"):
            # this a bundle10, do the old style call sequence
            ret, output = self._callpush("unbundle", cg, heads=heads)
            if ret == "":
                raise error.ResponseError(_("push failed:"), output)
            try:
                ret = int(ret)
            except ValueError:
                raise error.ResponseError(_("push failed (unexpected response):"), ret)

            for l in output.splitlines(True):
                self.ui.status(_("remote: "), l)
        else:
            # bundle2 push. Send a stream, fetch a stream.
            stream = self._calltwowaystream("unbundle", cg, heads=heads)
            ret = bundle2.getunbundler(self.ui, stream)
        return ret

    def unbundlereplay(self, cg, heads, url, replaydata, respondlightly):
        """Experimental command for replaying preserved bundles onto hg

        Intended to be used for mononoke->hg sync
        The idea is to send:
            - unbundle itself
            - a `replay.ReplayData` instance with override commit dates
              and expected resulting head hash
        """
        if heads != ["force"] and self.capable("unbundlehash"):
            heads = encodelist(
                ["hashed", hashlib.sha1("".join(sorted(heads))).digest()]
            )
        else:
            heads = encodelist(heads)

        respondlightly = "1" if respondlightly else "0"
        stream = self._calltwowaystream(
            "unbundlereplay",
            cg,
            heads=heads,
            replaydata=replaydata.serialize(),
            respondlightly=respondlightly,
        )
        ret = bundle2.getunbundler(self.ui, stream)
        return ret

    # End of basewirepeer interface.

    # Begin of baselegacywirepeer interface.

    def branches(self, nodes):
        n = encodelist(nodes)
        d = self._call("branches", nodes=n)
        try:
            br = [tuple(decodelist(b)) for b in d.splitlines()]
            return br
        except ValueError:
            self._abort(error.ResponseError(_("unexpected response:"), d))

    def between(self, pairs):
        batch = 8  # avoid giant requests
        r = []
        for i in xrange(0, len(pairs), batch):
            n = " ".join([encodelist(p, "-") for p in pairs[i : i + batch]])
            d = self._call("between", pairs=n)
            try:
                r.extend(l and decodelist(l) or [] for l in d.splitlines())
            except ValueError:
                self._abort(error.ResponseError(_("unexpected response:"), d))
        return r

    def changegroup(self, nodes, kind):
        n = encodelist(nodes)
        f = self._callcompressable("changegroup", roots=n)
        return changegroupmod.cg1unpacker(f, "UN")

    def changegroupsubset(self, bases, heads, kind):
        self.requirecap("changegroupsubset", _("look up remote changes"))
        bases = encodelist(bases)
        heads = encodelist(heads)
        f = self._callcompressable("changegroupsubset", bases=bases, heads=heads)
        return changegroupmod.cg1unpacker(f, "UN")

    # End of baselegacywirepeer interface.

    def _submitbatch(self, req):
        """run batch request <req> on the server

        Returns an iterator of the raw responses from the server.
        """
        rsp = self._callstream("batch", cmds=encodebatchcmds(req))
        chunk = rsp.read(1024)
        work = [chunk]
        while chunk:
            while ";" not in chunk and chunk:
                chunk = rsp.read(1024)
                work.append(chunk)
            merged = "".join(work)
            while ";" in merged:
                one, merged = merged.split(";", 1)
                yield unescapearg(one)
            chunk = rsp.read(1024)
            work = [merged, chunk]
        yield unescapearg("".join(work))

    def _submitone(self, op, args):
        return self._call(op, **pycompat.strkwargs(args))

    def debugwireargs(self, one, two, three=None, four=None, five=None):
        # don't pass optional arguments left at their default value
        opts = {}
        if three is not None:
            opts[r"three"] = three
        if four is not None:
            opts[r"four"] = four
        return self._call("debugwireargs", one=one, two=two, **opts)

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

    Accepts either a generator or an object with a ``read(size)`` method.

    ``v1compressible`` indicates whether this data can be compressed to
    "version 1" clients (technically: HTTP peers using
    application/mercurial-0.1 media type). This flag should NOT be used on
    new commands because new clients should support a more modern compression
    mechanism.
    """

    def __init__(self, gen=None, reader=None, v1compressible=False):
        self.gen = gen
        self.reader = reader
        self.v1compressible = v1compressible


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


def getdispatchrepo(repo, proto, command):
    """Obtain the repo used for processing wire protocol commands.

    The intent of this function is to serve as a monkeypatch point for
    extensions that need commands to operate on different repo views under
    specialized circumstances.
    """
    return repo.filtered("served")


def wrapstreamres(towrap, logger, start_time):
    if towrap.gen:
        gen = towrap.gen

        def logginggen():
            responselen = 0
            for chunk in gen:
                responselen += len(chunk)
                yield chunk
            duration = int((time.time() - start_time) * 1000)
            logger(duration=duration, responselen=responselen)

        towrap.gen = logginggen()
    else:
        towrap.reader.responselen = 0
        orig = towrap.reader.read

        def read(self, size):
            chunk = orig(size)
            self.reader.responselen += len(chunk)
            if not chunk:
                duration = int((time.time() - start_time) * 1000)
                logger(duration=duration, responselen=self.reader.responselen)
            return chunk


def logwireprotorequest(repo, ui, start_time, command, serializedargs, res):
    kwargs = {}
    try:
        clienttelemetry = extensions.find("clienttelemetry")
        kwargs = clienttelemetry.getclienttelemetry(repo)
    except KeyError:
        pass

    reponame = repo.ui.config("common", "reponame", "unknown")
    kwargs["reponame"] = reponame
    logger = functools.partial(
        ui.log, "wireproto_requests", "", command=command, args=serializedargs, **kwargs
    )
    duration = int((time.time() - start_time) * 1000)
    if isinstance(res, streamres):
        wrapstreamres(res, logger, start_time)
    elif isinstance(res, str):
        logger(duration=duration, responselen=len(res))
    elif isinstance(res, ooberror):
        logger(duration=duration, error=res.message)
    elif isinstance(res, pusherr):
        logger(duration=duration, error=str(res.res))
    elif isinstance(res, pushres):
        logger(duration=duration, responselen=len(str(res.res)))
    else:
        logger(duration=duration, error="unknown response")


def dispatch(repo, proto, command):
    repo = getdispatchrepo(repo, proto, command)
    func, spec = commands[command]
    args = proto.getargs(spec)

    try:
        serializedargs = json.dumps(args)
    except Exception:
        serializedargs = "Failed to serialize arguments"

    start_time = time.time()
    res = func(repo, proto, *args)

    logrequests = repo.ui.configlist("wireproto", "logrequests")
    if command in logrequests:
        ui = repo.ui
        try:
            logwireprotorequest(repo, ui, start_time, command, serializedargs, res)
        except Exception as e:
            # No logging error should break client-server interaction,
            # but let's warn about the problem
            ui.warn(_("error while logging wireproto request: %s") % e)
    return res


def options(cmd, keys, others):
    opts = {}
    for k in keys:
        if k in others:
            opts[k] = others[k]
            del others[k]
    if others:
        util.stderr.write(
            "warning: %s ignored unexpected arguments %s\n" % (cmd, ",".join(others))
        )
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
    gd = "generaldelta" in repo.requirements

    if gd:
        v = ui.configbool("server", "bundle1gd.%s" % action)
        if v is not None:
            return v

    v = ui.configbool("server", "bundle1.%s" % action)
    if v is not None:
        return v

    if gd:
        v = ui.configbool("server", "bundle1gd")
        if v is not None:
            return v

    return ui.configbool("server", "bundle1")


def supportedcompengines(ui, proto, role):
    """Obtain the list of supported compression engines for a request."""
    assert role in (util.CLIENTROLE, util.SERVERROLE)

    compengines = util.compengines.supportedwireengines(role)

    # Allow config to override default list and ordering.
    if role == util.SERVERROLE:
        configengines = ui.configlist("server", "compressionengines")
        config = "server.compressionengines"
    else:
        # This is currently implemented mainly to facilitate testing. In most
        # cases, the server should be in charge of choosing a compression engine
        # because a server has the most to lose from a sub-optimal choice. (e.g.
        # CPU DoS due to an expensive engine or a network DoS due to poor
        # compression ratio).
        configengines = ui.configlist("experimental", "clientcompressionengines")
        config = "experimental.clientcompressionengines"

    # No explicit config. Filter out the ones that aren't supposed to be
    # advertised and return default ordering.
    if not configengines:
        attr = "serverpriority" if role == util.SERVERROLE else "clientpriority"
        return [e for e in compengines if getattr(e.wireprotosupport(), attr) > 0]

    # If compression engines are listed in the config, assume there is a good
    # reason for it (like server operators wanting to achieve specific
    # performance characteristics). So fail fast if the config references
    # unusable compression engines.
    validnames = set(e.name() for e in compengines)
    invalidnames = set(e for e in configengines if e not in validnames)
    if invalidnames:
        raise error.Abort(
            _("invalid compression engine defined in %s: %s")
            % (config, ", ".join(sorted(invalidnames)))
        )

    compengines = [e for e in compengines if e.name() in configengines]
    compengines = sorted(compengines, key=lambda e: configengines.index(e.name()))

    if not compengines:
        raise error.Abort(
            _("%s config option does not specify any known " "compression engines")
            % config,
            hint=_("usable compression engines: %s") % ", ".sorted(validnames),
        )

    return compengines


# list of commands
commands = {}


def wireprotocommand(name, args=""):
    """decorator for wire protocol command"""

    def register(func):
        commands[name] = (func, args)
        return func

    return register


@wireprotocommand("batch", "cmds *")
def batch(repo, proto, cmds, others):
    repo = repo.filtered("served")
    res = []
    for pair in cmds.split(";"):
        op, args = pair.split(" ", 1)
        vals = {}
        for a in args.split(","):
            if a:
                n, v = a.split("=")
                vals[unescapearg(n)] = unescapearg(v)
        func, spec = commands[op]
        if spec:
            keys = spec.split()
            data = {}
            for k in keys:
                if k == "*":
                    star = {}
                    for key in vals.keys():
                        if key not in keys:
                            star[key] = vals[key]
                    data["*"] = star
                else:
                    data[k] = vals[k]
            result = func(repo, proto, *[data[k] for k in keys])
        else:
            result = func(repo, proto)
        if isinstance(result, ooberror):
            return result
        res.append(escapearg(result))
    return ";".join(res)


@wireprotocommand("between", "pairs")
def between(repo, proto, pairs):
    pairs = [decodelist(p, "-") for p in pairs.split(" ")]
    r = []
    for b in repo.between(pairs):
        r.append(encodelist(b) + "\n")
    return "".join(r)


@wireprotocommand("branchmap")
def branchmap(repo, proto):
    branchmap = repo.branchmap()
    heads = []
    for branch, nodes in branchmap.iteritems():
        branchname = urlreq.quote(encoding.fromlocal(branch))
        branchnodes = encodelist(nodes)
        heads.append("%s %s" % (branchname, branchnodes))
    return "\n".join(heads)


@wireprotocommand("branches", "nodes")
def branches(repo, proto, nodes):
    nodes = decodelist(nodes)
    r = []
    for b in repo.branches(nodes):
        r.append(encodelist(b) + "\n")
    return "".join(r)


@wireprotocommand("clonebundles", "")
def clonebundles(repo, proto):
    """Server command for returning info for available bundles to seed clones.

    Clients will parse this response and determine what bundle to fetch.

    Extensions may wrap this command to filter or dynamically emit data
    depending on the request. e.g. you could advertise URLs for the closest
    data center given the client's IP address.
    """
    return repo.localvfs.tryread("clonebundles.manifest")


wireprotocaps = [
    "lookup",
    "changegroupsubset",
    "branchmap",
    "pushkey",
    "known",
    "getbundle",
    "unbundlehash",
    "unbundlereplay",
    "batch",
]


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
    if streamclone.allowservergeneration(repo):
        if repo.ui.configbool("server", "preferuncompressed"):
            caps.append("stream-preferred")
        requiredformats = repo.requirements & repo.supportedformats
        # if our local revlogs are just revlogv1, add 'stream' cap
        if not requiredformats - {"revlogv1"}:
            caps.append("stream")
        # otherwise, add 'streamreqs' detailing our local revlog format
        else:
            caps.append("streamreqs=%s" % ",".join(sorted(requiredformats)))
        caps.append("stream_option")
    if repo.ui.configbool("experimental", "bundle2-advertise"):
        capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo))
        caps.append("bundle2=" + urlreq.quote(capsblob))
    caps.append("unbundle=%s" % ",".join(bundle2.bundlepriority))

    if proto.name == "http":
        caps.append("httpheader=%d" % repo.ui.configint("server", "maxhttpheaderlen"))
        if repo.ui.configbool("experimental", "httppostargs"):
            caps.append("httppostargs")

        # FUTURE advertise 0.2rx once support is implemented
        # FUTURE advertise minrx and mintx after consulting config option
        caps.append("httpmediatype=0.1rx,0.1tx,0.2tx")

        compengines = supportedcompengines(repo.ui, proto, util.SERVERROLE)
        if compengines:
            comptypes = ",".join(
                urlreq.quote(e.wireprotosupport().name) for e in compengines
            )
            caps.append("compression=%s" % comptypes)

    return caps


# If you are writing an extension and consider wrapping this function. Wrap
# `_capabilities` instead.
@wireprotocommand("capabilities")
def capabilities(repo, proto):
    return " ".join(_capabilities(repo, proto))


@wireprotocommand("changegroup", "roots")
def changegroup(repo, proto, roots):
    nodes = decodelist(roots)
    outgoing = discovery.outgoing(repo, missingroots=nodes, missingheads=repo.heads())
    cg = changegroupmod.makechangegroup(repo, outgoing, "01", "serve")
    return streamres(reader=cg, v1compressible=True)


@wireprotocommand("changegroupsubset", "bases heads")
def changegroupsubset(repo, proto, bases, heads):
    bases = decodelist(bases)
    heads = decodelist(heads)
    outgoing = discovery.outgoing(repo, missingroots=bases, missingheads=heads)
    cg = changegroupmod.makechangegroup(repo, outgoing, "01", "serve")
    return streamres(reader=cg, v1compressible=True)


@wireprotocommand("debugwireargs", "one two *")
def debugwireargs(repo, proto, one, two, others):
    # only accept optional args from the known set
    opts = options("debugwireargs", ["three", "four"], others)
    return repo.debugwireargs(one, two, **pycompat.strkwargs(opts))


@wireprotocommand("getbundle", "*")
def getbundle(repo, proto, others):
    opts = options("getbundle", gboptsmap.keys(), others)
    for k, v in opts.iteritems():
        keytype = gboptsmap[k]
        if keytype == "nodes":
            opts[k] = decodelist(v)
        elif keytype == "csv":
            opts[k] = list(v.split(","))
        elif keytype == "scsv":
            opts[k] = set(v.split(","))
        elif keytype == "boolean":
            # Client should serialize False as '0', which is a non-empty string
            # so it evaluates as a True bool.
            if v == "0":
                opts[k] = False
            else:
                opts[k] = bool(v)
        elif keytype != "plain":
            raise KeyError("unknown getbundle option type %s" % keytype)

    if not bundle1allowed(repo, "pull"):
        if not exchange.bundle2requested(opts.get("bundlecaps")):
            if proto.name == "http":
                return ooberror(bundle2required)
            raise error.Abort(bundle2requiredmain, hint=bundle2requiredhint)

    try:
        if repo.ui.configbool("server", "disablefullbundle"):
            # Check to see if this is a full clone.
            clheads = set(repo.changelog.heads())
            heads = set(opts.get("heads", set()))
            common = set(opts.get("common", set()))
            common.discard(nullid)
            if not common and clheads == heads:
                raise error.Abort(
                    _("server has pull-based clones disabled"),
                    hint=_("remove --pull if specified or upgrade Mercurial"),
                )

        chunks = exchange.getbundlechunks(repo, "serve", **pycompat.strkwargs(opts))
    except error.Abort as exc:
        # cleanly forward Abort error to the client
        if not exchange.bundle2requested(opts.get("bundlecaps")):
            if proto.name == "http":
                return ooberror(str(exc) + "\n")
            raise  # cannot do better for bundle1 + ssh
        # bundle2 request expect a bundle2 reply
        bundler = bundle2.bundle20(repo.ui)
        bundler.addpart(bundle2.createerrorpart(str(exc), hint=exc.hint))
        return streamres(gen=bundler.getchunks(), v1compressible=True)
    return streamres(gen=chunks, v1compressible=True)


@wireprotocommand("heads")
def heads(repo, proto):
    h = repo.heads()
    return encodelist(h) + "\n"


@wireprotocommand("hello")
def hello(repo, proto):
    """the hello command returns a set of lines describing various
    interesting things about the server, in an RFC822-like format.
    Currently the only one defined is "capabilities", which
    consists of a line in the form:

    capabilities: space separated list of tokens
    """
    return "capabilities: %s\n" % (capabilities(repo, proto))


@wireprotocommand("listkeys", "namespace")
def listkeys(repo, proto, namespace):
    d = repo.listkeys(encoding.tolocal(namespace)).items()
    return pushkeymod.encodekeys(d)


@wireprotocommand("lookup", "key")
def lookup(repo, proto, key):
    try:
        k = encoding.tolocal(key)
        c = repo[k]
        r = c.hex()
        success = 1
    except Exception as inst:
        r = str(inst)
        success = 0
    return "%d %s\n" % (success, r)


@wireprotocommand("known", "nodes *")
def known(repo, proto, nodes, others):
    return "".join(b and "1" or "0" for b in repo.known(decodelist(nodes)))


@wireprotocommand("pushkey", "namespace key old new")
def pushkey(repo, proto, namespace, key, old, new):
    # compatibility with pre-1.8 clients which were accidentally
    # sending raw binary nodes rather than utf-8-encoded hex
    if len(new) == 20 and util.escapestr(new) != new:
        # looks like it could be a binary node
        try:
            new.decode("utf-8")
            new = encoding.tolocal(new)  # but cleanly decodes as UTF-8
        except UnicodeDecodeError:
            pass  # binary, leave unmodified
    else:
        new = encoding.tolocal(new)  # normal path

    if util.safehasattr(proto, "restore"):

        proto.redirect()

        try:
            r = (
                repo.pushkey(
                    encoding.tolocal(namespace),
                    encoding.tolocal(key),
                    encoding.tolocal(old),
                    new,
                )
                or False
            )
        except error.Abort:
            r = False

        output = proto.restore()

        return "%s\n%s" % (int(r), output)

    r = repo.pushkey(
        encoding.tolocal(namespace), encoding.tolocal(key), encoding.tolocal(old), new
    )
    return "%s\n" % int(r)


@wireprotocommand("stream_out")
def stream(repo, proto):
    """If the server supports streaming clone, it advertises the "stream"
    capability with a value representing the version and flags of the repo
    it is serving. Client checks to see if it understands the format.
    """
    if not streamclone.allowservergeneration(repo):
        return "1\n"

    def getstream(it):
        yield "0\n"
        for chunk in it:
            yield chunk

    try:
        # LockError may be raised before the first result is yielded. Don't
        # emit output until we're sure we got the lock successfully.
        it = streamclone.generatev1wireproto(repo)
        return streamres(gen=getstream(it))
    except error.LockError:
        return "2\n"


@wireprotocommand("stream_out_option", "*")
def streamoption(repo, proto, options):
    if (
        repo.ui.configbool("server", "requireexplicitfullclone")
        and options.get("fullclone", False) != "True"
    ):
        # For large repositories, we want to block accidental full clones.
        repo.ui.warn(
            _(
                "unable to perform an implicit streaming clone - "
                "make sure remotefilelog is enabled\n"
            )
        )
        return "1\n"

    return stream(repo, proto)


@wireprotocommand("unbundlereplay", "heads replaydata respondlightly")
def unbundlereplay(repo, proto, heads, replaydata, respondlightly):
    """Replay the unbunlde content and check if the result matches the
    expectations, supplied in the `replaydata`

    Note that the goal is to apply the bundle exactly as we
    captured it on the wire + change commit timestamps, therefore
    I don't want to add any parts to the unbundle, so
    additional data (timestamps, expected hashes) needs to
    go on wireprotocol args.
    """
    proto.redirect()
    replaydata = replay.ReplayData.deserialize(replaydata)
    respondlightly = True if respondlightly == "1" else False
    res = unbundleimpl(
        repo, proto, heads, replaydata=replaydata, respondlightly=respondlightly
    )
    return res


@wireprotocommand("unbundle", "heads")
def unbundle(repo, proto, heads):
    return unbundleimpl(repo, proto, heads)


def unbundleimpl(repo, proto, heads, replaydata=None, respondlightly=False):
    their_heads = decodelist(heads)

    try:
        proto.redirect()

        exchange.check_heads(repo, their_heads, "preparing changes")

        # write bundle data to temporary file because it can be big
        fd, tempname = tempfile.mkstemp(prefix="hg-unbundle-")
        # Make the file available to other extensions.
        # See pushrebase recording for example
        repo.unbundlefile = tempname
        fp = util.fdopen(fd, pycompat.sysstr("wb+"))
        r = 0
        try:
            proto.getfile(fp)
            fp.seek(0)
            gen = exchange.readbundle(repo.ui, fp, None)
            if isinstance(gen, changegroupmod.cg1unpacker) and not bundle1allowed(
                repo, "push"
            ):
                if proto.name == "http":
                    # need to special case http because stderr do not get to
                    # the http client on failed push so we need to abuse some
                    # other error type to make sure the message get to the
                    # user.
                    return ooberror(bundle2required)
                raise error.Abort(bundle2requiredmain, hint=bundle2requiredhint)

            r = exchange.unbundle(
                repo,
                gen,
                their_heads,
                "serve",
                proto._client(),
                replaydata=replaydata,
                respondlightly=respondlightly,
            )
            if util.safehasattr(r, "addpart"):
                # The return looks streamable, we are in the bundle2 case and
                # should return a stream.
                return streamres(gen=r.getchunks())
            return pushres(r)

        finally:
            fp.close()
            os.unlink(tempname)

    except (error.BundleValueError, error.Abort, error.PushRaced) as exc:
        # handle non-bundle2 case first
        if not getattr(exc, "duringunbundle2", False):
            try:
                raise
            except error.Abort:
                # The old code we moved used util.stderr directly.
                # We did not change it to minimise code change.
                # This need to be moved to something proper.
                # Feel free to do it.
                util.stderr.write("abort: %s\n" % exc)
                if exc.hint is not None:
                    util.stderr.write("(%s)\n" % exc.hint)
                return pushres(0)
            except error.PushRaced:
                return pusherr(str(exc))

        bundler = bundle2.bundle20(repo.ui)
        for out in getattr(exc, "_bundle2salvagedoutput", ()):
            bundler.addpart(out)
        try:
            try:
                raise
            except error.PushkeyFailed as exc:
                # check client caps
                remotecaps = getattr(exc, "_replycaps", None)
                if remotecaps is not None and "pushkey" not in remotecaps.get(
                    "error", ()
                ):
                    # no support remote side, fallback to Abort handler.
                    raise
                part = bundler.newpart("error:pushkey")
                part.addparam("in-reply-to", exc.partid)
                if exc.namespace is not None:
                    part.addparam("namespace", exc.namespace, mandatory=False)
                if exc.key is not None:
                    part.addparam("key", exc.key, mandatory=False)
                if exc.new is not None:
                    part.addparam("new", exc.new, mandatory=False)
                if exc.old is not None:
                    part.addparam("old", exc.old, mandatory=False)
                if exc.ret is not None:
                    part.addparam("ret", exc.ret, mandatory=False)
        except error.BundleValueError as exc:
            errpart = bundler.newpart("error:unsupportedcontent")
            if exc.parttype is not None:
                errpart.addparam("parttype", exc.parttype)
            if exc.params:
                errpart.addparam("params", "\0".join(exc.params))
        except error.Abort as exc:
            bundler.addpart(bundle2.createerrorpart(str(exc), hint=exc.hint))
        except error.PushRaced as exc:
            bundler.newpart("error:pushraced", [("message", str(exc))])
        return streamres(gen=bundler.getchunks())
