# utility for color output for Mercurial commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .i18n import _

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
    if not _terminfo_params:
        start = [str(_effects[e]) for e in ['none'] + effects.split()]
        start = '\033[' + ';'.join(start) + 'm'
        stop = '\033[' + str(_effects['none']) + 'm'
    else:
        start = ''.join(_effect_str(effect)
                        for effect in ['none'] + effects.split())
        stop = _effect_str('none')
    return ''.join([start, text, stop])
