# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# utility for color output for Mercurial commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re
from typing import Dict, List, Optional, Pattern, Union

from . import encoding, pycompat, util
from .i18n import _
from .pycompat import encodeutf8


try:
    import curses

    curses.COLOR_BLACK
except (ImportError, AttributeError):
    curses = None

# start and stop parameters for effects
_defaulteffects = {
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
_effects: Dict[str, int] = {}

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
    "blackbox.session.3": "brightblue:blue",
    "blame.age.1hour": "#ffe:color231:bold",
    "blame.age.1day": "#eea:color230:bold",
    "blame.age.7day": "#dd5:color229:brightyellow:yellow",
    "blame.age.30day": "#cc3:color228:brightyellow:yellow",
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
    "changeset.public": "yellow",
    "changeset.draft": "brightyellow:yellow bold",
    "changeset.secret": "brightyellow:yellow",
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
    "log.changeset": "",
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
    "ui.metrics": "#777:color242:dim",
    "ui.prefix.component": "cyan",
    "ui.prefix.error": "brightred:red",
    "ui.prefix.notice": "yellow",
    "testing.divider": "brightblack:none",
    "testing.lineloc": "brightblue:blue",
    "testing.source": "none",
    "testing.exceeded": "cyan",
}


def loadcolortable(ui, extname, colortable) -> None:
    _defaultstyles.update(colortable)


def setup(ui) -> None:
    """configure color on a ui

    That function both set the colormode for the ui object and read
    the configuration looking for custom colors and effect definitions."""
    mode = _modesetup(ui)
    ui._colormode = mode
    if mode and mode != "debug":
        configstyles(ui)


def _modesetup(ui) -> Optional[str]:
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

    havecolors = always or (
        encoding.environ.get("TERM") != "dumb" and ui.terminaloutput()
    )

    if pycompat.iswindows:
        from . import win32

        if not (util.istest() or win32.enablevtmode()):
            if not ui.pageractive:
                if havecolors and not always:
                    ui.debug("couldn't enable VT mode, disabling colors\n")
                if not always:
                    return None

    if always or (auto and havecolors):
        return "ansi"
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


def _extendcolors(colors) -> None:
    # see https://en.wikipedia.org/wiki/ANSI_escape_code
    global _effects
    _effects = _defaulteffects.copy()
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
            # pyre-fixme[6]: For 2nd param expected `int` but got `str`.
            _effects["color%s" % i] = "38;5;%s" % i
            # pyre-fixme[6]: For 2nd param expected `int` but got `str`.
            _effects["color%s_background" % i] = "48;5;%s" % i
    if colors >= 16777216:
        _effects = truecoloreffects(_effects)


def configstyles(ui) -> None:
    if ui._colormode == "ansi":
        _extendcolors(supportedcolors(ui))
    ui._styles.update(_defaultstyles)
    for status, cfgeffects in ui.configitems("color"):
        if "." not in status or status.startswith("color."):
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
    if ui._colormode is not None:
        return _effects
    return {}


def valideffect(ui, effect) -> bool:
    "Determine if the effect is valid or not."
    return all(
        (isinstance(_activeeffects(ui), truecoloreffects) and _truecolorre.match(e))
        or (e in _activeeffects(ui))
        for e in effect.split("+")
    )


def _mergeeffects(
    text: "Union[str, bytes]", start: str, stop: str, usebytes: bool = False
) -> "Union[str, bytes]":
    """Insert start sequence at every occurrence of stop sequence

    >>> s = _mergeeffects('cyan', '[C]', '|')
    >>> s = _mergeeffects(s + 'yellow', '[Y]', '|')
    >>> s = _mergeeffects('ma' + s + 'genta', '[M]', '|')
    >>> s = _mergeeffects('red' + s, '[R]', '|')
    >>> s
    '[R]red[M]ma[Y][C]cyan|[R][M][Y]yellow|[R][M]genta|'
    """
    parts = []
    if usebytes:
        assert isinstance(text, bytes)
        for t in text.split(encodeutf8(stop)):
            if not t:
                continue
            parts.extend([encodeutf8(start), t, encodeutf8(stop)])
        return b"".join(parts)
    else:
        assert isinstance(text, str)
        for t in text.split(stop):
            if not t:
                continue
            parts.extend([start, t, stop])
        return "".join(parts)


def _render_effects(ui, text, effects: List[str], usebytes: bool = False):
    "Wrap text in commands to turn on each effect."
    if not text:
        return text
    activeeffects = _activeeffects(ui)
    # pyre-fixme[16]: `List` has no attribute `split`.
    effects = ["none"] + [e for effect in effects.split() for e in effect.split("+")]
    start = [pycompat.bytestr(activeeffects[e]) for e in effects]
    start = "\033[" + ";".join(start) + "m"
    stop = "\033[" + pycompat.bytestr(activeeffects["none"]) + "m"
    return _mergeeffects(text, start, stop, usebytes=usebytes)


_ansieffectre: Pattern[str] = re.compile(r"\x1b\[[0-9;]*m")
_truecolorre: Pattern[str] = re.compile(r"#([0-9A-Fa-f]{3}){1,2}(_background)?")


def stripeffects(text):
    """Strip ANSI control codes which could be inserted by colorlabel()"""
    return _ansieffectre.sub("", text)


def colorlabel(ui, msg, label, usebytes: bool = False) -> Union[bytes, str]:
    """add color control code according to the mode"""
    if ui._colormode == "debug":
        if label and msg:
            if msg[-1] == "\n":
                if usebytes:
                    msg = b"[%s|%s]\n" % (encodeutf8(label), msg[:-1])
                else:
                    msg = "[%s|%s]\n" % (label, msg[:-1])
            else:
                if usebytes:
                    msg = b"[%s|%s]" % (encodeutf8(label), msg)
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
            if usebytes:
                msg = b"\n".join(
                    [
                        # pyre-fixme[6]: For 3rd param expected `List[str]` but got
                        #  `str`.
                        _render_effects(ui, line, effects, usebytes=True)
                        for line in msg.split(b"\n")
                    ]
                )
            else:
                msg = "\n".join(
                    # pyre-fixme[6]: For 3rd param expected `List[str]` but got `str`.
                    [_render_effects(ui, line, effects) for line in msg.split("\n")]
                )
    return msg


def supportedcolors(ui):
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
    # ConEmu normalizes 256 colors incorrectly. Limit it to 16 colors.
    if "ConEmuPID" in env:
        realcolors = 16
    # Emacs has issues with 16 or 256 colors.
    if env.get("INSIDE_EMACS"):
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
    # XXX: The gitbash pager doesn't support more than 8 colors, remove once
    # we switched over to our embedded less pager.
    elif pycompat.iswindows and ui.pageractive:
        realcolors = 8
    # Otherwise, pretend to support 256 colors.
    else:
        realcolors = 256

    # terminfo can override "realcolors" upwards.
    return max([realcolors, ticolors])
