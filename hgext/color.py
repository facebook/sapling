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

This extension modifies the status command to add color to its output
to reflect file status, the qseries command to add color to reflect
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
'''

import os, sys

from mercurial import cmdutil, commands, extensions, error
from mercurial.i18n import _

# start and stop parameters for effects
_effect_params = {'none': 0,
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
                  'black_background': 40,
                  'red_background': 41,
                  'green_background': 42,
                  'yellow_background': 43,
                  'blue_background': 44,
                  'purple_background': 45,
                  'cyan_background': 46,
                  'white_background': 47}

def render_effects(text, effects):
    'Wrap text in commands to turn on each effect.'
    start = [str(_effect_params[e]) for e in ['none'] + effects]
    start = '\033[' + ';'.join(start) + 'm'
    stop = '\033[' + str(_effect_params['none']) + 'm'
    return ''.join([start, text, stop])

def colorstatus(orig, ui, repo, *pats, **opts):
    '''run the status command with colored output'''

    delimiter = opts['print0'] and '\0' or '\n'

    nostatus = opts.get('no_status')
    opts['no_status'] = False
    # run status and capture its output
    ui.pushbuffer()
    retval = orig(ui, repo, *pats, **opts)
    # filter out empty strings
    lines_with_status = [ line for line in ui.popbuffer().split(delimiter) if line ]

    if nostatus:
        lines = [l[2:] for l in lines_with_status]
    else:
        lines = lines_with_status

    # apply color to output and display it
    for i in xrange(len(lines)):
        status = _status_abbreviations[lines_with_status[i][0]]
        effects = _status_effects[status]
        if effects:
            lines[i] = render_effects(lines[i], effects)
        ui.write(lines[i] + delimiter)
    return retval

_status_abbreviations = { 'M': 'modified',
                          'A': 'added',
                          'R': 'removed',
                          '!': 'deleted',
                          '?': 'unknown',
                          'I': 'ignored',
                          'C': 'clean',
                          ' ': 'copied', }

_status_effects = { 'modified': ['blue', 'bold'],
                    'added': ['green', 'bold'],
                    'removed': ['red', 'bold'],
                    'deleted': ['cyan', 'bold', 'underline'],
                    'unknown': ['magenta', 'bold', 'underline'],
                    'ignored': ['black', 'bold'],
                    'clean': ['none'],
                    'copied': ['none'], }

def colorqseries(orig, ui, repo, *dummy, **opts):
    '''run the qseries command with colored output'''
    ui.pushbuffer()
    retval = orig(ui, repo, **opts)
    patchlines = ui.popbuffer().splitlines()
    patchnames = repo.mq.series

    for patch, patchname in zip(patchlines, patchnames):
        if opts['missing']:
            effects = _patch_effects['missing']
        # Determine if patch is applied.
        elif [ applied for applied in repo.mq.applied
               if patchname == applied.name ]:
            effects = _patch_effects['applied']
        else:
            effects = _patch_effects['unapplied']

        patch = patch.replace(patchname, render_effects(patchname, effects), 1)
        ui.write(patch + '\n')
    return retval

_patch_effects = { 'applied': ['blue', 'bold', 'underline'],
                    'missing': ['red', 'bold'],
                    'unapplied': ['black', 'bold'], }
def colorwrap(orig, *args):
    '''wrap ui.write for colored diff output'''
    def _colorize(s):
        lines = s.split('\n')
        for i, line in enumerate(lines):
            stripline = line
            if line and line[0] in '+-':
                # highlight trailing whitespace, but only in changed lines
                stripline = line.rstrip()
            for prefix, style in _diff_prefixes:
                if stripline.startswith(prefix):
                    lines[i] = render_effects(stripline, _diff_effects[style])
                    break
            if line != stripline:
                lines[i] += render_effects(
                    line[len(stripline):], _diff_effects['trailingwhitespace'])
        return '\n'.join(lines)
    orig(*[_colorize(s) for s in args])

def colorshowpatch(orig, self, node):
    '''wrap cmdutil.changeset_printer.showpatch with colored output'''
    oldwrite = extensions.wrapfunction(self.ui, 'write', colorwrap)
    try:
        orig(self, node)
    finally:
        self.ui.write = oldwrite

def colordiffstat(orig, s):
    lines = s.split('\n')
    for i, line in enumerate(lines):
        if line and line[-1] in '+-':
            name, graph = line.rsplit(' ', 1)
            graph = graph.replace('-',
                        render_effects('-', _diff_effects['deleted']))
            graph = graph.replace('+',
                        render_effects('+', _diff_effects['inserted']))
            lines[i] = ' '.join([name, graph])
    orig('\n'.join(lines))

def colordiff(orig, ui, repo, *pats, **opts):
    '''run the diff command with colored output'''
    if opts.get('stat'):
        wrapper = colordiffstat
    else:
        wrapper = colorwrap
    oldwrite = extensions.wrapfunction(ui, 'write', wrapper)
    try:
        orig(ui, repo, *pats, **opts)
    finally:
        ui.write = oldwrite

_diff_prefixes = [('diff', 'diffline'),
                  ('copy', 'extended'),
                  ('rename', 'extended'),
                  ('old', 'extended'),
                  ('new', 'extended'),
                  ('deleted', 'extended'),
                  ('---', 'file_a'),
                  ('+++', 'file_b'),
                  ('@', 'hunk'),
                  ('-', 'deleted'),
                  ('+', 'inserted')]

_diff_effects = {'diffline': ['bold'],
                 'extended': ['cyan', 'bold'],
                 'file_a': ['red', 'bold'],
                 'file_b': ['green', 'bold'],
                 'hunk': ['magenta'],
                 'deleted': ['red'],
                 'inserted': ['green'],
                 'changed': ['white'],
                 'trailingwhitespace': ['bold', 'red_background']}

def extsetup(ui):
    '''Initialize the extension.'''
    _setupcmd(ui, 'diff', commands.table, colordiff, _diff_effects)
    _setupcmd(ui, 'incoming', commands.table, None, _diff_effects)
    _setupcmd(ui, 'log', commands.table, None, _diff_effects)
    _setupcmd(ui, 'outgoing', commands.table, None, _diff_effects)
    _setupcmd(ui, 'tip', commands.table, None, _diff_effects)
    _setupcmd(ui, 'status', commands.table, colorstatus, _status_effects)

    try:
        mq = extensions.find('mq')
        _setupcmd(ui, 'qdiff', mq.cmdtable, colordiff, _diff_effects)
        _setupcmd(ui, 'qseries', mq.cmdtable, colorqseries, _patch_effects)
    except KeyError:
        mq = None

    try:
        rec = extensions.find('record')
        _setupcmd(ui, 'record', rec.cmdtable, colordiff, _diff_effects)
    except KeyError:
        rec = None

    if mq and rec:
        _setupcmd(ui, 'qrecord', rec.cmdtable, colordiff, _diff_effects)


def _setupcmd(ui, cmd, table, func, effectsmap):
    '''patch in command to command table and load effect map'''
    def nocolor(orig, *args, **opts):

        if (opts['no_color'] or opts['color'] == 'never' or
            (opts['color'] == 'auto' and (os.environ.get('TERM') == 'dumb'
                                          or not sys.__stdout__.isatty()))):
            return orig(*args, **opts)

        oldshowpatch = extensions.wrapfunction(cmdutil.changeset_printer,
                                               'showpatch', colorshowpatch)
        try:
            if func is not None:
                return func(orig, *args, **opts)
            return orig(*args, **opts)
        finally:
            cmdutil.changeset_printer.showpatch = oldshowpatch

    entry = extensions.wrapcommand(table, cmd, nocolor)
    entry[1].extend([
        ('', 'color', 'auto', _("when to colorize (always, auto, or never)")),
        ('', 'no-color', None, _("don't colorize output (DEPRECATED)")),
    ])

    for status in effectsmap:
        configkey = cmd + '.' + status
        effects = ui.configlist('color', configkey)
        if effects:
            good = []
            for e in effects:
                if e in _effect_params:
                    good.append(e)
                else:
                    ui.warn(_("ignoring unknown color/effect %r "
                              "(configured in color.%s)\n")
                            % (e, configkey))
            effectsmap[status] = good
