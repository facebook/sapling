# ui.py - user interface bits for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import ConfigParser
from i18n import gettext as _
from demandload import *
demandload(globals(), "errno os re socket sys tempfile util")

class ui(object):
    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True, parentui=None):
        self.overlay = {}
        if parentui is None:
            # this is the parent of all ui children
            self.parentui = None
            self.cdata = ConfigParser.SafeConfigParser()
            self.readconfig(util.rcpath())

            self.quiet = self.configbool("ui", "quiet")
            self.verbose = self.configbool("ui", "verbose")
            self.debugflag = self.configbool("ui", "debug")
            self.interactive = self.configbool("ui", "interactive", True)

            self.updateopts(verbose, debug, quiet, interactive)
            self.diffcache = None
        else:
            # parentui may point to an ui object which is already a child
            self.parentui = parentui.parentui or parentui
            parent_cdata = self.parentui.cdata
            self.cdata = ConfigParser.SafeConfigParser(parent_cdata.defaults())
            # make interpolation work
            for section in parent_cdata.sections():
                self.cdata.add_section(section)
                for name, value in parent_cdata.items(section, raw=True):
                    self.cdata.set(section, name, value)

    def __getattr__(self, key):
        return getattr(self.parentui, key)

    def updateopts(self, verbose=False, debug=False, quiet=False,
                 interactive=True):
        self.quiet = (self.quiet or quiet) and not verbose and not debug
        self.verbose = (self.verbose or verbose) or debug
        self.debugflag = (self.debugflag or debug)
        self.interactive = (self.interactive and interactive)

    def readconfig(self, fn, root=None):
        if isinstance(fn, basestring):
            fn = [fn]
        for f in fn:
            try:
                self.cdata.read(f)
            except ConfigParser.ParsingError, inst:
                raise util.Abort(_("Failed to parse %s\n%s") % (f, inst))
        # translate paths relative to root (or home) into absolute paths
        if root is None:
            root = os.path.expanduser('~')
        for name, path in self.configitems("paths"):
            if path and path.find("://") == -1 and not os.path.isabs(path):
                self.cdata.set("paths", name, os.path.join(root, path))

    def setconfig(self, section, name, val):
        self.overlay[(section, name)] = val

    def config(self, section, name, default=None):
        if self.overlay.has_key((section, name)):
            return self.overlay[(section, name)]
        if self.cdata.has_option(section, name):
            try:
                return self.cdata.get(section, name)
            except ConfigParser.InterpolationError, inst:
                raise util.Abort(_("Error in configuration:\n%s") % inst)
        if self.parentui is None:
            return default
        else:
            return self.parentui.config(section, name, default)

    def configbool(self, section, name, default=False):
        if self.overlay.has_key((section, name)):
            return self.overlay[(section, name)]
        if self.cdata.has_option(section, name):
            try:
                return self.cdata.getboolean(section, name)
            except ConfigParser.InterpolationError, inst:
                raise util.Abort(_("Error in configuration:\n%s") % inst)
        if self.parentui is None:
            return default
        else:
            return self.parentui.configbool(section, name, default)

    def configitems(self, section):
        items = {}
        if self.parentui is not None:
            items = dict(self.parentui.configitems(section))
        if self.cdata.has_section(section):
            try:
                items.update(dict(self.cdata.items(section)))
            except ConfigParser.InterpolationError, inst:
                raise util.Abort(_("Error in configuration:\n%s") % inst)
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

    def hgignorefiles(self):
        result = []
        cfgitems = self.configitems("ui")
        for key, value in cfgitems:
            if key == 'ignore' or key.startswith('ignore.'):
                path = os.path.expanduser(value)
                result.append(path)
        return result

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
        """Return default username to be used in commits.

        Searched in this order: $HGUSER, [ui] section of hgrcs, $EMAIL
        and stop searching if one of these is set.
        Abort if found username is an empty string to force specifying
        the commit user elsewhere, e.g. with line option or repo hgrc.
        If not found, use $LOGNAME or $USERNAME +"@full.hostname".
        """
        user = os.environ.get("HGUSER")
        if user is None:
            user = self.config("ui", "username")
        if user is None:
            user = os.environ.get("EMAIL")
        if user is None:
            user = os.environ.get("LOGNAME") or os.environ.get("USERNAME")
            if user:
                user = "%s@%s" % (user, socket.getfqdn())
        if not user:
            raise util.Abort(_("Please specify a username."))
        return user

    def shortuser(self, user):
        """Return a short representation of a user name or email address."""
        if not self.verbose: user = util.shortuser(user)
        return user

    def expandpath(self, loc):
        """Return repository location relative to cwd or from [paths]"""
        if loc.find("://") != -1 or os.path.exists(loc):
            return loc

        return self.config("paths", loc, loc)

    def write(self, *args):
        for a in args:
            sys.stdout.write(str(a))

    def write_err(self, *args):
        try:
            if not sys.stdout.closed: sys.stdout.flush()
            for a in args:
                sys.stderr.write(str(a))
        except IOError, inst:
            if inst.errno != errno.EPIPE:
                raise

    def flush(self):
        try: sys.stdout.flush()
        except: pass
        try: sys.stderr.flush()
        except: pass

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
    def edit(self, text, user):
        (fd, name) = tempfile.mkstemp(prefix="hg-editor-", suffix=".txt")
        try:
            f = os.fdopen(fd, "w")
            f.write(text)
            f.close()

            editor = (os.environ.get("HGEDITOR") or
                    self.config("ui", "editor") or
                    os.environ.get("EDITOR", "vi"))

            util.system("%s \"%s\"" % (editor, name),
                        environ={'HGUSER': user},
                        onerr=util.Abort, errprefix=_("edit failed"))

            f = open(name)
            t = f.read()
            f.close()
            t = re.sub("(?m)^HG:.*\n", "", t)
        finally:
            os.unlink(name)

        return t
