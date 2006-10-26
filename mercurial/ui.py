# ui.py - user interface bits for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import gettext as _
from demandload import *
demandload(globals(), "errno getpass os re socket sys tempfile")
demandload(globals(), "ConfigParser traceback util")

def dupconfig(orig):
    new = util.configparser(orig.defaults())
    updateconfig(orig, new)
    return new

def updateconfig(source, dest, sections=None):
    if not sections:
        sections = source.sections()
    for section in sections:
        if not dest.has_section(section):
            dest.add_section(section)
        for name, value in source.items(section, raw=True):
            dest.set(section, name, value)

class ui(object):
    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True, traceback=False, parentui=None):
        self.overlay = None
        self.header = []
        self.prev_header = []
        if parentui is None:
            # this is the parent of all ui children
            self.parentui = None
            self.readhooks = []
            self.quiet = quiet
            self.verbose = verbose
            self.debugflag = debug
            self.interactive = interactive
            self.traceback = traceback
            self.trusted_users = {}
            self.trusted_groups = {}
            self.cdata = util.configparser()
            self.readconfig(util.rcpath())
            self.updateopts(verbose, debug, quiet, interactive)
        else:
            # parentui may point to an ui object which is already a child
            self.parentui = parentui.parentui or parentui
            self.readhooks = self.parentui.readhooks[:]
            self.trusted_users = parentui.trusted_users.copy()
            self.trusted_groups = parentui.trusted_groups.copy()
            self.cdata = dupconfig(self.parentui.cdata)
            if self.parentui.overlay:
                self.overlay = dupconfig(self.parentui.overlay)

    def __getattr__(self, key):
        return getattr(self.parentui, key)

    def updateopts(self, verbose=False, debug=False, quiet=False,
                   interactive=True, traceback=False, config=[]):
        for section, name, value in config:
            self.setconfig(section, name, value)

        if quiet or verbose or debug:
            self.setconfig('ui', 'quiet', str(bool(quiet)))
            self.setconfig('ui', 'verbose', str(bool(verbose)))
            self.setconfig('ui', 'debug', str(bool(debug)))

        self.verbosity_constraints()

        if not interactive:
            self.setconfig('ui', 'interactive', 'False')
            self.interactive = False

        self.traceback = self.traceback or traceback

    def verbosity_constraints(self):
        self.quiet = self.configbool('ui', 'quiet')
        self.verbose = self.configbool('ui', 'verbose')
        self.debugflag = self.configbool('ui', 'debug')

        if self.debugflag:
            self.verbose = True
            self.quiet = False
        elif self.verbose and self.quiet:
            self.quiet = self.verbose = False

    def _is_trusted(self, fp, f, warn=True):
        tusers = self.trusted_users
        tgroups = self.trusted_groups
        if (tusers or tgroups) and '*' not in tusers and '*' not in tgroups:
            st = util.fstat(fp)
            user = util.username(st.st_uid)
            group = util.groupname(st.st_gid)
            if user not in tusers and group not in tgroups:
                if warn:
                    self.warn(_('Not reading file %s from untrusted '
                                'user %s, group %s\n') % (f, user, group))
                return False
        return True

    def readconfig(self, fn, root=None):
        if isinstance(fn, basestring):
            fn = [fn]
        for f in fn:
            try:
                fp = open(f)
            except IOError:
                continue
            if not self._is_trusted(fp, f):
                continue
            try:
                self.cdata.readfp(fp, f)
            except ConfigParser.ParsingError, inst:
                raise util.Abort(_("Failed to parse %s\n%s") % (f, inst))
        # override data from config files with data set with ui.setconfig
        if self.overlay:
            updateconfig(self.overlay, self.cdata)
        if root is None:
            root = os.path.expanduser('~')
        self.fixconfig(root=root)
        for hook in self.readhooks:
            hook(self)

    def addreadhook(self, hook):
        self.readhooks.append(hook)

    def readsections(self, filename, *sections):
        "read filename and add only the specified sections to the config data"
        if not sections:
            return

        cdata = util.configparser()
        try:
            cdata.read(filename)
        except ConfigParser.ParsingError, inst:
            raise util.Abort(_("failed to parse %s\n%s") % (filename,
                                                            inst))

        for section in sections:
            if not cdata.has_section(section):
                cdata.add_section(section)

        updateconfig(cdata, self.cdata, sections)

    def fixconfig(self, section=None, name=None, value=None, root=None):
        # translate paths relative to root (or home) into absolute paths
        if section is None or section == 'paths':
            if root is None:
                root = os.getcwd()
            items = section and [(name, value)] or []
            for cdata in self.cdata, self.overlay:
                if not cdata: continue
                if not items and cdata.has_section('paths'):
                    pathsitems = cdata.items('paths')
                else:
                    pathsitems = items
                for n, path in pathsitems:
                    if path and "://" not in path and not os.path.isabs(path):
                        cdata.set("paths", n, os.path.join(root, path))

        # update quiet/verbose/debug and interactive status
        if section is None or section == 'ui':
            if name is None or name in ('quiet', 'verbose', 'debug'):
                self.verbosity_constraints()

            if name is None or name == 'interactive':
                self.interactive = self.configbool("ui", "interactive", True)

        # update trust information
        if section is None or section == 'trusted':
            user = util.username()
            if user is not None:
                self.trusted_users[user] = 1
                for user in self.configlist('trusted', 'users'):
                    self.trusted_users[user] = 1
                for group in self.configlist('trusted', 'groups'):
                    self.trusted_groups[group] = 1

    def setconfig(self, section, name, value):
        if not self.overlay:
            self.overlay = util.configparser()
        for cdata in (self.overlay, self.cdata):
            if not cdata.has_section(section):
                cdata.add_section(section)
            cdata.set(section, name, value)
        self.fixconfig(section, name, value)

    def _config(self, section, name, default, funcname):
        if self.cdata.has_option(section, name):
            try:
                func = getattr(self.cdata, funcname)
                return func(section, name)
            except ConfigParser.InterpolationError, inst:
                raise util.Abort(_("Error in configuration section [%s] "
                                   "parameter '%s':\n%s")
                                 % (section, name, inst))
        return default

    def config(self, section, name, default=None):
        return self._config(section, name, default, 'get')

    def configbool(self, section, name, default=False):
        return self._config(section, name, default, 'getboolean')

    def configlist(self, section, name, default=None):
        """Return a list of comma/space separated strings"""
        result = self.config(section, name)
        if result is None:
            result = default or []
        if isinstance(result, basestring):
            result = result.replace(",", " ").split()
        return result

    def has_config(self, section):
        '''tell whether section exists in config.'''
        return self.cdata.has_section(section)

    def configitems(self, section):
        items = {}
        if self.cdata.has_section(section):
            try:
                items.update(dict(self.cdata.items(section)))
            except ConfigParser.InterpolationError, inst:
                raise util.Abort(_("Error in configuration section [%s]:\n%s")
                                 % (section, inst))
        x = items.items()
        x.sort()
        return x

    def walkconfig(self):
        sections = self.cdata.sections()
        sections.sort()
        for section in sections:
            for name, value in self.configitems(section):
                yield section, name, value.replace('\n', '\\n')

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
        Abort if no username is found, to force specifying the commit user
        with line option or repo hgrc.
        """
        user = os.environ.get("HGUSER")
        if user is None:
            user = self.config("ui", "username")
        if user is None:
            user = os.environ.get("EMAIL")
        if not user:
            self.status(_("Please choose a commit username to be recorded "
                          "in the changelog via\ncommand line option "
                          '(-u "First Last <email@example.com>"), in the\n'
                          "configuration files (hgrc), or by setting the "
                          "EMAIL environment variable.\n\n"))
            raise util.Abort(_("No commit username specified!"))
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
