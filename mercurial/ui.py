# ui.py - user interface bits for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
import errno, getpass, os, re, socket, sys, tempfile
import ConfigParser, traceback, util

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
    _isatty = None

    def __init__(self, verbose=False, debug=False, quiet=False,
                 interactive=True, traceback=False, report_untrusted=True,
                 parentui=None):
        self.overlay = None
        self.buffers = []
        if parentui is None:
            # this is the parent of all ui children
            self.parentui = None
            self.quiet = quiet
            self.verbose = verbose
            self.debugflag = debug
            self.interactive = interactive
            self.traceback = traceback
            self.report_untrusted = report_untrusted
            self.trusted_users = {}
            self.trusted_groups = {}
            # if ucdata is not None, its keys must be a superset of cdata's
            self.cdata = util.configparser()
            self.ucdata = None
            # we always trust global config files
            self.check_trusted = False
            self.readconfig(util.rcpath())
            self.check_trusted = True
            self.updateopts(verbose, debug, quiet, interactive)
        else:
            # parentui may point to an ui object which is already a child
            self.parentui = parentui.parentui or parentui
            self.trusted_users = parentui.trusted_users.copy()
            self.trusted_groups = parentui.trusted_groups.copy()
            self.cdata = dupconfig(self.parentui.cdata)
            if self.parentui.ucdata:
                self.ucdata = dupconfig(self.parentui.ucdata)
            if self.parentui.overlay:
                self.overlay = dupconfig(self.parentui.overlay)

    def __getattr__(self, key):
        return getattr(self.parentui, key)

    def isatty(self):
        if ui._isatty is None:
            ui._isatty = sys.stdin.isatty()
        return ui._isatty

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
        if not self.check_trusted:
            return True
        st = util.fstat(fp)
        if util.isowner(fp, st):
            return True
        tusers = self.trusted_users
        tgroups = self.trusted_groups
        if not tusers:
            user = util.username()
            if user is not None:
                self.trusted_users[user] = 1
                self.fixconfig(section='trusted')
        if (tusers or tgroups) and '*' not in tusers and '*' not in tgroups:
            user = util.username(st.st_uid)
            group = util.groupname(st.st_gid)
            if user not in tusers and group not in tgroups:
                if warn and self.report_untrusted:
                    self.warn(_('Not trusting file %s from untrusted '
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
            cdata = self.cdata
            trusted = self._is_trusted(fp, f)
            if not trusted:
                if self.ucdata is None:
                    self.ucdata = dupconfig(self.cdata)
                cdata = self.ucdata
            elif self.ucdata is not None:
                # use a separate configparser, so that we don't accidentally
                # override ucdata settings later on.
                cdata = util.configparser()

            try:
                cdata.readfp(fp, f)
            except ConfigParser.ParsingError, inst:
                msg = _("Failed to parse %s\n%s") % (f, inst)
                if trusted:
                    raise util.Abort(msg)
                self.warn(_("Ignored: %s\n") % msg)

            if trusted:
                if cdata != self.cdata:
                    updateconfig(cdata, self.cdata)
                if self.ucdata is not None:
                    updateconfig(cdata, self.ucdata)
        # override data from config files with data set with ui.setconfig
        if self.overlay:
            updateconfig(self.overlay, self.cdata)
        if root is None:
            root = os.path.expanduser('~')
        self.fixconfig(root=root)

    def readsections(self, filename, *sections):
        """Read filename and add only the specified sections to the config data

        The settings are added to the trusted config data.
        """
        if not sections:
            return

        cdata = util.configparser()
        try:
            try:
                fp = open(filename)
            except IOError, inst:
                raise util.Abort(_("unable to open %s: %s") %
                                 (filename, getattr(inst, "strerror", inst)))
            try:
                cdata.readfp(fp, filename)
            finally:
                fp.close()
        except ConfigParser.ParsingError, inst:
            raise util.Abort(_("failed to parse %s\n%s") % (filename, inst))

        for section in sections:
            if not cdata.has_section(section):
                cdata.add_section(section)

        updateconfig(cdata, self.cdata, sections)
        if self.ucdata:
            updateconfig(cdata, self.ucdata, sections)

    def fixconfig(self, section=None, name=None, value=None, root=None):
        # translate paths relative to root (or home) into absolute paths
        if section is None or section == 'paths':
            if root is None:
                root = os.getcwd()
            items = section and [(name, value)] or []
            for cdata in self.cdata, self.ucdata, self.overlay:
                if not cdata: continue
                if not items and cdata.has_section('paths'):
                    pathsitems = cdata.items('paths')
                else:
                    pathsitems = items
                for n, path in pathsitems:
                    if path and "://" not in path and not os.path.isabs(path):
                        cdata.set("paths", n, os.path.join(root, path))

        # update verbosity/interactive/report_untrusted settings
        if section is None or section == 'ui':
            if name is None or name in ('quiet', 'verbose', 'debug'):
                self.verbosity_constraints()
            if name is None or name == 'interactive':
                interactive = self.configbool("ui", "interactive", None)
                if interactive is None and self.interactive:
                    self.interactive = self.isatty()
                else:
                    self.interactive = interactive
            if name is None or name == 'report_untrusted':
                self.report_untrusted = (
                    self.configbool("ui", "report_untrusted", True))

        # update trust information
        if (section is None or section == 'trusted') and self.trusted_users:
            for user in self.configlist('trusted', 'users'):
                self.trusted_users[user] = 1
            for group in self.configlist('trusted', 'groups'):
                self.trusted_groups[group] = 1

    def setconfig(self, section, name, value):
        if not self.overlay:
            self.overlay = util.configparser()
        for cdata in (self.overlay, self.cdata, self.ucdata):
            if not cdata: continue
            if not cdata.has_section(section):
                cdata.add_section(section)
            cdata.set(section, name, value)
        self.fixconfig(section, name, value)

    def _get_cdata(self, untrusted):
        if untrusted and self.ucdata:
            return self.ucdata
        return self.cdata

    def _config(self, section, name, default, funcname, untrusted, abort):
        cdata = self._get_cdata(untrusted)
        if cdata.has_option(section, name):
            try:
                func = getattr(cdata, funcname)
                return func(section, name)
            except (ConfigParser.InterpolationError, ValueError), inst:
                msg = _("Error in configuration section [%s] "
                        "parameter '%s':\n%s") % (section, name, inst)
                if abort:
                    raise util.Abort(msg)
                self.warn(_("Ignored: %s\n") % msg)
        return default

    def _configcommon(self, section, name, default, funcname, untrusted):
        value = self._config(section, name, default, funcname,
                             untrusted, abort=True)
        if self.debugflag and not untrusted and self.ucdata:
            uvalue = self._config(section, name, None, funcname,
                                  untrusted=True, abort=False)
            if uvalue is not None and uvalue != value:
                self.warn(_("Ignoring untrusted configuration option "
                            "%s.%s = %s\n") % (section, name, uvalue))
        return value

    def config(self, section, name, default=None, untrusted=False):
        return self._configcommon(section, name, default, 'get', untrusted)

    def configbool(self, section, name, default=False, untrusted=False):
        return self._configcommon(section, name, default, 'getboolean',
                                  untrusted)

    def configlist(self, section, name, default=None, untrusted=False):
        """Return a list of comma/space separated strings"""
        result = self.config(section, name, untrusted=untrusted)
        if result is None:
            result = default or []
        if isinstance(result, basestring):
            result = result.replace(",", " ").split()
        return result

    def has_section(self, section, untrusted=False):
        '''tell whether section exists in config.'''
        cdata = self._get_cdata(untrusted)
        return cdata.has_section(section)

    def _configitems(self, section, untrusted, abort):
        items = {}
        cdata = self._get_cdata(untrusted)
        if cdata.has_section(section):
            try:
                items.update(dict(cdata.items(section)))
            except ConfigParser.InterpolationError, inst:
                msg = _("Error in configuration section [%s]:\n"
                        "%s") % (section, inst)
                if abort:
                    raise util.Abort(msg)
                self.warn(_("Ignored: %s\n") % msg)
        return items

    def configitems(self, section, untrusted=False):
        items = self._configitems(section, untrusted=untrusted, abort=True)
        if self.debugflag and not untrusted and self.ucdata:
            uitems = self._configitems(section, untrusted=True, abort=False)
            keys = uitems.keys()
            keys.sort()
            for k in keys:
                if uitems[k] != items.get(k):
                    self.warn(_("Ignoring untrusted configuration option "
                                "%s.%s = %s\n") % (section, k, uitems[k]))
        x = items.items()
        x.sort()
        return x

    def walkconfig(self, untrusted=False):
        cdata = self._get_cdata(untrusted)
        sections = cdata.sections()
        sections.sort()
        for section in sections:
            for name, value in self.configitems(section, untrusted):
                yield section, name, str(value).replace('\n', '\\n')

    def username(self):
        """Return default username to be used in commits.

        Searched in this order: $HGUSER, [ui] section of hgrcs, $EMAIL
        and stop searching if one of these is set.
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
                self.warn(_("No username found, using '%s' instead\n") % user)
            except KeyError:
                pass
        if not user:
            raise util.Abort(_("Please specify a username."))
        return user

    def shortuser(self, user):
        """Return a short representation of a user name or email address."""
        if not self.verbose: user = util.shortuser(user)
        return user

    def expandpath(self, loc, default=None):
        """Return repository location relative to cwd or from [paths]"""
        if "://" in loc or os.path.isdir(os.path.join(loc, '.hg')):
            return loc

        path = self.config("paths", loc)
        if not path and default is not None:
            path = self.config("paths", default)
        return path or loc

    def pushbuffer(self):
        self.buffers.append([])

    def popbuffer(self):
        return "".join(self.buffers.pop())

    def write(self, *args):
        if self.buffers:
            self.buffers[-1].extend([str(a) for a in args])
        else:
            for a in args:
                sys.stdout.write(str(a))

    def write_err(self, *args):
        try:
            if not sys.stdout.closed: sys.stdout.flush()
            for a in args:
                sys.stderr.write(str(a))
            # stderr may be buffered under win32 when redirected to files,
            # including stdout.
            if not sys.stderr.closed: sys.stderr.flush()
        except IOError, inst:
            if inst.errno != errno.EPIPE:
                raise

    def flush(self):
        try: sys.stdout.flush()
        except: pass
        try: sys.stderr.flush()
        except: pass

    def _readline(self, prompt=''):
        if self.isatty():
            try:
                # magically add command line editing support, where
                # available
                import readline
                # force demandimport to really load the module
                readline.read_history_file
            except ImportError:
                pass
        line = raw_input(prompt)
        # When stdin is in binary mode on Windows, it can cause
        # raw_input() to emit an extra trailing carriage return
        if os.linesep == '\r\n' and line and line[-1] == '\r':
            line = line[:-1]
        return line

    def prompt(self, msg, pat=None, default="y"):
        """Prompt user with msg, read response, and ensure it matches pat

        If not interactive -- the default is returned
        """
        if not self.interactive: return default
        while True:
            try:
                r = self._readline(msg + ' ')
                if not r:
                    return default
                if not pat or re.match(pat, r):
                    return r
                else:
                    self.write(_("unrecognized response\n"))
            except EOFError:
                raise util.Abort(_('response expected'))

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

            editor = self.geteditor()

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

    def geteditor(self):
        '''return editor to use'''
        return (os.environ.get("HGEDITOR") or
                self.config("ui", "editor") or
                os.environ.get("VISUAL") or
                os.environ.get("EDITOR", "vi"))

