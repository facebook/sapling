# ui.py - user interface bits for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import gettext as _
from demandload import *
demandload(globals(), "errno getpass os re socket sys tempfile")
demandload(globals(), "ConfigParser mdiff templater traceback util")

class ui(object):
    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True, traceback=False, parentui=None,
                 readhooks=[]):
        self.overlay = {}
        if parentui is None:
            # this is the parent of all ui children
            self.parentui = None
            self.readhooks = list(readhooks)
            self.cdata = ConfigParser.SafeConfigParser()
            self.readconfig(util.rcpath())

            self.quiet = self.configbool("ui", "quiet")
            self.verbose = self.configbool("ui", "verbose")
            self.debugflag = self.configbool("ui", "debug")
            self.interactive = self.configbool("ui", "interactive", True)
            self.traceback = traceback

            self.updateopts(verbose, debug, quiet, interactive)
            self.diffcache = None
            self.header = []
            self.prev_header = []
            self.revlogopts = self.configrevlog()
        else:
            # parentui may point to an ui object which is already a child
            self.parentui = parentui.parentui or parentui
            self.readhooks = list(parentui.readhooks or readhooks)
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
                   interactive=True, traceback=False, config=[]):
        self.quiet = (self.quiet or quiet) and not verbose and not debug
        self.verbose = (self.verbose or verbose) or debug
        self.debugflag = (self.debugflag or debug)
        self.interactive = (self.interactive and interactive)
        self.traceback = self.traceback or traceback
        for cfg in config:
            try:
                name, value = cfg.split('=', 1)
                section, name = name.split('.', 1)
                if not self.cdata.has_section(section):
                    self.cdata.add_section(section)
                if not section or not name:
                    raise IndexError
                self.cdata.set(section, name, value)
            except (IndexError, ValueError):
                raise util.Abort(_('malformed --config option: %s') % cfg)

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
            if path and "://" not in path and not os.path.isabs(path):
                self.cdata.set("paths", name, os.path.join(root, path))
        for hook in self.readhooks:
            hook(self)

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

    def configlist(self, section, name, default=None):
        """Return a list of comma/space separated strings"""
        result = self.config(section, name)
        if result is None:
            result = default or []
        if isinstance(result, basestring):
            result = result.replace(",", " ").split()
        return result

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

    def has_config(self, section):
        '''tell whether section exists in config.'''
        return self.cdata.has_section(section)

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
        result = self.configitems("extensions")
        for i, (key, value) in enumerate(result):
            if value:
                result[i] = (key, os.path.expanduser(value))
        return result

    def hgignorefiles(self):
        result = []
        for key, value in self.configitems("ui"):
            if key == 'ignore' or key.startswith('ignore.'):
                result.append(os.path.expanduser(value))
        return result

    def configrevlog(self):
        result = {}
        for key, value in self.configitems("revlog"):
            result[key.lower()] = value
        return result

    def username(self):
        """Return default username to be used in commits.

        Searched in this order: $HGUSER, [ui] section of hgrcs, $EMAIL
        and stop searching if one of these is set.
        Abort if found username is an empty string to force specifying
        the commit user elsewhere, e.g. with line option or repo hgrc.
        If not found, use ($LOGNAME or $USER or $LNAME or
        $USERNAME) +"@full.hostname".
        """
        user = os.environ.get("HGUSER")
        if user is None:
            user = self.config("ui", "username")
        if user is None:
            user = os.environ.get("EMAIL")
        if user is None:
            try:
                user = '%s@%s' % (util.getuser(), socket.getfqdn())
            except KeyError:
                raise util.Abort(_("Please specify a username."))
        return user

    def shortuser(self, user):
        """Return a short representation of a user name or email address."""
        if not self.verbose: user = util.shortuser(user)
        return user

    def expandpath(self, loc, default=None):
        """Return repository location relative to cwd or from [paths]"""
        if "://" in loc or os.path.isdir(loc):
            return loc

        path = self.config("paths", loc)
        if not path and default is not None:
            path = self.config("paths", default)
        return path or loc

    def write(self, *args):
        if self.header:
            if self.header != self.prev_header:
                self.prev_header = self.header
                self.write(*self.header)
            self.header = []
        for a in args:
            sys.stdout.write(str(a))

    def write_header(self, *args):
        for a in args:
            self.header.append(str(a))

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
    def prompt(self, msg, pat=None, default="y"):
        if not self.interactive: return default
        while 1:
            self.write(msg, " ")
            r = self.readline()
            if not pat or re.match(pat, r):
                return r
            else:
                self.write(_("unrecognized response\n"))
    def getpass(self, prompt=None, default=None):
        if not self.interactive: return default
        return getpass.getpass(prompt or _('password: '))
    def status(self, *msg):
        if not self.quiet: self.write(*msg)
    def warn(self, *msg):
        self.write_err(*msg)
    def note(self, *msg):
        if self.verbose: self.write(*msg)
    def debug(self, *msg):
        if self.debugflag: self.write(*msg)
    def edit(self, text, user):
        (fd, name) = tempfile.mkstemp(prefix="hg-editor-", suffix=".txt",
                                      text=True)
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

    def print_exc(self):
        '''print exception traceback if traceback printing enabled.
        only to call in exception handler. returns true if traceback
        printed.'''
        if self.traceback:
            traceback.print_exc()
        return self.traceback
