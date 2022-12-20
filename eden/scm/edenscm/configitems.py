# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# configitems.py - centralized declaration of configuration option
#
#  Copyright 2017 Pierre-Yves David <pierre-yves.david@octobus.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import functools
import re

from . import encoding, error, util


def loadconfigtable(ui, extname, configtable):
    """update config item known to the ui with the extension ones"""
    for section, items in configtable.items():
        knownitems = ui.uiconfig()._knownconfig.setdefault(section, itemregister())
        knownkeys = set(knownitems)
        newkeys = set(items)
        for key in sorted(knownkeys & newkeys):
            msg = "extension '%s' overwrite config item '%s.%s'"
            msg %= (extname, section, key)
            ui.develwarn(msg, config="warn-config")

        knownitems.update(items)


class configitem(object):
    """represent a known config item

    :section: the official config section where to find this item,
       :name: the official name within the section,
    :default: default value for this item,
    :alias: optional list of tuples as alternatives,
    :generic: this is a generic definition, match name using regular expression.
    """

    def __init__(
        self, section, name, default=None, alias=(), generic=False, priority=0
    ):
        self.section = section
        self.name = name
        self.default = default
        self.alias = list(alias)
        self.generic = generic
        self.priority = priority
        self._re = None
        if generic:
            self._re = re.compile(self.name)


class itemregister(dict):
    """A specialized dictionary that can handle wild-card selection"""

    def __init__(self):
        super(itemregister, self).__init__()
        self._generics = set()

    def update(self, other):
        super(itemregister, self).update(other)
        self._generics.update(other._generics)

    def __setitem__(self, key, item):
        super(itemregister, self).__setitem__(key, item)
        if item.generic:
            self._generics.add(item)

    def get(self, key):
        baseitem = super(itemregister, self).get(key)
        if baseitem is not None and not baseitem.generic:
            return baseitem

        # search for a matching generic item
        generics = sorted(self._generics, key=(lambda x: (x.priority, x.name)))
        for item in generics:
            # we use 'match' instead of 'search' to make the matching simpler
            # for people unfamiliar with regular expression. Having the match
            # rooted to the start of the string will produce less surprising
            # result for user writing simple regex for sub-attribute.
            #
            # For example using "color\..*" match produces an unsurprising
            # result, while using search could suddenly match apparently
            # unrelated configuration that happens to contains "color."
            # anywhere. This is a tradeoff where we favor requiring ".*" on
            # some match to avoid the need to prefix most pattern with "^".
            # The "^" seems more error prone.
            if item._re.match(key):
                return item

        return None


coreitems = {}


def _register(configtable, *args, **kwargs):
    item = configitem(*args, **kwargs)
    section = configtable.setdefault(item.section, itemregister())
    if item.name in section:
        msg = "duplicated config item registration for '%s.%s'"
        raise error.ProgrammingError(msg % (item.section, item.name))
    section[item.name] = item


# special value for case where the default is derived from other values
dynamicdefault = object()

# Registering actual config items


def getitemregister(configtable):
    f = functools.partial(_register, configtable)
    # export pseudo enum as configitem.*
    f.dynamicdefault = dynamicdefault
    return f


coreconfigitem = getitemregister(coreitems)

coreconfigitem("alias", ".*", default=None, generic=True)
coreconfigitem("annotate", "nodates", default=False)
coreconfigitem("annotate", "showfunc", default=False)
coreconfigitem("annotate", "unified", default=None)
coreconfigitem("annotate", "git", default=False)
coreconfigitem("annotate", "ignorews", default=False)
coreconfigitem("annotate", "ignorewsamount", default=False)
coreconfigitem("annotate", "ignoreblanklines", default=False)
coreconfigitem("annotate", "ignorewseol", default=False)
coreconfigitem("annotate", "nobinary", default=False)
coreconfigitem("annotate", "noprefix", default=False)
coreconfigitem("auth", "cookiefile", default=None)
coreconfigitem("auth_proxy", "unix_socket_path", default=None)
coreconfigitem("blackbox", "maxsize", default="100 MB")
coreconfigitem("blackbox", "maxfiles", default=3)
# bookmarks.pushing: internal hack for discovery
coreconfigitem("bookmarks", "pushing", default=list)
# bundle.mainreporoot: internal hack for bundlerepo
coreconfigitem("bundle", "mainreporoot", default="")
coreconfigitem("bundle2", "rechunkthreshold", default="1MB")
# bundle.reorder: experimental config
coreconfigitem("bundle", "reorder", default="auto")
coreconfigitem("censor", "policy", default="abort")
coreconfigitem("chgserver", "idletimeout", default=3600)
coreconfigitem("chgserver", "skiphash", default=False)
coreconfigitem("clone", "prefer-edenapi-clonedata", default=True)
coreconfigitem("clone", "nativepull", default=False)
coreconfigitem("cmdserver", "log", default=None)
coreconfigitem("color", ".*", default=None, generic=True)
coreconfigitem("commands", "show.aliasprefix", default=list)
coreconfigitem("commands", "status.relative", default=False)
coreconfigitem("commands", "status.skipstates", default=[])
coreconfigitem("commands", "status.verbose", default=False)
coreconfigitem(
    "commands",
    "update.check",
    default=None,
    # Deprecated, remove after 4.4 release
    alias=[("experimental", "updatecheck")],
)
coreconfigitem("commands", "update.requiredest", default=False)
coreconfigitem("commands", "new-pull", default=True)
coreconfigitem("commit", "description-size-limit", default=None)
coreconfigitem("commit", "extras-size-limit", default=None)
coreconfigitem("committemplate", ".*", default=None, generic=True)
coreconfigitem("connectionpool", "lifetime", default=None)
coreconfigitem("configs", "generationtime", default=-1)
coreconfigitem("configs", "mismatchsampling", default=10000)
coreconfigitem("configs", "mismatchwarn", default=False)
coreconfigitem("convert", "git.committeractions", default=lambda: ["messagedifferent"])
coreconfigitem("convert", "git.extrakeys", default=list)
coreconfigitem("convert", "git.findcopiesharder", default=False)
coreconfigitem("convert", "git.remoteprefix", default="remote")
coreconfigitem("convert", "git.renamelimit", default=400)
coreconfigitem("convert", "git.saverev", default=True)
coreconfigitem("convert", "git.similarity", default=50)
coreconfigitem("convert", "git.skipsubmodules", default=True)
coreconfigitem("convert", "hg.clonebranches", default=False)
coreconfigitem("convert", "hg.ignoreerrors", default=False)
coreconfigitem("convert", "hg.revs", default=None)
coreconfigitem("convert", "hg.saverev", default=False)
coreconfigitem("convert", "hg.sourcename", default=None)
coreconfigitem("convert", "hg.startrev", default=None)
coreconfigitem("convert", "hg.tagsbranch", default="default")
coreconfigitem("convert", "hg.usebranchnames", default=True)
coreconfigitem("convert", "ignoreancestorcheck", default=False)
coreconfigitem("convert", "localtimezone", default=False)
coreconfigitem("convert", "p4.encoding", default=dynamicdefault)
coreconfigitem("convert", "p4.startrev", default=0)
coreconfigitem("convert", "skiptags", default=False)
coreconfigitem("convert", "svn.debugsvnlog", default=True)
coreconfigitem("convert", "svn.trunk", default=None)
coreconfigitem("convert", "svn.tags", default=None)
coreconfigitem("convert", "svn.branches", default=None)
coreconfigitem("convert", "svn.startrev", default=0)
coreconfigitem("debug", "dirstate.delaywrite", default=0)
coreconfigitem("defaults", ".*", default=None, generic=True)
coreconfigitem("devel", "all-warnings", default=False)
coreconfigitem("devel", "bundle2.debug", default=False)
coreconfigitem("devel", "cache-vfs", default=None)
coreconfigitem("devel", "check-locks", default=False)
coreconfigitem("devel", "check-relroot", default=False)
coreconfigitem("devel", "debugger", default=False)
coreconfigitem("devel", "default-date", default=None)
coreconfigitem("devel", "deprec-warn", default=False)
coreconfigitem("devel", "disableloaddefaultcerts", default=False)
coreconfigitem("devel", "legacy.exchange", default=list)
coreconfigitem("devel", "legacy.revnum", default="accept")
coreconfigitem("devel", "servercafile", default="")
coreconfigitem("devel", "serverexactprotocol", default="")
coreconfigitem("devel", "serverrequirecert", default=False)
coreconfigitem("devel", "strip-obsmarkers", default=True)
coreconfigitem("devel", "warn-config", default=None)
coreconfigitem("devel", "warn-config-default", default=None)
coreconfigitem("devel", "user.obsmarker", default=None)
coreconfigitem("devel", "warn-config-unknown", default=None)
coreconfigitem("discovery", "full-sample-size", default=200)
coreconfigitem("discovery", "initial-sample-size", default=100)
coreconfigitem("diff", "nodates", default=False)
coreconfigitem("diff", "showfunc", default=False)
coreconfigitem("diff", "unified", default=None)
coreconfigitem("diff", "git", default=False)
coreconfigitem("diff", "ignorews", default=False)
coreconfigitem("diff", "ignorewsamount", default=False)
coreconfigitem("diff", "ignoreblanklines", default=False)
coreconfigitem("diff", "ignorewseol", default=False)
coreconfigitem("diff", "nobinary", default=False)
coreconfigitem("diff", "noprefix", default=False)
coreconfigitem("doctor", "check-lag-name", "master")
coreconfigitem("doctor", "check-lag-threshold", 50)
coreconfigitem("doctor", "check-too-many-names-threshold", 20)
coreconfigitem("edenfs", "tree-fetch-depth", default=3)
coreconfigitem("email", "bcc", default=None)
coreconfigitem("email", "cc", default=None)
coreconfigitem("email", "charsets", default=list)
coreconfigitem("email", "from", default=None)
coreconfigitem("email", "method", default="smtp")
coreconfigitem("email", "reply-to", default=None)
coreconfigitem("email", "to", default=None)
coreconfigitem("experimental", "archivemetatemplate", default=dynamicdefault)
coreconfigitem("experimental", "bundle-phases", default=False)
coreconfigitem("experimental", "bundle2-advertise", default=True)
coreconfigitem("experimental", "bundle2-output-capture", default=False)
coreconfigitem("experimental", "bundle2.pushback", default=False)
coreconfigitem("experimental", "bundle2lazylocking", default=False)
coreconfigitem("experimental", "bundlecomplevel", default=None)
coreconfigitem("experimental", "changegroup3", default=False)
coreconfigitem("experimental", "clientcompressionengines", default=list)
coreconfigitem("experimental", "copytrace", default="on")
coreconfigitem("experimental", "copytrace.movecandidateslimit", default=100)
coreconfigitem("experimental", "copytrace.sourcecommitlimit", default=100)
coreconfigitem("experimental", "crecordtest", default=None)
coreconfigitem("experimental", "disable-narrow-heads-ssh-server", default=True)
coreconfigitem(
    "experimental",
    "evolution.allowdivergence",
    default=False,
    alias=[("experimental", "allowdivergence")],
)
coreconfigitem("experimental", "evolution.allowunstable", default=None)
coreconfigitem("experimental", "evolution.createmarkers", default=None)
coreconfigitem(
    "experimental",
    "evolution.effect-flags",
    default=True,
    alias=[("experimental", "effect-flags")],
)
coreconfigitem("experimental", "evolution.exchange", default=None)
coreconfigitem("experimental", "evolution.track-operation", default=True)
coreconfigitem("experimental", "worddiff", default=False)
coreconfigitem("experimental", "mmapindexthreshold", default=1)
coreconfigitem("experimental", "nonnormalparanoidcheck", default=False)
coreconfigitem("experimental", "exportableenviron", default=list)
coreconfigitem("experimental", "extendedheader.index", default=None)
coreconfigitem("experimental", "extendedheader.similarity", default=False)
coreconfigitem("experimental", "format.compression", default="zlib")
coreconfigitem("experimental", "graph.renderer", default="lines")
coreconfigitem("experimental", "graph.min-row-height", default=dynamicdefault)
coreconfigitem("experimental", "graphshorten", default=False)
coreconfigitem("experimental", "graphstyle.parent", default=dynamicdefault)
coreconfigitem("experimental", "graphstyle.missing", default=dynamicdefault)
coreconfigitem("experimental", "graphstyle.grandparent", default=dynamicdefault)
coreconfigitem("experimental", "hook-track-tags", default=False)
coreconfigitem("experimental", "httppostargs", default=False)
coreconfigitem("experimental", "manifestv2", default=False)
coreconfigitem("experimental", "mergedriver", default=None)
coreconfigitem("experimental", "narrow-heads", default=True)
coreconfigitem("experimental", "obsmarkers-exchange-debug", default=False)
coreconfigitem("experimental", "pathhistory", default=False)
coreconfigitem("experimental", "pathhistory.find-merge-conflicts", default=True)
coreconfigitem("experimental", "remotenames", default=False)
# Map rev to safe f64 range for Javascript consumption.
coreconfigitem("experimental", "revf64compat", default=True)

# load Rust-based HgCommits on changelog.
coreconfigitem("experimental", "rust-commits", default=True)

coreconfigitem("experimental", "single-head-per-branch", default=False)
coreconfigitem("experimental", "spacemovesdown", default=False)
coreconfigitem("experimental", "sparse-read", default=False)
coreconfigitem("experimental", "sparse-read.density-threshold", default=0.25)
coreconfigitem("experimental", "sparse-read.min-gap-size", default="256K")
coreconfigitem("experimental", "treemanifest", default=False)
coreconfigitem("experimental", "treematcher", default=True)
coreconfigitem("experimental", "regexmatcher", default=True)
coreconfigitem("experimental", "dynmatcher", default=False)
coreconfigitem("experimental", "uncommitondirtywdir", default=True)
coreconfigitem("experimental", "xdiff", default=True)
coreconfigitem("extensions", ".*", default=None, generic=True)
coreconfigitem("format", "aggressivemergedeltas", default=False)
coreconfigitem(
    "format", "cgdeltabase", default="default"  # changegroup.CFG_CGDELTA_DEFAULT
)
coreconfigitem("format", "chunkcachesize", default=None)
coreconfigitem("format", "dirstate", default=2)
coreconfigitem("format", "manifestcachesize", default=None)
coreconfigitem("format", "maxchainlen", default=None)
coreconfigitem("format", "obsstore-version", default=None)
coreconfigitem("format", "usegeneraldelta", default=True)
coreconfigitem("format", "use-segmented-changelog", default=util.istest())
coreconfigitem("fsmonitor", "warn_when_unused", default=True)
coreconfigitem("fsmonitor", "warn_update_file_count", default=50000)
coreconfigitem("git", "submodules", default=True)
coreconfigitem("gpg", "enabled", default=True)
coreconfigitem("gpg", "key", default=None)
coreconfigitem("hint", "ack", default=list)
coreconfigitem("histgrep", "allowfullrepogrep", default=True)
coreconfigitem("hooks", ".*", default=dynamicdefault, generic=True)
coreconfigitem("hostfingerprints", ".*", default=list, generic=True)
coreconfigitem("hostsecurity", "ciphers", default=None)
coreconfigitem("hostsecurity", "disabletls10warning", default=False)
coreconfigitem("hostsecurity", "minimumprotocol", default=dynamicdefault)
coreconfigitem(
    "hostsecurity", ".*:minimumprotocol$", default=dynamicdefault, generic=True
)
coreconfigitem("hostsecurity", ".*:ciphers$", default=dynamicdefault, generic=True)
coreconfigitem("hostsecurity", ".*:fingerprints$", default=list, generic=True)
coreconfigitem("hostsecurity", ".*:verifycertsfile$", default=None, generic=True)

coreconfigitem("http_proxy", "always", default=False)
coreconfigitem("http_proxy", "host", default=None)
coreconfigitem("http_proxy", "no", default=list)
coreconfigitem("http_proxy", "passwd", default=None)
coreconfigitem("http_proxy", "user", default=None)
coreconfigitem("log", "simplify-grandparents", default=True)
coreconfigitem("logtoprocess", "commandexception", default=None)
coreconfigitem("logtoprocess", "commandfinish", default=None)
coreconfigitem("logtoprocess", "command", default=None)
coreconfigitem("logtoprocess", "develwarn", default=None)
coreconfigitem("logtoprocess", "measuredtimes", default=None)
coreconfigitem("merge", "checkunknown", default="abort")
coreconfigitem("merge", "checkignored", default="abort")
coreconfigitem("experimental", "merge.checkpathconflicts", default=False)
coreconfigitem("merge", "followcopies", default=True)
coreconfigitem("merge", "on-failure", default="continue")
coreconfigitem("merge", "preferancestor", default=lambda: ["*"])
coreconfigitem("merge", "printcandidatecommmits", default=False)
coreconfigitem("merge", "word-merge", default=False)
coreconfigitem("merge-tools", ".*", default=None, generic=True)
coreconfigitem(
    "merge-tools",
    r".*\.args$",
    default="$local $base $other",
    generic=True,
    priority=-1,
)
coreconfigitem("merge-tools", r".*\.binary$", default=False, generic=True, priority=-1)
coreconfigitem("merge-tools", r".*\.check$", default=list, generic=True, priority=-1)
coreconfigitem(
    "merge-tools", r".*\.checkchanged$", default=False, generic=True, priority=-1
)
coreconfigitem(
    "merge-tools", r".*\.executable$", default=dynamicdefault, generic=True, priority=-1
)
coreconfigitem("merge-tools", r".*\.fixeol$", default=False, generic=True, priority=-1)
coreconfigitem("merge-tools", r".*\.gui$", default=False, generic=True, priority=-1)
coreconfigitem("merge-tools", r".*\.priority$", default=0, generic=True, priority=-1)
coreconfigitem(
    "merge-tools", r".*\.premerge$", default=dynamicdefault, generic=True, priority=-1
)
coreconfigitem("merge-tools", r".*\.symlink$", default=False, generic=True, priority=-1)
coreconfigitem("metalog", "track-config", default=True)
coreconfigitem("mononokepeer", "compression", default=False)
coreconfigitem("mononokepeer", "sockettimeout", default=15.0)
coreconfigitem("mutation", "date", default=None)
coreconfigitem("mutation", "enabled", default=True)
coreconfigitem("mutation", "record", default=True)
coreconfigitem("mutation", "user", default=None)
coreconfigitem("pager", "attend-.*", default=dynamicdefault, generic=True)
coreconfigitem("pager", "ignore", default=list)
coreconfigitem("pager", "pager", default="internal:streampager")
coreconfigitem("pager", "stderr", default=True)
coreconfigitem("patch", "eol", default="strict")
coreconfigitem("patch", "fuzz", default=2)
coreconfigitem("paths", "default", default=None)
coreconfigitem("paths", "default-push", default=None)
coreconfigitem("paths", ".*", default=None, generic=True)
coreconfigitem("phases", "new-commit", default="draft")
coreconfigitem("phases", "publish", default=True)
coreconfigitem("profiling", "enabled", default=False)
coreconfigitem("profiling", "format", default="text")
coreconfigitem("profiling", "freq", default=1000)
coreconfigitem("profiling", "limit", default=30)
coreconfigitem("profiling", "minelapsed", default=0)
coreconfigitem("profiling", "nested", default=0)
coreconfigitem("profiling", "output", default=None)
coreconfigitem("profiling", "showmax", default=0.999)
coreconfigitem("profiling", "showmin", default=dynamicdefault)
coreconfigitem("profiling", "sort", default="inlinetime")
coreconfigitem("profiling", "statformat", default="hotpath")
coreconfigitem("profiling", "type", default="stat")
coreconfigitem("progress", "assume-tty", default=False)
coreconfigitem("progress", "changedelay", default=1)
coreconfigitem("progress", "clear-complete", default=True)
coreconfigitem("progress", "debug", default=False)
coreconfigitem("progress", "delay", default=3)
coreconfigitem("progress", "disable", default=False)
coreconfigitem("progress", "estimateinterval", default=10.0)
coreconfigitem(
    "progress", "format", default=lambda: ["topic", "bar", "number", "estimate"]
)
coreconfigitem("progress", "refresh", default=0.1)
coreconfigitem("progress", "renderer", default="classic")
coreconfigitem("progress", "width", default=dynamicdefault)
coreconfigitem("pull", "automigrate", default=True)
# Practically, 100k commit data takes about 200MB memroy (or 400MB if
# duplicated in Python / Rust).
coreconfigitem("pull", "buffer-commit-count", default=(util.istest() and 5 or 100000))
coreconfigitem("pull", "httpbookmarks", default=True)
coreconfigitem("pull", "httphashprefix", default=False)
coreconfigitem("pull", "httpcommitgraph", default=False)
coreconfigitem("pull", "httpmutation", default=True)
coreconfigitem("pull", "master-fastpath", default=True)
coreconfigitem("exchange", "httpcommitlookup", default=True)
coreconfigitem("push", "pushvars.server", default=True)
coreconfigitem("push", "requirereason", default=False)
coreconfigitem("push", "requirereasonmsg", default="")
coreconfigitem("sendunbundlereplay", "respondlightly", default=True)
coreconfigitem("server", "bookmarks-pushkey-compat", default=True)
coreconfigitem("server", "bundle1", default=True)
coreconfigitem("server", "bundle1gd", default=None)
coreconfigitem("server", "bundle1.pull", default=None)
coreconfigitem("server", "bundle1gd.pull", default=None)
coreconfigitem("server", "bundle1.push", default=None)
coreconfigitem("server", "bundle1gd.push", default=None)
coreconfigitem("server", "compressionengines", default=list)
coreconfigitem("server", "disablefullbundle", default=False)
coreconfigitem("server", "maxhttpheaderlen", default=1024)
coreconfigitem("server", "preferuncompressed", default=False)
coreconfigitem("server", "uncompressed", default=True)
coreconfigitem("server", "uncompressedallowsecret", default=False)
coreconfigitem("server", "validate", default=False)
coreconfigitem("server", "zliblevel", default=-1)
coreconfigitem("smallcommitmetadata", "entrylimit", default=100)
coreconfigitem("smtp", "host", default=None)
coreconfigitem("smtp", "local_hostname", default=None)
coreconfigitem("smtp", "password", default=None)
coreconfigitem("smtp", "port", default=dynamicdefault)
coreconfigitem("smtp", "tls", default="none")
coreconfigitem("smtp", "username", default=None)
coreconfigitem("sparse", "missingwarning", default=False)
coreconfigitem("templates", ".*", default=None, generic=True)
coreconfigitem("trusted", "groups", default=list)
coreconfigitem("trusted", "users", default=list)
coreconfigitem("ui", "allowemptycommit", default=False)
coreconfigitem("ui", "allowmerge", default=True)
coreconfigitem("ui", "archivemeta", default=True)
coreconfigitem("ui", "askusername", default=False)
coreconfigitem("ui", "assume-tty", default=False)
coreconfigitem("ui", "autopullcommits", default=True)
coreconfigitem("ui", "clonebundlefallback", default=False)
coreconfigitem("ui", "clonebundleprefers", default=list)
coreconfigitem("ui", "clonebundles", default=True)
coreconfigitem("ui", "debug", default=False)
coreconfigitem("ui", "debugger", default="ipdb")
coreconfigitem("ui", "editor", default=dynamicdefault)
coreconfigitem("ui", "exitcodemask", default=255)
coreconfigitem("ui", "fallbackencoding", default=None)
coreconfigitem("ui", "fancy-traceback", default=True)
coreconfigitem("ui", "forcecwd", default=None)
coreconfigitem("ui", "forcemerge", default=None)
coreconfigitem("ui", "formatdebug", default=False)
coreconfigitem("ui", "formatjson", default=False)
coreconfigitem("ui", "formatted", default=None)
coreconfigitem("ui", "git", default="git")
coreconfigitem("ui", "gitignore", default=True)
coreconfigitem("ui", "graphnodetemplate", default=None)
coreconfigitem("ui", "hgignore", default=False)
coreconfigitem("ui", "http2debuglevel", default=None)
coreconfigitem("ui", "ignorerevnum", default=True)
coreconfigitem("ui", "interactive", default=None)
coreconfigitem("ui", "interface", default=None)
coreconfigitem("ui", "interface.chunkselector", default=None)
coreconfigitem("ui", "logmeasuredtimes", default=False)
coreconfigitem("ui", "logtemplate", default=None)
coreconfigitem("ui", "merge", default=None)
coreconfigitem("ui", "mergemarkers", default="basic")
coreconfigitem(
    "ui",
    "mergemarkertemplate",
    default=(
        "{node|short} "
        '{ifeq(tags, "tip", "", '
        'ifeq(tags, "", "", "{tags} "))}'
        '{if(bookmarks, "{bookmarks} ")}'
        '{ifeq(branch, "default", "", "{branch} ")}'
        "- {author|user}: {desc|firstline}"
    ),
)
coreconfigitem("ui", "nontty", default=False)
coreconfigitem("ui", "origbackuppath", default=None)
coreconfigitem("ui", "patch", default=None)
coreconfigitem("ui", "portablefilenames", default="warn")
coreconfigitem("ui", "promptecho", default=False)
coreconfigitem("ui", "quiet", default=False)
coreconfigitem("ui", "quietbookmarkmove", default=False)
coreconfigitem("ui", "remotecmd", default="hg")
coreconfigitem("ui", "report_untrusted", default=True)
coreconfigitem("ui", "skip-local-bookmarks-on-pull", default=False)
coreconfigitem("ui", "slash", default=False)
coreconfigitem("ui", "ssh", default="ssh")
coreconfigitem("ui", "ssherrorhint", default=None)
coreconfigitem("ui", "statuscopies", default=False)
coreconfigitem("ui", "strict", default=False)
coreconfigitem("ui", "style", default="")
coreconfigitem("ui", "supportcontact", default=None)
coreconfigitem("ui", "textwidth", default=78)
coreconfigitem("ui", "threaded", default=util.istest())
coreconfigitem("ui", "traceback", default=False)
coreconfigitem("ui", "tweakdefaults", default=False)
coreconfigitem("ui", "usehttp2", default=False)
coreconfigitem("ui", "username", alias=[("ui", "user")])
coreconfigitem("ui", "verbose", default=False)
coreconfigitem("ui", "version-age-threshold-days", default=31)
coreconfigitem("ui", "enableincomingoutgoing", default=True)
coreconfigitem("unsafe", "wvfsauditorcache", default=False)
coreconfigitem("visibility", "all-heads", default=False)
coreconfigitem("visibility", "enabled", default=True)
coreconfigitem("web", "allowbz2", default=False)
coreconfigitem("web", "allowgz", default=False)
coreconfigitem("web", "allow-pull", alias=[("web", "allowpull")], default=True)
coreconfigitem("web", "allow-push", alias=[("web", "allow_push")], default=list)
coreconfigitem("web", "allowzip", default=False)
coreconfigitem("web", "cache", default=True)
coreconfigitem("web", "contact", default=None)
coreconfigitem("web", "deny_push", default=list)
coreconfigitem("web", "guessmime", default=False)
coreconfigitem("web", "hidden", default=False)
coreconfigitem("web", "labels", default=list)
coreconfigitem("web", "logoimg", default="hglogo.png")
coreconfigitem("web", "logourl", default="https://mercurial-scm.org/")
coreconfigitem("web", "accesslog", default="-")
coreconfigitem("web", "address", default="")
coreconfigitem("web", "allow_archive", default=list)
coreconfigitem("web", "allow_read", default=list)
coreconfigitem("web", "baseurl", default=None)
coreconfigitem("web", "cacerts", default=None)
coreconfigitem("web", "certificate", default=None)
coreconfigitem("web", "collapse", default=False)
coreconfigitem("web", "csp", default=None)
coreconfigitem("web", "deny_read", default=list)
coreconfigitem("web", "descend", default=True)
coreconfigitem("web", "description", default="")
coreconfigitem("web", "encoding", default=lambda: encoding.encoding)
coreconfigitem("web", "errorlog", default="-")
coreconfigitem("web", "ipv6", default=False)
coreconfigitem("web", "maxchanges", default=10)
coreconfigitem("web", "maxfiles", default=10)
coreconfigitem("web", "maxshortchanges", default=60)
coreconfigitem("web", "motd", default="")
coreconfigitem("web", "name", default=dynamicdefault)
coreconfigitem("web", "port", default=8000)
coreconfigitem("web", "prefix", default="")
coreconfigitem("web", "push_ssl", default=True)
coreconfigitem("web", "refreshinterval", default=20)
coreconfigitem("web", "staticurl", default=None)
coreconfigitem("web", "stripes", default=1)
coreconfigitem("web", "style", default="paper")
coreconfigitem("web", "templates", default=None)
coreconfigitem("web", "view", default="served")
coreconfigitem("wireproto", "logrequests", default=list)
coreconfigitem("worker", "backgroundclose", default=dynamicdefault)
# Windows defaults to a limit of 512 open files. A buffer of 128
# should give us enough headway.
coreconfigitem("worker", "backgroundclosemaxqueue", default=384)
coreconfigitem("worker", "backgroundcloseminfilecount", default=2048)
coreconfigitem("worker", "backgroundclosethreadcount", default=4)
coreconfigitem("worker", "enabled", default=True)
coreconfigitem("worker", "numcpus", default=None)

coreconfigitem("workingcopy", "enablerustwalker", default=False)
coreconfigitem("workingcopy", "rustwalkerthreads", default=0)
coreconfigitem("workingcopy", "rustpendingchanges", default=False)
coreconfigitem("workingcopy", "ruststatus", default=False)
coreconfigitem("workingcopy", "use-rust", default=True)

# Rebase related configuration moved to core because other extension are doing
# strange things. For example, shelve import the extensions to reuse some bit
# without formally loading it.
coreconfigitem("commands", "rebase.requiredest", default=False)
coreconfigitem("experimental", "rebaseskipobsolete", default=True)
coreconfigitem("rebase", "singletransaction", default=False)
coreconfigitem("rebase", "experimental.inmemory", default=False)

# Remote names.
coreconfigitem("remotenames", "autocleanupthreshold", default=50)
# XXX: Enable selectivepull for tests.
coreconfigitem("remotenames", "selectivepull", default=not util.istest())
coreconfigitem("remotenames", "selectivepulldefault", default=["master"])
coreconfigitem("remotenames", "selectivepulldiscovery", default=True)
coreconfigitem("remotenames", "autopullhoistpattern", default="")
coreconfigitem(
    "remotenames",
    "autopullpattern",
    default=r"re:^(?:default|remote)/[A-Za-z0-9._/-]+$",
)
configitem("remotenames", "hoist", default="default")
