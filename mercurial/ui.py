# ui.py - user interface bits for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, ConfigParser
from i18n import gettext as _
from demandload import *
demandload(globals(), "re socket sys util")

class ui(object):
    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True, parentui=None):
        self.overlay = {}
        self.cdata = ConfigParser.SafeConfigParser()
        self.parentui = parentui and parentui.parentui or parentui
        if parentui is None:
            self.readconfig(util.rcpath)

            self.quiet = self.configbool("ui", "quiet")
            self.verbose = self.configbool("ui", "verbose")
            self.debugflag = self.configbool("ui", "debug")
            self.interactive = self.configbool("ui", "interactive", True)

            self.updateopts(verbose, debug, quiet, interactive)
            self.diffcache = None

    def __getattr__(self, key):
        return getattr(self.parentui, key)

    def updateopts(self, verbose=False, debug=False, quiet=False,
                 interactive=True):
        self.quiet = (self.quiet or quiet) and not verbose and not debug
        self.verbose = (self.verbose or verbose) or debug
        self.debugflag = (self.debugflag or debug)
        self.interactive = (self.interactive and interactive)

    def readconfig(self, fn):
        if isinstance(fn, basestring):
            fn = [fn]
        for f in fn:
            try:
                self.cdata.read(f)
            except ConfigParser.ParsingError, inst:
                raise util.Abort(_("Failed to parse %s\n%s") % (f, inst))

    def setconfig(self, section, name, val):
        self.overlay[(section, name)] = val

    def config(self, section, name, default=None):
        if self.overlay.has_key((section, name)):
            return self.overlay[(section, name)]
        if self.cdata.has_option(section, name):
            return self.cdata.get(section, name)
        if self.parentui is None:
            return default
        else:
            return self.parentui.config(section, name, default)

    def configbool(self, section, name, default=False):
        if self.overlay.has_key((section, name)):
            return self.overlay[(section, name)]
        if self.cdata.has_option(section, name):
            return self.cdata.getboolean(section, name)
        if self.parentui is None:
            return default
        else:
            return self.parentui.configbool(section, name, default)

    def configitems(self, section):
        items = {}
        if self.parentui is not None:
            items = dict(self.parentui.configitems(section))
        if self.cdata.has_section(section):
            items.update(dict(self.cdata.items(section)))
        x = items.items()
        x.sort()
        return x

    def walkconfig(self, seen=None):
        if seen is None:
            seen = {}
        for (section, name), value in self.overlay.iteritems():
            yield section, name, value
            seen[section, name] = 1
        for section in self.cdata.sections():
            for name, value in self.cdata.items(section):
                if (section, name) in seen: continue
                yield section, name, value.replace('\n', '\\n')
                seen[section, name] = 1
        if self.parentui is not None:
            for parent in self.parentui.walkconfig(seen):
                yield parent

    def extensions(self):
        return self.configitems("extensions")

    def diffopts(self):
        if self.diffcache:
            return self.diffcache
        ret = { 'showfunc' : True, 'ignorews' : False}
        for x in self.configitems("diff"):
            k = x[0].lower()
            v = x[1]
            if v:
                v = v.lower()
                if v == 'true':
                    value = True
                else:
                    value = False
                ret[k] = value
        self.diffcache = ret
        return ret

    def username(self):
        return (os.environ.get("HGUSER") or
                self.config("ui", "username") or
                os.environ.get("EMAIL") or
                (os.environ.get("LOGNAME",
                                os.environ.get("USERNAME", "unknown"))
                 + '@' + socket.getfqdn()))

    def shortuser(self, user):
        """Return a short representation of a user name or email address."""
        if not self.verbose:
            f = user.find('@')
            if f >= 0:
                user = user[:f]
            f = user.find('<')
            if f >= 0:
                user = user[f+1:]
        return user

    def expandpath(self, loc, root=""):
        paths = {}
        for name, path in self.configitems("paths"):
            m = path.find("://")
            if m == -1:
                    path = os.path.join(root, path)
            paths[name] = path

        return paths.get(loc, loc)

    def write(self, *args):
        for a in args:
            sys.stdout.write(str(a))

    def write_err(self, *args):
        if not sys.stdout.closed: sys.stdout.flush()
        for a in args:
            sys.stderr.write(str(a))

    def flush(self):
        try:
            sys.stdout.flush()
        finally:
            sys.stderr.flush()

    def readline(self):
        return sys.stdin.readline()[:-1]
    def prompt(self, msg, pat, default="y"):
        if not self.interactive: return default
        while 1:
            self.write(msg, " ")
            r = self.readline()
            if re.match(pat, r):
                return r
            else:
                self.write(_("unrecognized response\n"))
    def status(self, *msg):
        if not self.quiet: self.write(*msg)
    def warn(self, *msg):
        self.write_err(*msg)
    def note(self, *msg):
        if self.verbose: self.write(*msg)
    def debug(self, *msg):
        if self.debugflag: self.write(*msg)
    def edit(self, text):
        import tempfile
        (fd, name) = tempfile.mkstemp("hg")
        f = os.fdopen(fd, "w")
        f.write(text)
        f.close()

        editor = (os.environ.get("HGEDITOR") or
                  self.config("ui", "editor") or
                  os.environ.get("EDITOR", "vi"))

        os.environ["HGUSER"] = self.username()
        util.system("%s \"%s\"" % (editor, name), errprefix=_("edit failed"))

        t = open(name).read()
        t = re.sub("(?m)^HG:.*\n", "", t)

        os.unlink(name)

        return t

