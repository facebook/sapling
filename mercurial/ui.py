# ui.py - user interface bits for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
import errno, getpass, os, re, socket, sys, tempfile
import config, traceback, util, error

_booleans = {'1':True, 'yes':True, 'true':True, 'on':True,
             '0':False, 'no':False, 'false':False, 'off':False}

class ui(object):
    def __init__(self, src=None):
        self._buffers = []
        self.quiet = self.verbose = self.debugflag = self.traceback = False
        self.interactive = self.report_untrusted = True
        self.overlay = config.config()
        self.cdata = config.config()
        self.ucdata = config.config()
        self._trustusers = {}
        self._trustgroups = {}

        if src:
            self.cdata = src.cdata.copy()
            self.ucdata = src.ucdata.copy()
            self.overlay = src.overlay.copy()
            self._trustusers = src._trustusers.copy()
            self._trustgroups = src._trustgroups.copy()
            self.fixconfig()
        else:
            # we always trust global config files
            for f in util.rcpath():
                self.readconfig(f, trust=True)
    def copy(self):
        return ui(self)

    _isatty = None
    def isatty(self):
        if ui._isatty is None:
            try:
                ui._isatty = sys.stdin.isatty()
            except AttributeError: # not a real file object
                ui._isatty = False
            except IOError:
                # access to stdin is unsafe in a WSGI environment
                ui._isatty = False
        return ui._isatty

    def _is_trusted(self, fp, f):
        st = util.fstat(fp)
        if util.isowner(fp, st):
            return True

        tusers, tgroups = self._trustusers, self._trustgroups
        if '*' in tusers or '*' in tgroups:
            return True

        user = util.username(st.st_uid)
        group = util.groupname(st.st_gid)
        if user in tusers or group in tgroups or user == util.username():
            return True

        if self.report_untrusted:
            self.warn(_('Not trusting file %s from untrusted '
                        'user %s, group %s\n') % (f, user, group))
        return False

    def readconfig(self, filename, root=None, trust=False,
                   sections = None):
        try:
            fp = open(filename)
        except IOError:
            if not sections: # ignore unless we were looking for something
                return
            raise

        cdata = config.config()
        trusted = sections or trust or self._is_trusted(fp, filename)

        try:
            cdata.read(filename, fp, sections=sections)
        except error.ConfigError, inst:
            if trusted:
                raise
            self.warn(_("Ignored: %s\n") % str(inst))

        if trusted:
            self.cdata.update(cdata)
            self.cdata.update(self.overlay)
        self.ucdata.update(cdata)
        self.ucdata.update(self.overlay)

        if root is None:
            root = os.path.expanduser('~')
        self.fixconfig(root=root)

    def fixconfig(self, root=None):
        # translate paths relative to root (or home) into absolute paths
        root = root or os.getcwd()
        for c in self.cdata, self.ucdata, self.overlay:
            for n, p in c.items('paths'):
                if p and "://" not in p and not os.path.isabs(p):
                    c.set("paths", n, os.path.normpath(os.path.join(root, p)))

        # update ui options
        self.debugflag = self.configbool('ui', 'debug')
        self.verbose = self.debugflag or self.configbool('ui', 'verbose')
        self.quiet = not self.debugflag and self.configbool('ui', 'quiet')
        if self.verbose and self.quiet:
            self.quiet = self.verbose = False
        self.report_untrusted = self.configbool("ui", "report_untrusted", True)
        self.interactive = self.configbool("ui", "interactive", self.isatty())
        self.traceback = self.configbool('ui', 'traceback', False)

        # update trust information
        for user in self.configlist('trusted', 'users'):
            self._trustusers[user] = 1
        for group in self.configlist('trusted', 'groups'):
            self._trustgroups[group] = 1

    def setconfig(self, section, name, value):
        for cdata in (self.overlay, self.cdata, self.ucdata):
            cdata.set(section, name, value)
        self.fixconfig()

    def _data(self, untrusted):
        return untrusted and self.ucdata or self.cdata

    def configsource(self, section, name, untrusted=False):
        return self._data(untrusted).source(section, name) or 'none'

    def config(self, section, name, default=None, untrusted=False):
        value = self._data(untrusted).get(section, name, default)
        if self.debugflag and not untrusted:
            uvalue = self.ucdata.get(section, name)
            if uvalue is not None and uvalue != value:
                self.warn(_("Ignoring untrusted configuration option "
                            "%s.%s = %s\n") % (section, name, uvalue))
        return value

    def configbool(self, section, name, default=False, untrusted=False):
        v = self.config(section, name, None, untrusted)
        if v == None:
            return default
        if v.lower() not in _booleans:
            raise error.ConfigError(_("%s.%s not a boolean ('%s')")
                                    % (section, name, v))
        return _booleans[v.lower()]

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
        return section in self._data(untrusted)

    def configitems(self, section, untrusted=False):
        items = self._data(untrusted).items(section)
        if self.debugflag and not untrusted:
            for k,v in self.ucdata.items(section):
                if self.cdata.get(section, k) != v:
                    self.warn(_("Ignoring untrusted configuration option "
                                "%s.%s = %s\n") % (section, k, v))
        return items

    def walkconfig(self, untrusted=False):
        cdata = self._data(untrusted)
        for section in cdata.sections():
            for name, value in self.configitems(section, untrusted):
                yield section, name, str(value).replace('\n', '\\n')

    def username(self):
        """Return default username to be used in commits.

        Searched in this order: $HGUSER, [ui] section of hgrcs, $EMAIL
        and stop searching if one of these is set.
        If not found and ui.askusername is True, ask the user, else use
        ($LOGNAME or $USER or $LNAME or $USERNAME) + "@full.hostname".
        """
        user = os.environ.get("HGUSER")
        if user is None:
            user = self.config("ui", "username")
        if user is None:
            user = os.environ.get("EMAIL")
        if user is None and self.configbool("ui", "askusername"):
            user = self.prompt(_("enter a commit username:"), default=None)
        if user is None:
            try:
                user = '%s@%s' % (util.getuser(), socket.getfqdn())
                self.warn(_("No username found, using '%s' instead\n") % user)
            except KeyError:
                pass
        if not user:
            raise util.Abort(_("Please specify a username."))
        if "\n" in user:
            raise util.Abort(_("username %s contains a newline\n") % repr(user))
        return user

    def shortuser(self, user):
        """Return a short representation of a user name or email address."""
        if not self.verbose: user = util.shortuser(user)
        return user

    def _path(self, loc):
        p = self.config('paths', loc)
        if p and '%%' in p:
            ui.warn('(deprecated \'\%\%\' in path %s=%s from %s)\n' %
                    (loc, p, self.configsource('paths', loc)))
            p = p.replace('%%', '%')
        return p

    def expandpath(self, loc, default=None):
        """Return repository location relative to cwd or from [paths]"""
        if "://" in loc or os.path.isdir(os.path.join(loc, '.hg')):
            return loc

        path = self._path(loc)
        if not path and default is not None:
            path = self._path(default)
        return path or loc

    def pushbuffer(self):
        self._buffers.append([])

    def popbuffer(self):
        return "".join(self._buffers.pop())

    def write(self, *args):
        if self._buffers:
            self._buffers[-1].extend([str(a) for a in args])
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
                # windows sometimes raises something other than ImportError
            except Exception:
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
        if not self.interactive:
            self.note(msg, ' ', default, "\n")
            return default
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
        try:
            return getpass.getpass(prompt or _('password: '))
        except EOFError:
            raise util.Abort(_('response expected'))
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
