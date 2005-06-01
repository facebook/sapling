# ui.py - user interface bits for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, tempfile, sys, re

class ui:
    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True):
        self.quiet = quiet and not verbose and not debug
        self.verbose = verbose or debug
        self.debugflag = debug
        self.interactive = interactive
    def write(self, *args):
        for a in args:
            sys.stdout.write(str(a))
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
    def warn(self, msg):
        self.write(*msg)
    def note(self, *msg):
        if self.verbose: self.write(*msg)
    def debug(self, *msg):
        if self.debugflag: self.write(*msg)
    def edit(self, text):
        (fd, name) = tempfile.mkstemp("hg")
        f = os.fdopen(fd, "w")
        f.write(text)
        f.close()

        editor = os.environ.get("HGEDITOR") or os.environ.get("EDITOR", "vi")
        r = os.system("%s %s" % (editor, name))

        if r:
            raise "Edit failed!"

        t = open(name).read()
        t = re.sub("(?m)^HG:.*\n", "", t)

        return t
    
