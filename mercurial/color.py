# utility for color output for Mercurial commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re

from .i18n import _

from . import (
    encoding,
    pycompat,
    util
)

try:
    import curses
    # Mapping from effect name to terminfo attribute name (or raw code) or
    # color number.  This will also force-load the curses module.
    _baseterminfoparams = {
        'none': (True, 'sgr0', ''),
        'standout': (True, 'smso', ''),
        'underline': (True, 'smul', ''),
        'reverse': (True, 'rev', ''),
        'inverse': (True, 'rev', ''),
        'blink': (True, 'blink', ''),
        'dim': (True, 'dim', ''),
        'bold': (True, 'bold', ''),
        'invisible': (True, 'invis', ''),
        'italic': (True, 'sitm', ''),
        'black': (False, curses.COLOR_BLACK, ''),
        'red': (False, curses.COLOR_RED, ''),
        'green': (False, curses.COLOR_GREEN, ''),
        'yellow': (False, curses.COLOR_YELLOW, ''),
        'blue': (False, curses.COLOR_BLUE, ''),
        'magenta': (False, curses.COLOR_MAGENTA, ''),
        'cyan': (False, curses.COLOR_CYAN, ''),
        'white': (False, curses.COLOR_WHITE, ''),
    }
except ImportError:
    curses = None
    _baseterminfoparams = {}

# start and stop parameters for effects
_effects = {
    'none': 0,
    'black': 30,
    'red': 31,
    'green': 32,
    'yellow': 33,
    'blue': 34,
    'magenta': 35,
    'cyan': 36,
    'white': 37,
    'bold': 1,
    'italic': 3,
    'underline': 4,
    'inverse': 7,
    'dim': 2,
    'black_background': 40,
    'red_background': 41,
    'green_background': 42,
    'yellow_background': 43,
    'blue_background': 44,
    'purple_background': 45,
    'cyan_background': 46,
    'white_background': 47,
    }

_defaultstyles = {
    'grep.match': 'red bold',
    'grep.linenumber': 'green',
    'grep.rev': 'green',
    'grep.change': 'green',
    'grep.sep': 'cyan',
    'grep.filename': 'magenta',
    'grep.user': 'magenta',
    'grep.date': 'magenta',
    'bookmarks.active': 'green',
    'branches.active': 'none',
    'branches.closed': 'black bold',
    'branches.current': 'green',
    'branches.inactive': 'none',
    'diff.changed': 'white',
    'diff.deleted': 'red',
    'diff.deleted.highlight': 'red bold underline',
    'diff.diffline': 'bold',
    'diff.extended': 'cyan bold',
    'diff.file_a': 'red bold',
    'diff.file_b': 'green bold',
    'diff.hunk': 'magenta',
    'diff.inserted': 'green',
    'diff.inserted.highlight': 'green bold underline',
    'diff.tab': '',
    'diff.trailingwhitespace': 'bold red_background',
    'changeset.public': '',
    'changeset.draft': '',
    'changeset.secret': '',
    'diffstat.deleted': 'red',
    'diffstat.inserted': 'green',
    'formatvariant.name.mismatchconfig': 'red',
    'formatvariant.name.mismatchdefault': 'yellow',
    'formatvariant.name.uptodate': 'green',
    'formatvariant.repo.mismatchconfig': 'red',
    'formatvariant.repo.mismatchdefault': 'yellow',
    'formatvariant.repo.uptodate': 'green',
    'formatvariant.config.special': 'yellow',
    'formatvariant.config.default': 'green',
    'formatvariant.default': '',
    'histedit.remaining': 'red bold',
    'ui.prompt': 'yellow',
    'log.changeset': 'yellow',
    'patchbomb.finalsummary': '',
    'patchbomb.from': 'magenta',
    'patchbomb.to': 'cyan',
    'patchbomb.subject': 'green',
    'patchbomb.diffstats': '',
    'rebase.rebased': 'blue',
    'rebase.remaining': 'red bold',
    'resolve.resolved': 'green bold',
    'resolve.unresolved': 'red bold',
    'shelve.age': 'cyan',
    'shelve.newest': 'green bold',
    'shelve.name': 'blue bold',
    'status.added': 'green bold',
    'status.clean': 'none',
    'status.copied': 'none',
    'status.deleted': 'cyan bold underline',
    'status.ignored': 'black bold',
    'status.modified': 'blue bold',
    'status.removed': 'red bold',
    'status.unknown': 'magenta bold underline',
    'tags.normal': 'green',
    'tags.local': 'black bold',
}

def loadcolortable(ui, extname, colortable):
    _defaultstyles.update(colortable)

def _terminfosetup(ui, mode, formatted):
    '''Initialize terminfo data and the terminal if we're in terminfo mode.'''

    # If we failed to load curses, we go ahead and return.
    if curses is None:
        return
    # Otherwise, see what the config file says.
    if mode not in ('auto', 'terminfo'):
        return
    ui._terminfoparams.update(_baseterminfoparams)

    for key, val in ui.configitems('color'):
        if key.startswith('color.'):
            newval = (False, int(val), '')
            ui._terminfoparams[key[6:]] = newval
        elif key.startswith('terminfo.'):
            newval = (True, '', val.replace('\\E', '\x1b'))
            ui._terminfoparams[key[9:]] = newval
    try:
        curses.setupterm()
    except curses.error as e:
        ui._terminfoparams.clear()
        return

    for key, (b, e, c) in ui._terminfoparams.items():
        if not b:
            continue
        if not c and not curses.tigetstr(e):
            # Most terminals don't support dim, invis, etc, so don't be
            # noisy and use ui.debug().
            ui.debug("no terminfo entry for %s\n" % e)
            del ui._terminfoparams[key]
    if not curses.tigetstr('setaf') or not curses.tigetstr('setab'):
        # Only warn about missing terminfo entries if we explicitly asked for
        # terminfo mode and we're in a formatted terminal.
        if mode == "terminfo" and formatted:
            ui.warn(_("no terminfo entry for setab/setaf: reverting to "
              "ECMA-48 color\n"))
        ui._terminfoparams.clear()

def setup(ui):
    """configure color on a ui

    That function both set the colormode for the ui object and read
    the configuration looking for custom colors and effect definitions."""
    mode = _modesetup(ui)
    ui._colormode = mode
    if mode and mode != 'debug':
        configstyles(ui)

def _modesetup(ui):
    if ui.plain('color'):
        return None
    config = ui.config('ui', 'color')
    if config == 'debug':
        return 'debug'

    auto = (config == 'auto')
    always = False
    if not auto and util.parsebool(config):
        # We want the config to behave like a boolean, "on" is actually auto,
        # but "always" value is treated as a special case to reduce confusion.
        if ui.configsource('ui', 'color') == '--color' or config == 'always':
            always = True
        else:
            auto = True

    if not always and not auto:
        return None

    formatted = (always or (encoding.environ.get('TERM') != 'dumb'
                 and ui.formatted()))

    mode = ui.config('color', 'mode')

    # If pager is active, color.pagermode overrides color.mode.
    if getattr(ui, 'pageractive', False):
        mode = ui.config('color', 'pagermode', mode)

    realmode = mode
    if pycompat.iswindows:
        from . import win32

        term = encoding.environ.get('TERM')
        # TERM won't be defined in a vanilla cmd.exe environment.

        # UNIX-like environments on Windows such as Cygwin and MSYS will
        # set TERM. They appear to make a best effort attempt at setting it
        # to something appropriate. However, not all environments with TERM
        # defined support ANSI.
        ansienviron = term and 'xterm' in term

        if mode == 'auto':
            # Since "ansi" could result in terminal gibberish, we error on the
            # side of selecting "win32". However, if w32effects is not defined,
            # we almost certainly don't support "win32", so don't even try.
            # w32ffects is not populated when stdout is redirected, so checking
            # it first avoids win32 calls in a state known to error out.
            if ansienviron or not w32effects or win32.enablevtmode():
                realmode = 'ansi'
            else:
                realmode = 'win32'
        # An empty w32effects is a clue that stdout is redirected, and thus
        # cannot enable VT mode.
        elif mode == 'ansi' and w32effects and not ansienviron:
            win32.enablevtmode()
    elif mode == 'auto':
        realmode = 'ansi'

    def modewarn():
        # only warn if color.mode was explicitly set and we're in
        # a formatted terminal
        if mode == realmode and formatted:
            ui.warn(_('warning: failed to set color mode to %s\n') % mode)

    if realmode == 'win32':
        ui._terminfoparams.clear()
        if not w32effects:
            modewarn()
            return None
    elif realmode == 'ansi':
        ui._terminfoparams.clear()
    elif realmode == 'terminfo':
        _terminfosetup(ui, mode, formatted)
        if not ui._terminfoparams:
            ## FIXME Shouldn't we return None in this case too?
            modewarn()
            realmode = 'ansi'
    else:
        return None

    if always or (auto and formatted):
        return realmode
    return None

def configstyles(ui):
    ui._styles.update(_defaultstyles)
    for status, cfgeffects in ui.configitems('color'):
        if '.' not in status or status.startswith(('color.', 'terminfo.')):
            continue
        cfgeffects = ui.configlist('color', status)
        if cfgeffects:
            good = []
            for e in cfgeffects:
                if valideffect(ui, e):
                    good.append(e)
                else:
                    ui.warn(_("ignoring unknown color/effect %r "
                              "(configured in color.%s)\n")
                            % (e, status))
            ui._styles[status] = ' '.join(good)

def _activeeffects(ui):
    '''Return the effects map for the color mode set on the ui.'''
    if ui._colormode == 'win32':
        return w32effects
    elif ui._colormode is not None:
        return _effects
    return {}

def valideffect(ui, effect):
    'Determine if the effect is valid or not.'
    return ((not ui._terminfoparams and effect in _activeeffects(ui))
             or (effect in ui._terminfoparams
                 or effect[:-11] in ui._terminfoparams))

def _effect_str(ui, effect):
    '''Helper function for render_effects().'''

    bg = False
    if effect.endswith('_background'):
        bg = True
        effect = effect[:-11]
    try:
        attr, val, termcode = ui._terminfoparams[effect]
    except KeyError:
        return ''
    if attr:
        if termcode:
            return termcode
        else:
            return curses.tigetstr(val)
    elif bg:
        return curses.tparm(curses.tigetstr('setab'), val)
    else:
        return curses.tparm(curses.tigetstr('setaf'), val)

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
    return ''.join(parts)

def _render_effects(ui, text, effects):
    'Wrap text in commands to turn on each effect.'
    if not text:
        return text
    if ui._terminfoparams:
        start = ''.join(_effect_str(ui, effect)
                        for effect in ['none'] + effects.split())
        stop = _effect_str(ui, 'none')
    else:
        activeeffects = _activeeffects(ui)
        start = [pycompat.bytestr(activeeffects[e])
                 for e in ['none'] + effects.split()]
        start = '\033[' + ';'.join(start) + 'm'
        stop = '\033[' + pycompat.bytestr(activeeffects['none']) + 'm'
    return _mergeeffects(text, start, stop)

_ansieffectre = re.compile(br'\x1b\[[0-9;]*m')

def stripeffects(text):
    """Strip ANSI control codes which could be inserted by colorlabel()"""
    return _ansieffectre.sub('', text)

def colorlabel(ui, msg, label):
    """add color control code according to the mode"""
    if ui._colormode == 'debug':
        if label and msg:
            if msg[-1] == '\n':
                msg = "[%s|%s]\n" % (label, msg[:-1])
            else:
                msg = "[%s|%s]" % (label, msg)
    elif ui._colormode is not None:
        effects = []
        for l in label.split():
            s = ui._styles.get(l, '')
            if s:
                effects.append(s)
            elif valideffect(ui, l):
                effects.append(l)
        effects = ' '.join(effects)
        if effects:
            msg = '\n'.join([_render_effects(ui, line, effects)
                             for line in msg.split('\n')])
    return msg

w32effects = None
if pycompat.iswindows:
    import ctypes

    _kernel32 = ctypes.windll.kernel32

    _WORD = ctypes.c_ushort

    _INVALID_HANDLE_VALUE = -1

    class _COORD(ctypes.Structure):
        _fields_ = [('X', ctypes.c_short),
                    ('Y', ctypes.c_short)]

    class _SMALL_RECT(ctypes.Structure):
        _fields_ = [('Left', ctypes.c_short),
                    ('Top', ctypes.c_short),
                    ('Right', ctypes.c_short),
                    ('Bottom', ctypes.c_short)]

    class _CONSOLE_SCREEN_BUFFER_INFO(ctypes.Structure):
        _fields_ = [('dwSize', _COORD),
                    ('dwCursorPosition', _COORD),
                    ('wAttributes', _WORD),
                    ('srWindow', _SMALL_RECT),
                    ('dwMaximumWindowSize', _COORD)]

    _STD_OUTPUT_HANDLE = 0xfffffff5 # (DWORD)-11
    _STD_ERROR_HANDLE = 0xfffffff4  # (DWORD)-12

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
        'none': -1,
        'black': 0,
        'red': _FOREGROUND_RED,
        'green': _FOREGROUND_GREEN,
        'yellow': _FOREGROUND_RED | _FOREGROUND_GREEN,
        'blue': _FOREGROUND_BLUE,
        'magenta': _FOREGROUND_BLUE | _FOREGROUND_RED,
        'cyan': _FOREGROUND_BLUE | _FOREGROUND_GREEN,
        'white': _FOREGROUND_RED | _FOREGROUND_GREEN | _FOREGROUND_BLUE,
        'bold': _FOREGROUND_INTENSITY,
        'black_background': 0x100,                  # unused value > 0x0f
        'red_background': _BACKGROUND_RED,
        'green_background': _BACKGROUND_GREEN,
        'yellow_background': _BACKGROUND_RED | _BACKGROUND_GREEN,
        'blue_background': _BACKGROUND_BLUE,
        'purple_background': _BACKGROUND_BLUE | _BACKGROUND_RED,
        'cyan_background': _BACKGROUND_BLUE | _BACKGROUND_GREEN,
        'white_background': (_BACKGROUND_RED | _BACKGROUND_GREEN |
                             _BACKGROUND_BLUE),
        'bold_background': _BACKGROUND_INTENSITY,
        'underline': _COMMON_LVB_UNDERSCORE,  # double-byte charsets only
        'inverse': _COMMON_LVB_REVERSE_VIDEO, # double-byte charsets only
    }

    passthrough = {_FOREGROUND_INTENSITY,
                   _BACKGROUND_INTENSITY,
                   _COMMON_LVB_UNDERSCORE,
                   _COMMON_LVB_REVERSE_VIDEO}

    stdout = _kernel32.GetStdHandle(
                  _STD_OUTPUT_HANDLE)  # don't close the handle returned
    if stdout is None or stdout == _INVALID_HANDLE_VALUE:
        w32effects = None
    else:
        csbi = _CONSOLE_SCREEN_BUFFER_INFO()
        if not _kernel32.GetConsoleScreenBufferInfo(
                    stdout, ctypes.byref(csbi)):
            # stdout may not support GetConsoleScreenBufferInfo()
            # when called from subprocess or redirected
            w32effects = None
        else:
            origattr = csbi.wAttributes
            ansire = re.compile('\033\[([^m]*)m([^\033]*)(.*)',
                                re.MULTILINE | re.DOTALL)

    def win32print(ui, writefunc, *msgs, **opts):
        for text in msgs:
            _win32print(ui, text, writefunc, **opts)

    def _win32print(ui, text, writefunc, **opts):
        label = opts.get(r'label', '')
        attr = origattr

        def mapcolor(val, attr):
            if val == -1:
                return origattr
            elif val in passthrough:
                return attr | val
            elif val > 0x0f:
                return (val & 0x70) | (attr & 0x8f)
            else:
                return (val & 0x07) | (attr & 0xf8)

        # determine console attributes based on labels
        for l in label.split():
            style = ui._styles.get(l, '')
            for effect in style.split():
                try:
                    attr = mapcolor(w32effects[effect], attr)
                except KeyError:
                    # w32effects could not have certain attributes so we skip
                    # them if not found
                    pass
        # hack to ensure regexp finds data
        if not text.startswith('\033['):
            text = '\033[m' + text

        # Look for ANSI-like codes embedded in text
        m = re.match(ansire, text)

        try:
            while m:
                for sattr in m.group(1).split(';'):
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
