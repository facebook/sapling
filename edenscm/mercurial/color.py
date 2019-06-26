# utility for color output for Mercurial commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re

from . import encoding, pycompat, util
from .i18n import _


try:
    import curses

    # Mapping from effect name to terminfo attribute name (or raw code) or
    # color number.  This will also force-load the curses module.
    _baseterminfoparams = {
        "none": (True, "sgr0", ""),
        "standout": (True, "smso", ""),
        "underline": (True, "smul", ""),
        "reverse": (True, "rev", ""),
        "inverse": (True, "rev", ""),
        "blink": (True, "blink", ""),
        "dim": (True, "dim", ""),
        "bold": (True, "bold", ""),
        "invisible": (True, "invis", ""),
        "italic": (True, "sitm", ""),
        "black": (False, curses.COLOR_BLACK, ""),
        "red": (False, curses.COLOR_RED, ""),
        "green": (False, curses.COLOR_GREEN, ""),
        "yellow": (False, curses.COLOR_YELLOW, ""),
        "blue": (False, curses.COLOR_BLUE, ""),
        "magenta": (False, curses.COLOR_MAGENTA, ""),
        "cyan": (False, curses.COLOR_CYAN, ""),
        "white": (False, curses.COLOR_WHITE, ""),
    }
except ImportError:
    curses = None
    _baseterminfoparams = {}

# start and stop parameters for effects
_effects = {
    "none": 0,
    "black": 30,
    "red": 31,
    "green": 32,
    "yellow": 33,
    "blue": 34,
    "magenta": 35,
    "cyan": 36,
    "white": 37,
    "bold": 1,
    "italic": 3,
    "underline": 4,
    "inverse": 7,
    "dim": 2,
    "black_background": 40,
    "red_background": 41,
    "green_background": 42,
    "yellow_background": 43,
    "blue_background": 44,
    "purple_background": 45,
    "cyan_background": 46,
    "white_background": 47,
}

_defaultstyles = {
    "grep.match": "red bold",
    "grep.linenumber": "green",
    "grep.rev": "green",
    "grep.change": "green",
    "grep.sep": "cyan",
    "grep.filename": "magenta",
    "grep.user": "magenta",
    "grep.date": "magenta",
    "blackbox.timestamp": "green",
    "blackbox.session.0": "yellow",
    "blackbox.session.1": "cyan",
    "blackbox.session.2": "magenta",
    "blackbox.session.3": "brightblue",
    "blame.age.1hour": "#ffe:color231:bold",
    "blame.age.1day": "#eea:color230:bold",
    "blame.age.7day": "#dd5:color229:brightyellow",
    "blame.age.30day": "#cc3:color228:brightyellow",
    "blame.age.60day": "#aa2:color185:yellow",
    "blame.age.180day": "#881:color142:yellow",
    "blame.age.360day": "#661:color100:yellow",
    "blame.age.old": "#440:color58:brightblack:yellow",
    "bookmarks.active": "green",
    "branches.active": "none",
    "branches.closed": "black bold",
    "branches.current": "green",
    "branches.inactive": "none",
    "diff.changed": "white",
    "diff.deleted": "color160:brightred:red",
    "diff.deleted.changed": "color196:brightred:red",
    "diff.deleted.unchanged": "color124:red",
    "diff.diffline": "bold",
    "diff.extended": "cyan bold",
    "diff.file_a": "red bold",
    "diff.file_b": "green bold",
    "diff.hunk": "magenta",
    "diff.inserted": "color40:brightgreen:green",
    "diff.inserted.changed": "color40:brightgreen:green",
    "diff.inserted.unchanged": "color28:green",
    "diff.tab": "",
    "diff.trailingwhitespace": "bold red_background",
    "changeset.public": "",
    "changeset.draft": "",
    "changeset.secret": "",
    "diffstat.deleted": "red",
    "diffstat.inserted": "green",
    "formatvariant.name.mismatchconfig": "red",
    "formatvariant.name.mismatchdefault": "yellow",
    "formatvariant.name.uptodate": "green",
    "formatvariant.repo.mismatchconfig": "red",
    "formatvariant.repo.mismatchdefault": "yellow",
    "formatvariant.repo.uptodate": "green",
    "formatvariant.config.special": "yellow",
    "formatvariant.config.default": "green",
    "formatvariant.default": "",
    "histedit.remaining": "red bold",
    "log.changeset": "yellow",
    "processtree.descendants": "green",
    "processtree.selected": "green bold",
    "progress.fancy.bar.background": "",
    "progress.fancy.bar.indeterminate": "yellow_background",
    "progress.fancy.bar.normal": "green_background",
    "progress.fancy.bar.spinner": "cyan_background",
    "progress.fancy.count": "bold",
    "progress.fancy.item": "",
    "progress.fancy.topic": "bold",
    "ui.prompt": "yellow",
    "rebase.rebased": "blue",
    "rebase.remaining": "red bold",
    "resolve.resolved": "green bold",
    "resolve.unresolved": "red bold",
    "shelve.age": "cyan",
    "shelve.newest": "green bold",
    "shelve.name": "blue bold",
    "status.added": "green bold",
    "status.clean": "none",
    "status.copied": "none",
    "status.deleted": "cyan bold underline",
    "status.ignored": "black bold",
    "status.modified": "blue bold",
    "status.removed": "red bold",
    "status.unknown": "magenta bold underline",
    "tags.normal": "green",
    "tags.local": "black bold",
    "ui.metrics": "#777:color242:dim",
    "ui.prefix.component": "cyan",
    "ui.prefix.error": "brightred:red",
    "ui.prefix.notice": "yellow",
}


def loadcolortable(ui, extname, colortable):
    _defaultstyles.update(colortable)


def setup(ui):
    """configure color on a ui

    That function both set the colormode for the ui object and read
    the configuration looking for custom colors and effect definitions."""
    mode = _modesetup(ui)
    ui._colormode = mode
    if mode and mode != "debug":
        configstyles(ui)


def _modesetup(ui):
    if ui.plain("color"):
        return None
    config = ui.config("ui", "color")
    if config == "debug":
        return "debug"

    auto = config == "auto"
    always = False
    if not auto and util.parsebool(config):
        # We want the config to behave like a boolean, "on" is actually auto,
        # but "always" value is treated as a special case to reduce confusion.
        if ui.configsource("ui", "color") == "--color" or config == "always":
            always = True
        else:
            auto = True

    if not always and not auto:
        return None

    formatted = always or (encoding.environ.get("TERM") != "dumb" and ui.formatted())

    mode = ui.config("color", "mode")

    # If pager is active, color.pagermode overrides color.mode.
    if getattr(ui, "pageractive", False):
        mode = ui.config("color", "pagermode", mode)

    realmode = mode
    if pycompat.iswindows:
        from . import win32

        term = encoding.environ.get("TERM")
        # TERM won't be defined in a vanilla cmd.exe environment.

        # UNIX-like environments on Windows such as Cygwin and MSYS will
        # set TERM. They appear to make a best effort attempt at setting it
        # to something appropriate. However, not all environments with TERM
        # defined support ANSI.
        ansienviron = term and "xterm" in term

        if mode == "auto":
            # Since "ansi" could result in terminal gibberish, we error on the
            # side of selecting "win32". However, if w32effects is not defined,
            # we almost certainly don't support "win32", so don't even try.
            # w32ffects is not populated when stdout is redirected, so checking
            # it first avoids win32 calls in a state known to error out.
            if ansienviron or not w32effects or win32.enablevtmode():
                realmode = "ansi"
            else:
                realmode = "win32"
        # An empty w32effects is a clue that stdout is redirected, and thus
        # cannot enable VT mode.
        elif mode == "ansi" and w32effects and not ansienviron:
            win32.enablevtmode()
    elif mode == "auto":
        realmode = "ansi"

    def modewarn():
        # only warn if color.mode was explicitly set and we're in
        # a formatted terminal
        if mode == realmode and formatted:
            ui.warn(_("warning: failed to set color mode to %s\n") % mode)

    if realmode == "terminfo":
        ui.warn(_("warning: color.mode = terminfo is no longer supported\n"))
        realmode = "ansi"

    if realmode == "win32":
        ui._terminfoparams.clear()
        if not w32effects:
            modewarn()
            return None
    elif realmode == "ansi":
        ui._terminfoparams.clear()
    else:
        return None

    if always or (auto and formatted):
        return realmode
    return None


def normalizestyle(ui, style):
    """choose a fallback from a list of labels"""
    # colorname1:colorname2:colorname3 means:
    # use colorname1 if supported, fallback to colorname2, then
    # fallback to colorname3.
    for e in style.split(":"):
        if valideffect(ui, e):
            return e


class truecoloreffects(dict):
    def makecolor(self, c):
        n = 38
        if c.endswith("_background"):
            c = c[:-11]
            n = 48
        if len(c) == 4:
            r, g, b = c[1] + c[1], c[2] + c[2], c[3] + c[3]
        else:
            r, g, b = c[1:3], c[3:5], c[5:7]
        return ";".join(map(str, [n, 2, int(r, 16), int(g, 16), int(b, 16)]))

    def get(self, key, default=None):
        if _truecolorre.match(key):
            return self.makecolor(key)
        else:
            return super(truecoloreffects, self).get(key, default)

    def __getitem__(self, key):
        if _truecolorre.match(key):
            return self.makecolor(key)
        else:
            return super(truecoloreffects, self).__getitem__(key)


def _extendcolors(colors):
    # see https://en.wikipedia.org/wiki/ANSI_escape_code
    global _effects
    if colors >= 16:
        _effects.update(
            {
                "brightblack": 90,
                "brightred": 91,
                "brightgreen": 92,
                "brightyellow": 93,
                "brightblue": 94,
                "brightmagenta": 95,
                "brightcyan": 96,
                "brightwhite": 97,
            }
        )
    if colors >= 256:
        for i in range(256):
            _effects["color%s" % i] = "38;5;%s" % i
            _effects["color%s_background" % i] = "48;5;%s" % i
    if colors >= 16777216:
        _effects = truecoloreffects(_effects)


def configstyles(ui):
    if ui._colormode in ("ansi", "terminfo"):
        _extendcolors(supportedcolors())
    ui._styles.update(_defaultstyles)
    for status, cfgeffects in ui.configitems("color"):
        if "." not in status or status.startswith(("color.", "terminfo.")):
            continue
        cfgeffects = ui.configlist("color", status)
        if cfgeffects:
            good = []
            for e in cfgeffects:
                n = normalizestyle(ui, e)
                if n:
                    good.append(n)
                else:
                    ui.warn(
                        _(
                            "ignoring unknown color/effect %r "
                            "(configured in color.%s)\n"
                        )
                        % (e, status)
                    )
            ui._styles[status] = " ".join(good)


def _activeeffects(ui):
    """Return the effects map for the color mode set on the ui."""
    if ui._colormode == "win32":
        return w32effects
    elif ui._colormode is not None:
        return _effects
    return {}


def valideffect(ui, effect):
    "Determine if the effect is valid or not."
    return (
        (
            isinstance(_activeeffects(ui), truecoloreffects)
            and _truecolorre.match(effect)
        )
        or (not ui._terminfoparams and effect in _activeeffects(ui))
        or (effect in ui._terminfoparams or effect[:-11] in ui._terminfoparams)
    )


def _effect_str(ui, effect):
    """Helper function for render_effects()."""

    bg = False
    if effect.endswith("_background"):
        bg = True
        effect = effect[:-11]
    try:
        attr, val, termcode = ui._terminfoparams[effect]
    except KeyError:
        return ""
    if attr:
        if termcode:
            return termcode
        else:
            return curses.tigetstr(val)
    elif bg:
        return curses.tparm(curses.tigetstr("setab"), val)
    else:
        return curses.tparm(curses.tigetstr("setaf"), val)


def _mergeeffects(text, start, stop):
    """Insert start sequence at every occurrence of stop sequence

    >>> s = _mergeeffects(b'cyan', b'[C]', b'|')
    >>> s = _mergeeffects(s + b'yellow', b'[Y]', b'|')
    >>> s = _mergeeffects(b'ma' + s + b'genta', b'[M]', b'|')
    >>> s = _mergeeffects(b'red' + s, b'[R]', b'|')
    >>> s
    '[R]red[M]ma[Y][C]cyan|[R][M][Y]yellow|[R][M]genta|'
    """
    parts = []
    for t in text.split(stop):
        if not t:
            continue
        parts.extend([start, t, stop])
    return "".join(parts)


def _render_effects(ui, text, effects):
    "Wrap text in commands to turn on each effect."
    if not text:
        return text
    if ui._terminfoparams:
        start = "".join(
            _effect_str(ui, effect) for effect in ["none"] + effects.split()
        )
        stop = _effect_str(ui, "none")
    else:
        activeeffects = _activeeffects(ui)
        start = [pycompat.bytestr(activeeffects[e]) for e in ["none"] + effects.split()]
        start = "\033[" + ";".join(start) + "m"
        stop = "\033[" + pycompat.bytestr(activeeffects["none"]) + "m"
    return _mergeeffects(text, start, stop)


_ansieffectre = re.compile(br"\x1b\[[0-9;]*m")
_truecolorre = re.compile(br"#([0-9A-Fa-f]{3}){1,2}(_background)?")


def stripeffects(text):
    """Strip ANSI control codes which could be inserted by colorlabel()"""
    return _ansieffectre.sub("", text)


def colorlabel(ui, msg, label):
    """add color control code according to the mode"""
    if ui._colormode == "debug":
        if label and msg:
            if msg[-1] == "\n":
                msg = "[%s|%s]\n" % (label, msg[:-1])
            else:
                msg = "[%s|%s]" % (label, msg)
    elif ui._colormode is not None:
        effects = []
        for l in label.split():
            s = ui._styles.get(l, "")
            if ":" in s:
                s = normalizestyle(ui, s)
            if s:
                effects.append(s)
            elif valideffect(ui, l):
                effects.append(l)
        effects = " ".join(effects)
        if effects:
            msg = "\n".join(
                [_render_effects(ui, line, effects) for line in msg.split("\n")]
            )
    return msg


w32effects = None
if pycompat.iswindows:
    import ctypes

    _kernel32 = ctypes.windll.kernel32

    _WORD = ctypes.c_ushort

    _INVALID_HANDLE_VALUE = -1

    class _COORD(ctypes.Structure):
        _fields_ = [("X", ctypes.c_short), ("Y", ctypes.c_short)]

    class _SMALL_RECT(ctypes.Structure):
        _fields_ = [
            ("Left", ctypes.c_short),
            ("Top", ctypes.c_short),
            ("Right", ctypes.c_short),
            ("Bottom", ctypes.c_short),
        ]

    class _CONSOLE_SCREEN_BUFFER_INFO(ctypes.Structure):
        _fields_ = [
            ("dwSize", _COORD),
            ("dwCursorPosition", _COORD),
            ("wAttributes", _WORD),
            ("srWindow", _SMALL_RECT),
            ("dwMaximumWindowSize", _COORD),
        ]

    _STD_OUTPUT_HANDLE = 0xFFFFFFF5  # (DWORD)-11
    _STD_ERROR_HANDLE = 0xFFFFFFF4  # (DWORD)-12

    _FOREGROUND_BLUE = 0x0001
    _FOREGROUND_GREEN = 0x0002
    _FOREGROUND_RED = 0x0004
    _FOREGROUND_INTENSITY = 0x0008

    _BACKGROUND_BLUE = 0x0010
    _BACKGROUND_GREEN = 0x0020
    _BACKGROUND_RED = 0x0040
    _BACKGROUND_INTENSITY = 0x0080

    _COMMON_LVB_REVERSE_VIDEO = 0x4000
    _COMMON_LVB_UNDERSCORE = 0x8000

    # http://msdn.microsoft.com/en-us/library/ms682088%28VS.85%29.aspx
    w32effects = {
        "none": -1,
        "black": 0,
        "red": _FOREGROUND_RED,
        "green": _FOREGROUND_GREEN,
        "yellow": _FOREGROUND_RED | _FOREGROUND_GREEN,
        "blue": _FOREGROUND_BLUE,
        "magenta": _FOREGROUND_BLUE | _FOREGROUND_RED,
        "cyan": _FOREGROUND_BLUE | _FOREGROUND_GREEN,
        "white": _FOREGROUND_RED | _FOREGROUND_GREEN | _FOREGROUND_BLUE,
        "bold": _FOREGROUND_INTENSITY,
        "brightblack": 0,
        "brightred": _FOREGROUND_RED | _FOREGROUND_INTENSITY,
        "brightgreen": _FOREGROUND_GREEN | _FOREGROUND_INTENSITY,
        "brightyellow": (_FOREGROUND_RED | _FOREGROUND_GREEN | _FOREGROUND_INTENSITY),
        "brightblue": _FOREGROUND_BLUE | _FOREGROUND_INTENSITY,
        "brightmagenta": (_FOREGROUND_BLUE | _FOREGROUND_RED | _FOREGROUND_INTENSITY),
        "brightcyan": (_FOREGROUND_BLUE | _FOREGROUND_GREEN | _FOREGROUND_INTENSITY),
        "brightwhite": (
            _FOREGROUND_RED
            | _FOREGROUND_GREEN
            | _FOREGROUND_BLUE
            | _FOREGROUND_INTENSITY
        ),
        "black_background": 0x100,  # unused value > 0x0f
        "red_background": _BACKGROUND_RED,
        "green_background": _BACKGROUND_GREEN,
        "yellow_background": _BACKGROUND_RED | _BACKGROUND_GREEN,
        "blue_background": _BACKGROUND_BLUE,
        "purple_background": _BACKGROUND_BLUE | _BACKGROUND_RED,
        "cyan_background": _BACKGROUND_BLUE | _BACKGROUND_GREEN,
        "white_background": (_BACKGROUND_RED | _BACKGROUND_GREEN | _BACKGROUND_BLUE),
        "bold_background": _BACKGROUND_INTENSITY,
        "underline": _COMMON_LVB_UNDERSCORE,  # double-byte charsets only
        "inverse": _COMMON_LVB_REVERSE_VIDEO,  # double-byte charsets only
    }

    passthrough = {
        _FOREGROUND_INTENSITY,
        _BACKGROUND_INTENSITY,
        _COMMON_LVB_UNDERSCORE,
        _COMMON_LVB_REVERSE_VIDEO,
    }

    stdout = _kernel32.GetStdHandle(
        _STD_OUTPUT_HANDLE
    )  # don't close the handle returned
    if stdout is None or stdout == _INVALID_HANDLE_VALUE:
        w32effects = None
    else:
        csbi = _CONSOLE_SCREEN_BUFFER_INFO()
        if not _kernel32.GetConsoleScreenBufferInfo(stdout, ctypes.byref(csbi)):
            # stdout may not support GetConsoleScreenBufferInfo()
            # when called from subprocess or redirected
            w32effects = None
        else:
            origattr = csbi.wAttributes
            ansire = re.compile(
                "\033\\[([^m]*)m([^\033]*)(.*)", re.MULTILINE | re.DOTALL
            )

    def win32print(ui, writefunc, *msgs, **opts):
        for text in msgs:
            _win32print(ui, text, writefunc, **opts)

    def _win32print(ui, text, writefunc, **opts):
        attr = origattr

        def mapcolor(val, attr):
            if val == -1:
                return origattr
            elif val in passthrough:
                return attr | val
            elif val > 0x0F:
                return (val & 0x70) | (attr & 0x8F)
            else:
                return (val & 0x07) | (attr & 0xF8)

        # Look for ANSI-like codes embedded in text
        m = re.match(ansire, text)
        if m:
            try:
                while m:
                    for sattr in m.group(1).split(";"):
                        if sattr:
                            attr = mapcolor(int(sattr), attr)
                    ui.flush()
                    _kernel32.SetConsoleTextAttribute(stdout, attr)
                    writefunc(m.group(2), **opts)
                    m = re.match(ansire, m.group(3))
            finally:
                # Explicitly reset original attributes
                ui.flush()
                _kernel32.SetConsoleTextAttribute(stdout, origattr)
        else:
            writefunc(text, **opts)


def supportedcolors():
    """Return the number of colors likely supported by the terminal

    Usually it's one of 8, 16, 256.
    """
    # HGCOLORS can override the decision
    env = encoding.environ
    if "HGCOLORS" in env:
        colors = 8
        try:
            colors = int(env["HGCOLORS"])
        except Exception:
            pass
        return colors

    # Colors reported by terminfo. Might be smaller than the real value.
    ticolors = 8
    if curses:
        try:
            curses.setupterm()
            ticolors = curses.tigetnum("colors")
        except Exception:
            pass

    # Guess the real number of colors supported.
    # Windows supports 16 colors.
    if pycompat.iswindows:
        realcolors = 16
    # Emacs has issues with 16 or 256 colors.
    elif env.get("INSIDE_EMACS"):
        realcolors = 8
    # Detecting Terminal features is hard. "infocmp" seems to be a "standard"
    # way to do it. But it can often miss real terminal capabilities.
    #
    # Tested on real tmux (2.2), mosh (1.3.0), screen (4.04) from Linux (xfce)
    # and OS X Terminal.app and iTerm 2. Every terminal support 256 colors
    # except for "screen". "screen" also uses underline for "dim".
    #
    # screen can actually support 256 colors if it's started with TERM set to
    # "xterm-256color". In that case, screen will set TERM to
    # "screen.xterm-256color". Tmux sets TERM to "screen" by default. But it
    # also sets TMUX.
    elif env.get("TERM") == "screen" and "TMUX" not in env:
        realcolors = 16
    # If COLORTERM is set to indicate a truecolor terminal, believe it.
    elif env.get("COLORTERM") in ("truecolor", "24bit"):
        realcolors = 16777216
    # Otherwise, pretend to support 256 colors.
    else:
        realcolors = 256

    # terminfo can override "realcolors" upwards.
    return max([realcolors, ticolors])
