# gitlookup.py - server-side support for hg->git and git->hg lookups
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

""" extension that will look up hashes from an hg-git map file over the wire.
    This also provides client and server commands to download all the Git
    metadata via bundle2. Example usage:

    - get the git equivalent of hg 47d743e068523a9346a5ea4e429eeab185c886c6

        hg identify --id -r\\
            _gitlookup_hg_47d743e068523a9346a5ea4e429eeab185c886c6\\
            ssh://server/repo

    - get the hg equivalent of git 6916a3c30f53878032dea8d01074d8c2a03927bd

        hg identify --id -r\\
            _gitlookup_git_6916a3c30f53878032dea8d01074d8c2a03927bd\\
            ssh://server/repo

::

    [gitlookup]
    # Define the location of the map file with the mapfile config option.
    mapfile = <location of map file>

    # The config option onlymapdelta controls how the server handles the hg-git
    # map. A True value corresponds to serving only missing map data while False
    # corresponds to serving the complete map.
    onlymapdelta = False

"""

import errno
import json

from edenscm.mercurial import (
    bundle2,
    encoding,
    error,
    exchange,
    extensions,
    hg,
    localrepo,
    registrar,
    util,
    wireproto,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin, hex, nullid


cmdtable = {}
command = registrar.command(cmdtable)


def wrapwireprotocommand(command, wrapper):
    """Wrap the wire proto command named `command' in table

    Just like extensions.wrapcommand, except for wire protocol commands.
    """
    assert util.safehasattr(wrapper, "__call__")
    origfn, args = wireproto.commands[command]

    def wrap(*args, **kwargs):
        return util.checksignature(wrapper)(
            util.checksignature(origfn), *args, **kwargs
        )

    wireproto.commands[command] = wrap, args
    return wrapper


def remotelookup(orig, repo, proto, key):
    k = encoding.tolocal(key)
    if k.startswith("_gitlookup_"):
        ret = _dolookup(repo, k)
        if ret is not None:
            success = 1
        else:
            success = 0
            ret = "gitlookup failed"
        return "%s %s\n" % (success, ret)
    return orig(repo, proto, key)


def locallookup(orig, repo, key):
    gitlookup = _dolookup(repo, key)
    if gitlookup:
        return bin(gitlookup)
    else:
        return orig(repo, key)


def _dolookup(repo, key):
    mapfile = repo.ui.configpath("gitlookup", "mapfile")
    if mapfile is None:
        return None
    if not isinstance(key, str):
        return None
    # direction: git to hg = g, hg to git = h
    if key.startswith("_gitlookup_git_"):
        direction = "tohg"
        sha = key[15:]
    elif key.startswith("_gitlookup_hg_"):
        direction = "togit"
        sha = key[14:]
    else:
        return None
    if direction == "togit":
        # we've started recording the hg hash in extras.
        try:
            ctx = repo[sha]
        except error.RepoLookupError as e:
            if "unknown revision" in str(e):
                return None
            raise e
        fromextra = ctx.extra().get("convert_revision", "")
        if fromextra:
            return fromextra
    hggitmap = open(mapfile, "rb")
    for line in hggitmap:
        gitsha, hgsha = line.strip().split(" ", 1)
        if direction == "tohg" and sha == gitsha:
            return hgsha
        if direction == "togit" and sha == hgsha:
            return gitsha
    return None


@command("gitgetmeta", [], "[SOURCE]")
def gitgetmeta(ui, repo, source="default"):
    """get git metadata from a server that supports fb_gitmeta"""
    source, branch = hg.parseurl(ui.expandpath(source))
    other = hg.peer(repo, {}, source)
    ui.status(_("getting git metadata from %s\n") % util.hidepassword(source))

    kwargs = {"bundlecaps": exchange.caps20to10(repo)}
    capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo))
    kwargs["bundlecaps"].add("bundle2=" + util.urlreq.quote(capsblob))
    # this would ideally not be in the bundlecaps at all, but adding new kwargs
    # for wire transmissions is not possible as of Mercurial d19164a018a1
    kwargs["bundlecaps"].add("fb_gitmeta")
    kwargs["heads"] = [nullid]
    kwargs["cg"] = False
    kwargs["common"] = _getcommonheads(repo)
    bundle = other.getbundle("pull", **kwargs)
    try:
        op = bundle2.processbundle(repo, bundle)
    except error.BundleValueError as exc:
        raise error.Abort("missing support for %s" % exc)
    writebytes = op.records["fb:gitmeta:writebytes"]
    ui.status(_("wrote %d files (%d bytes)\n") % (len(writebytes), sum(writebytes)))


hgheadsfile = "git-synced-hgheads"
gitmapfile = "git-mapfile"
gitmetafiles = set([gitmapfile, "git-named-branches", "git-tags", "git-remote-refs"])


def _getfile(repo, filename):
    try:
        return repo.localvfs(filename)
    except (IOError, OSError) as e:
        if e.errno != errno.ENOENT:
            repo.ui.warn(_("warning: unable to read %s: %s\n") % (filename, e))

    return None


def _getcommonheads(repo):
    commonheads = []
    f = _getfile(repo, hgheadsfile)
    if f:
        commonheads = f.readlines()
        commonheads = [bin(x.strip()) for x in commonheads]
    return commonheads


def _isheadmissing(repo, heads):
    return not all(repo.known(heads))


def _getmissinglines(mapfile, missinghashes):
    missinglines = set()

    # Avoid expensive lookup through the map file if there is no missing hash.
    if not missinghashes:
        return missinglines

    linelen = 82
    hashestofind = missinghashes.copy()
    content = mapfile.read()
    if len(content) % linelen != 0:
        raise error.Abort(_("gitmeta: invalid mapfile length (%s)") % len(content))

    # Walk backwards through the map file, since recent commits are added at the
    # end.
    count = len(content) / linelen
    for i in range(count - 1, -1, -1):
        offset = i * linelen
        line = content[offset : offset + linelen]
        hgsha = line[41:81]
        if hgsha in hashestofind:
            missinglines.add(line)

            # Return the missing lines if we found all of them.
            hashestofind.remove(hgsha)
            if not hashestofind:
                return missinglines

    raise error.Abort(_("gitmeta: missing hashes in file %s") % mapfile.name)


class _githgmappayload(object):
    def __init__(self, needfullsync, newheads, missinglines):
        self.needfullsync = needfullsync
        self.newheads = newheads
        self.missinglines = missinglines

    def _todict(self):
        d = {}
        d["needfullsync"] = self.needfullsync
        d["newheads"] = list(self.newheads)
        d["missinglines"] = list(self.missinglines)
        return d

    def tojson(self):
        return json.dumps(self._todict())

    @classmethod
    def _fromdict(cls, d):
        needfullsync = d["needfullsync"]
        newheads = set(d["newheads"])
        missinglines = set(d["missinglines"])
        return cls(needfullsync, newheads, missinglines)

    @classmethod
    def fromjson(cls, jsonstr):
        d = json.loads(jsonstr)
        return cls._fromdict(d)


@exchange.getbundle2partsgenerator("b2x:fb:gitmeta:githgmap")
def _getbundlegithgmappart(bundler, repo, source, bundlecaps=None, **kwargs):
    """send missing git to hg map data via bundle2"""
    if "fb_gitmeta" in bundlecaps:
        # Do nothing if the config indicates serving the complete git-hg map
        # file. _getbundlegitmetapart will handle serving the complete file in
        # this case.
        if not repo.ui.configbool("gitlookup", "onlymapdelta", False):
            return

        mapfile = _getfile(repo, gitmapfile)
        if not mapfile:
            return

        commonheads = kwargs["common"]

        # If there are missing heads, we will sync everything.
        if _isheadmissing(repo, commonheads):
            commonheads = []

        needfullsync = len(commonheads) == 0

        heads = repo.heads()
        newheads = set(hex(head) for head in heads)

        missingcommits = repo.changelog.findmissing(commonheads, heads)
        missinghashes = set(hex(commit) for commit in missingcommits)
        missinglines = _getmissinglines(mapfile, missinghashes)

        payload = _githgmappayload(needfullsync, newheads, missinglines)
        serializedpayload = payload.tojson()
        part = bundle2.bundlepart(
            "b2x:fb:gitmeta:githgmap",
            [("filename", gitmapfile)],
            data=serializedpayload,
        )

        bundler.addpart(part)


@exchange.getbundle2partsgenerator("b2x:fb:gitmeta")
def _getbundlegitmetapart(bundler, repo, source, bundlecaps=None, **kwargs):
    """send git metadata via bundle2"""
    if "fb_gitmeta" in bundlecaps:
        filestooverwrite = gitmetafiles

        # Exclude the git-hg map file if the config indicates that the server
        # should only be serving the missing map data. _getbundle2partsgenerator
        # will serve the missing map data in this case.
        if repo.ui.configbool("gitlookup", "onlymapdelta", False):
            filestooverwrite = filestooverwrite - set([gitmapfile])

        for fname in sorted(filestooverwrite):
            f = _getfile(repo, fname)
            if not f:
                continue

            part = bundle2.bundlepart(
                "b2x:fb:gitmeta", [("filename", fname)], data=f.read()
            )
            bundler.addpart(part)


def _writefile(op, filename, data):
    with op.repo.localvfs(filename, "w+", atomictemp=True) as f:
        op.repo.ui.note(_("writing .hg/%s\n") % filename)
        f.write(data)
        op.records.add("fb:gitmeta:writebytes", len(data))


def _validatepartparams(op, params):
    if "filename" not in params:
        raise error.Abort(_("gitmeta: 'filename' missing"))

    fname = params["filename"]
    if fname not in gitmetafiles:
        op.repo.ui.warn(_("warning: gitmeta: unknown file '%s' skipped\n") % fname)
        return False

    return True


@bundle2.parthandler("b2x:fb:gitmeta:githgmap", ("filename",))
@bundle2.parthandler("fb:gitmeta:githgmap", ("filename",))
def bundle2getgithgmap(op, part):
    params = dict(part.mandatoryparams)
    if _validatepartparams(op, params):
        filename = params["filename"]
        with op.repo.wlock():
            data = _githgmappayload.fromjson(part.read())
            missinglines = data.missinglines

            # No need to update anything if already in sync.
            if not missinglines:
                return

            if data.needfullsync:
                newlines = missinglines
            else:
                mapfile = _getfile(op.repo, filename)
                if mapfile:
                    currentlines = set(mapfile.readlines())
                    if currentlines & missinglines:
                        msg = "warning: gitmeta: unexpected lines in .hg/%s\n"
                        op.repo.ui.warn(_(msg) % filename)

                    currentlines.update(missinglines)
                    newlines = currentlines
                else:
                    raise error.Abort(
                        _("gitmeta: could not read from .hg/%s") % filename
                    )

            _writefile(op, filename, "".join(newlines))
            _writefile(op, hgheadsfile, "\n".join(data.newheads))


@bundle2.parthandler("b2x:fb:gitmeta", ("filename",))
@bundle2.parthandler("fb:gitmeta", ("filename",))
def bundle2getgitmeta(op, part):
    """unbundle a bundle2 containing git metadata on the client"""
    params = dict(part.mandatoryparams)
    if _validatepartparams(op, params):
        filename = params["filename"]
        with op.repo.wlock():
            data = part.read()
            _writefile(op, filename, data)


def extsetup(ui):
    wrapwireprotocommand("lookup", remotelookup)
    extensions.wrapfunction(localrepo.localrepository, "lookup", locallookup)
