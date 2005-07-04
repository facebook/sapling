# ui.py - user interface bits for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, ConfigParser
from demandload import *
demandload(globals(), "re socket sys util")

class ui:
    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True):
        self.cdata = ConfigParser.SafeConfigParser()
        self.cdata.read(os.path.expanduser("~/.hgrc"))

        self.quiet = self.configbool("ui", "quiet")
        self.verbose = self.configbool("ui", "verbose")
        self.debugflag = self.configbool("ui", "debug")
        self.interactive = self.configbool("ui", "interactive", True)

        self.quiet = (self.quiet or quiet) and not verbose and not debug
        self.verbose = (self.verbose or verbose) or debug
        self.debugflag = (self.debugflag or debug)
        self.interactive = (self.interactive and interactive)

    def readconfig(self, fp):
        self.cdata.readfp(fp)

    def config(self, section, val, default=None):
        if self.cdata.has_option(section, val):
            return self.cdata.get(section, val)
        return default

    def configbool(self, section, val, default=False):
        if self.cdata.has_option(section, val):
            return self.cdata.getboolean(section, val)
        return default

    def configitems(self, section):
        if self.cdata.has_section(section):
            return self.cdata.items(section)
        return []

    def username(self):
        return (self.config("ui", "username") or
                os.environ.get("HGUSER") or
                os.environ.get("EMAIL") or
                (os.environ.get("LOGNAME",
                                os.environ.get("USERNAME", "unknown"))
                 + '@' + socket.getfqdn()))

    def expandpath(self, loc):
        paths = {}
        for name, path in self.configitems("paths"):
            paths[name] = path

        return paths.get(loc, loc)

    def write(self, *args):
        for a in args:
            sys.stdout.write(str(a))

    def write_err(self, *args):
        sys.stdout.flush()
        for a in args:
            sys.stderr.write(str(a))

    def readline(self):
        return sys.stdin.readline()[:-1]
    def prompt(self, msg, pat, default = "y"):
        if not self.interactive: return default
        while 1:
            self.write(msg, " ")
            r = self.readline()
            if re.match(pat, r):
                return r
            else:
                self.write("unrecognized response\n")
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

        editor = (self.config("ui", "editor") or
                  os.environ.get("HGEDITOR") or
                  os.environ.get("EDITOR", "vi"))

        util.system("%s %s" % (editor, name), errprefix = "edit failed")

        t = open(name).read()
        t = re.sub("(?m)^HG:.*\n", "", t)

        return t
