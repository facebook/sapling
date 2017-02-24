# utility for color output for Mercurial commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .i18n import _

from . import pycompat

try:
    import curses
    # Mapping from effect name to terminfo attribute name (or raw code) or
    # color number.  This will also force-load the curses module.
    _terminfo_params = {'none': (True, 'sgr0', ''),
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
                        'white': (False, curses.COLOR_WHITE, '')}
except ImportError:
    curses = None
    _terminfo_params = {}

# start and stop parameters for effects
_effects = {'none': 0,
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
            'white_background': 47}

_styles = {'grep.match': 'red bold',
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
           'diff.diffline': 'bold',
           'diff.extended': 'cyan bold',
           'diff.file_a': 'red bold',
           'diff.file_b': 'green bold',
           'diff.hunk': 'magenta',
           'diff.inserted': 'green',
           'diff.tab': '',
           'diff.trailingwhitespace': 'bold red_background',
           'changeset.public' : '',
           'changeset.draft' : '',
           'changeset.secret' : '',
           'diffstat.deleted': 'red',
           'diffstat.inserted': 'green',
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
           'tags.local': 'black bold'}

def loadcolortable(ui, extname, colortable):
    _styles.update(colortable)

def configstyles(ui):
    for status, cfgeffects in ui.configitems('color'):
        if '.' not in status or status.startswith(('color.', 'terminfo.')):
            continue
        cfgeffects = ui.configlist('color', status)
        if cfgeffects:
            good = []
            for e in cfgeffects:
                if valideffect(e):
                    good.append(e)
                else:
                    ui.warn(_("ignoring unknown color/effect %r "
                              "(configured in color.%s)\n")
                            % (e, status))
            _styles[status] = ' '.join(good)

def valideffect(effect):
    'Determine if the effect is valid or not.'
    return ((not _terminfo_params and effect in _effects)
             or (effect in _terminfo_params
                 or effect[:-11] in _terminfo_params))

def _effect_str(effect):
    '''Helper function for render_effects().'''

    bg = False
    if effect.endswith('_background'):
        bg = True
        effect = effect[:-11]
    try:
        attr, val, termcode = _terminfo_params[effect]
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

def _render_effects(text, effects):
    'Wrap text in commands to turn on each effect.'
    if not text:
        return text
    if _terminfo_params:
        start = ''.join(_effect_str(effect)
                        for effect in ['none'] + effects.split())
        stop = _effect_str('none')
    else:
        start = [str(_effects[e]) for e in ['none'] + effects.split()]
        start = '\033[' + ';'.join(start) + 'm'
        stop = '\033[' + str(_effects['none']) + 'm'
    return ''.join([start, text, stop])

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
            s = _styles.get(l, '')
            if s:
                effects.append(s)
            elif valideffect(l):
                effects.append(l)
        effects = ' '.join(effects)
        if effects:
            msg = '\n'.join([_render_effects(line, effects)
                             for line in msg.split('\n')])
    return msg

w32effects = None
if pycompat.osname == 'nt':
    import ctypes
    import re

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

    passthrough = set([_FOREGROUND_INTENSITY,
                       _BACKGROUND_INTENSITY,
                       _COMMON_LVB_UNDERSCORE,
                       _COMMON_LVB_REVERSE_VIDEO])

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

    def win32print(writefunc, *msgs, **opts):
        for text in msgs:
            _win32print(text, writefunc, **opts)

    def _win32print(text, writefunc, **opts):
        label = opts.get('label', '')
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
            style = _styles.get(l, '')
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
                _kernel32.SetConsoleTextAttribute(stdout, attr)
                writefunc(m.group(2), **opts)
                m = re.match(ansire, m.group(3))
        finally:
            # Explicitly reset original attributes
            _kernel32.SetConsoleTextAttribute(stdout, origattr)
