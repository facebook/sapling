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


class configitem:
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

coreconfigitem("blackbox", "maxsize", default="100 MB")
coreconfigitem("blackbox", "maxfiles", default=3)
coreconfigitem("bundle2", "rechunkthreshold", default="1MB")
# bundle.reorder: experimental config
coreconfigitem("bundle", "reorder", default="auto")
coreconfigitem("censor", "policy", default="abort")
coreconfigitem("chgserver", "idletimeout", default=3600)
coreconfigitem("commands", "update.check", default="noconflict")
coreconfigitem("configs", "generationtime", default=-1)
coreconfigitem("configs", "mismatchsampling", default=10000)
coreconfigitem("copytrace", "sourcecommitlimit", default=100)
coreconfigitem("copytrace", "enableamendcopytrace", default=True)
coreconfigitem("copytrace", "amendcopytracecommitlimit", default=100)
coreconfigitem("debug", "dirstate.delaywrite", default=0)
coreconfigitem("devel", "legacy.revnum", default="accept")
coreconfigitem("devel", "strip-obsmarkers", default=True)
coreconfigitem("discovery", "full-sample-size", default=200)
coreconfigitem("discovery", "initial-sample-size", default=100)
coreconfigitem("doctor", "check-lag-name", "master")
coreconfigitem("doctor", "check-lag-threshold", 50)
coreconfigitem("doctor", "check-too-many-names-threshold", 20)
coreconfigitem("edenfs", "tree-fetch-depth", default=3)
coreconfigitem("email", "method", default="smtp")
coreconfigitem("experimental", "bundle2-advertise", default=True)
coreconfigitem("experimental", "disable-narrow-heads-ssh-server", default=True)
coreconfigitem("experimental", "mmapindexthreshold", default=1)
coreconfigitem("experimental", "format.compression", default="zlib")
coreconfigitem("experimental", "graph.renderer", default="lines")
coreconfigitem("experimental", "narrow-heads", default=True)
coreconfigitem("experimental", "pathhistory.find-merge-conflicts", default=True)
# Map rev to safe f64 range for Javascript consumption.
coreconfigitem("experimental", "revf64compat", default=True)

# load Rust-based HgCommits on changelog.
coreconfigitem("experimental", "rust-commits", default=True)

coreconfigitem("experimental", "uncommitondirtywdir", default=True)
coreconfigitem(
    "format",
    "cgdeltabase",
    default="default",  # changegroup.CFG_CGDELTA_DEFAULT
)
coreconfigitem("format", "dirstate", default=2)
coreconfigitem("format", "usegeneraldelta", default=True)
coreconfigitem("fsmonitor", "warn_when_unused", default=True)
coreconfigitem("fsmonitor", "warn_update_file_count", default=50000)
coreconfigitem("git", "submodules", default=True)
coreconfigitem("gpg", "enabled", default=True)
coreconfigitem("histgrep", "allowfullrepogrep", default=True)

coreconfigitem("log", "simplify-grandparents", default=True)
coreconfigitem("merge", "checkunknown", default="abort")
coreconfigitem("merge", "checkignored", default="abort")
coreconfigitem("merge", "followcopies", default=True)
coreconfigitem("merge", "on-failure", default="continue")
coreconfigitem("merge", "preferancestor", default=lambda: ["*"])
coreconfigitem(
    "merge-tools",
    r".*\.args$",
    default="$local $base $other",
    generic=True,
    priority=-1,
)
coreconfigitem("merge-tools", r".*\.priority$", default=0, generic=True, priority=-1)
coreconfigitem("metalog", "track-config", default=True)
coreconfigitem("mononokepeer", "sockettimeout", default=15.0)
coreconfigitem("mutation", "enabled", default=True)
coreconfigitem("mutation", "record", default=True)
coreconfigitem("pager", "pager", default="internal:streampager")
coreconfigitem("patch", "eol", default="strict")
coreconfigitem("patch", "fuzz", default=2)
coreconfigitem("phases", "new-commit", default="draft")
coreconfigitem("phases", "publish", default=True)
coreconfigitem("profiling", "format", default="text")
coreconfigitem("profiling", "freq", default=1000)
coreconfigitem("profiling", "limit", default=30)
coreconfigitem("profiling", "minelapsed", default=0)
coreconfigitem("profiling", "nested", default=0)
coreconfigitem("profiling", "showmax", default=0.999)
coreconfigitem("profiling", "sort", default="inlinetime")
coreconfigitem("profiling", "statformat", default="hotpath")
coreconfigitem("profiling", "type", default="stat")
coreconfigitem("progress", "changedelay", default=1)
coreconfigitem("progress", "clear-complete", default=True)
coreconfigitem("progress", "delay", default=3)
coreconfigitem("progress", "estimateinterval", default=10.0)
coreconfigitem(
    "progress", "format", default=lambda: ["topic", "bar", "number", "estimate"]
)
coreconfigitem("progress", "refresh", default=0.1)
coreconfigitem("progress", "renderer", default="classic")
coreconfigitem("pull", "automigrate", default=True)
# Practically, 100k commit data takes about 200MB memory (or 400MB if
# duplicated in Python / Rust).
coreconfigitem(
    "pull", "buffer-commit-count", default=lambda: util.istest() and 5 or 100000
)
coreconfigitem("pull", "httpbookmarks", default=True)
coreconfigitem("pull", "httpmutation", default=True)
coreconfigitem("pull", "master-fastpath", default=True)
coreconfigitem("exchange", "httpcommitlookup", default=True)
coreconfigitem("push", "pushvars.server", default=True)
coreconfigitem("push", "requirereasonmsg", default="")
coreconfigitem("sendunbundlereplay", "respondlightly", default=True)
coreconfigitem("server", "bookmarks-pushkey-compat", default=True)
coreconfigitem("server", "bundle1", default=True)
coreconfigitem("server", "compressionengines", default=list)
coreconfigitem("server", "maxhttpheaderlen", default=1024)
coreconfigitem("server", "uncompressed", default=True)
coreconfigitem("server", "zliblevel", default=-1)
coreconfigitem("smallcommitmetadata", "entrylimit", default=100)
coreconfigitem("smtp", "tls", default="none")
coreconfigitem("ui", "allowmerge", default=True)
coreconfigitem("ui", "archivemeta", default=True)
coreconfigitem("ui", "autopullcommits", default=True)
coreconfigitem("ui", "changesetdate", default="authordate")
coreconfigitem("ui", "clonebundles", default=True)
coreconfigitem("ui", "debugger", default="ipdb")
coreconfigitem("ui", "exitcodemask", default=255)
coreconfigitem("ui", "fancy-traceback", default=True)
coreconfigitem("ui", "git", default="git")
coreconfigitem("ui", "gitignore", default=True)
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
coreconfigitem("ui", "portablefilenames", default="warn")
coreconfigitem("ui", "remotecmd", default="hg")
coreconfigitem("ui", "ssh", default="ssh")
coreconfigitem("ui", "style", default="")
coreconfigitem("ui", "textwidth", default=78)
coreconfigitem("ui", "username", alias=[("ui", "user")])
coreconfigitem("ui", "version-age-threshold-days", default=31)
coreconfigitem("ui", "enableincomingoutgoing", default=True)
coreconfigitem("visibility", "enabled", default=True)
coreconfigitem("web", "allow-pull", alias=[("web", "allowpull")], default=True)
coreconfigitem("web", "cache", default=True)
coreconfigitem("web", "logoimg", default="hglogo.png")
coreconfigitem("web", "logourl", default="https://mercurial-scm.org/")
coreconfigitem("web", "accesslog", default="-")
coreconfigitem("web", "address", default="")
coreconfigitem("web", "descend", default=True)
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
# Windows defaults to a limit of 512 open files. A buffer of 128
# should give us enough headway.
coreconfigitem("worker", "backgroundclosemaxqueue", default=384)
coreconfigitem("worker", "backgroundcloseminfilecount", default=2048)
coreconfigitem("worker", "backgroundclosethreadcount", default=4)
coreconfigitem("worker", "enabled", default=True)

# Rebase related configuration moved to core because other extension are doing
# strange things. For example, shelve import the extensions to reuse some bit
# without formally loading it.
coreconfigitem("experimental", "rebaseskipobsolete", default=True)
