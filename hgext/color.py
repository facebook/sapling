# color.py color output for the status and qseries commands
#
# Copyright (C) 2007 Kevin Christen <kevin.christen@gmail.com>
#
# This program is free software; you can redistribute it and/or modify it
# under the terms of the GNU General Public License as published by the
# Free Software Foundation; either version 2 of the License, or (at your
# option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General
# Public License for more details.
#
# You should have received a copy of the GNU General Public License along
# with this program; if not, write to the Free Software Foundation, Inc.,
# 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.

'''colorize output from some commands

This extension modifies the status and resolve commands to add color to their
output to reflect file status, the qseries command to add color to reflect
patch status (applied, unapplied, missing), and to diff-related
commands to highlight additions, removals, diff headers, and trailing
whitespace.

Other effects in addition to color, like bold and underlined text, are
also available. Effects are rendered with the ECMA-48 SGR control
function (aka ANSI escape codes). This module also provides the
render_text function, which can be used to add effects to any text.

Default effects may be overridden from the .hgrc file::

  [color]
  status.modified = blue bold underline red_background
  status.added = green bold
  status.removed = red bold blue_background
  status.deleted = cyan bold underline
  status.unknown = magenta bold underline
  status.ignored = black bold

  # 'none' turns off all effects
  status.clean = none
  status.copied = none

  qseries.applied = blue bold underline
  qseries.unapplied = black bold
  qseries.missing = red bold

  diff.diffline = bold
  diff.extended = cyan bold
  diff.file_a = red bold
  diff.file_b = green bold
  diff.hunk = magenta
  diff.deleted = red
  diff.inserted = green
  diff.changed = white
  diff.trailingwhitespace = bold red_background

  resolve.unresolved = red bold
  resolve.resolved = green bold

  bookmarks.current = green
'''

import os, sys

from mercurial import commands, dispatch, extensions
from mercurial.i18n import _
from mercurial.ui import ui as uicls

# start and stop parameters for effects
_effects = {'none': 0, 'black': 30, 'red': 31, 'green': 32, 'yellow': 33,
            'blue': 34, 'magenta': 35, 'cyan': 36, 'white': 37, 'bold': 1,
            'italic': 3, 'underline': 4, 'inverse': 7,
            'black_background': 40, 'red_background': 41,
            'green_background': 42, 'yellow_background': 43,
            'blue_background': 44, 'purple_background': 45,
            'cyan_background': 46, 'white_background': 47}

_styles = {'grep.match': 'red bold',
           'diff.changed': 'white',
           'diff.deleted': 'red',
           'diff.diffline': 'bold',
           'diff.extended': 'cyan bold',
           'diff.file_a': 'red bold',
           'diff.file_b': 'green bold',
           'diff.hunk': 'magenta',
           'diff.inserted': 'green',
           'diff.trailingwhitespace': 'bold red_background',
           'diffstat.deleted': 'red',
           'diffstat.inserted': 'green',
           'log.changeset': 'yellow',
           'resolve.resolved': 'green bold',
           'resolve.unresolved': 'red bold',
           'status.added': 'green bold',
           'status.clean': 'none',
           'status.copied': 'none',
           'status.deleted': 'cyan bold underline',
           'status.ignored': 'black bold',
           'status.modified': 'blue bold',
           'status.removed': 'red bold',
           'status.unknown': 'magenta bold underline'}


def render_effects(text, effects):
    'Wrap text in commands to turn on each effect.'
    if not text:
        return text
    start = [str(_effects[e]) for e in ['none'] + effects.split()]
    start = '\033[' + ';'.join(start) + 'm'
    stop = '\033[' + str(_effects['none']) + 'm'
    return ''.join([start, text, stop])

def extstyles():
    for name, ext in extensions.extensions():
        _styles.update(getattr(ext, 'colortable', {}))

def configstyles(ui):
    for status, cfgeffects in ui.configitems('color'):
        if '.' not in status:
            continue
        cfgeffects = ui.configlist('color', status)
        if cfgeffects:
            good = []
            for e in cfgeffects:
                if e in _effects:
                    good.append(e)
                else:
                    ui.warn(_("ignoring unknown color/effect %r "
                              "(configured in color.%s)\n")
                            % (e, status))
            _styles[status] = ' '.join(good)

_buffers = None
def style(msg, label):
    effects = []
    for l in label.split():
        s = _styles.get(l, '')
        if s:
            effects.append(s)
    effects = ''.join(effects)
    if effects:
        return '\n'.join([render_effects(s, effects)
                          for s in msg.split('\n')])
    return msg

def popbuffer(orig, labeled=False):
    global _buffers
    if labeled:
        return ''.join(style(a, label) for a, label in _buffers.pop())
    return ''.join(a for a, label in _buffers.pop())

def write(orig, *args, **opts):
    label = opts.get('label', '')
    global _buffers
    if _buffers:
        _buffers[-1].extend([(str(a), label) for a in args])
    else:
        return orig(*[style(str(a), label) for a in args], **opts)

def write_err(orig, *args, **opts):
    label = opts.get('label', '')
    return orig(*[style(str(a), label) for a in args], **opts)

def uisetup(ui):
    def colorcmd(orig, ui_, opts, cmd, cmdfunc):
        if (opts['color'] == 'always' or
            (opts['color'] == 'auto' and (os.environ.get('TERM') != 'dumb'
                                          and sys.__stdout__.isatty()))):
            global _buffers
            _buffers = ui_._buffers
            extensions.wrapfunction(ui_, 'popbuffer', popbuffer)
            extensions.wrapfunction(ui_, 'write', write)
            extensions.wrapfunction(ui_, 'write_err', write_err)
            ui_.label = style
            extstyles()
            configstyles(ui)
        return orig(ui_, opts, cmd, cmdfunc)
    extensions.wrapfunction(dispatch, '_runcommand', colorcmd)

commands.globalopts.append(('', 'color', 'auto',
                            _("when to colorize (always, auto, or never)")))
