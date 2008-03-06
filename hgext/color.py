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

'''add color output to the status and qseries commands

This extension modifies the status command to add color to its output to
reflect file status, and the qseries command to add color to reflect patch
status (applied, unapplied, missing).  Other effects in addition to color,
like bold and underlined text, are also available.  Effects are rendered
with the ECMA-48 SGR control function (aka ANSI escape codes).  This module
also provides the render_text function, which can be used to add effects to
any text.

To enable this extension, add this to your .hgrc file:
[extensions]
color =

Default effects my be overriden from the .hgrc file:

[color]
status.modified = blue bold underline red_background
status.added = green bold
status.removed = red bold blue_background
status.deleted = cyan bold underline
status.unknown = magenta bold underline
status.ignored = black bold

 'none' turns off all effects
status.clean = none
status.copied = none

qseries.applied = blue bold underline
qseries.unapplied = black bold
qseries.missing = red bold
'''

import re, sys

from mercurial import commands, cmdutil
from mercurial.i18n import _

# start and stop parameters for effects
_effect_params = { 'none': (0, 0),
                   'black': (30, 39),
                   'red': (31, 39),
                   'green': (32, 39),
                   'yellow': (33, 39),
                   'blue': (34, 39),
                   'magenta': (35, 39),
                   'cyan': (36, 39),
                   'white': (37, 39),
                   'bold': (1, 22),
                   'italic': (3, 23),
                   'underline': (4, 24),
                   'inverse': (7, 27),
                   'black_background': (40, 49),
                   'red_background': (41, 49),
                   'green_background': (42, 49),
                   'yellow_background': (43, 49),
                   'blue_background': (44, 49),
                   'purple_background': (45, 49),
                   'cyan_background': (46, 49),
                   'white_background': (47, 49), }

def render_effects(text, *effects):
    'Wrap text in commands to turn on each effect.'
    start = []
    stop = []
    for effect in effects:
        start.append(str(_effect_params[effect][0]))
        stop.append(str(_effect_params[effect][1]))
    start = '\033[' + ';'.join(start) + 'm'
    stop = '\033[' + ';'.join(stop) + 'm'
    return start + text + stop

def colorstatus(statusfunc, ui, repo, *pats, **opts):
    '''run the status command with colored output'''

    delimiter = opts['print0'] and '\0' or '\n'

    # run status and capture it's output
    ui.pushbuffer()
    retval = statusfunc(ui, repo, *pats, **opts)
    # filter out empty strings
    lines = [ line for line in ui.popbuffer().split(delimiter) if line ]

    if opts['no_status']:
        # if --no-status, run the command again without that option to get
        # output with status abbreviations
        opts['no_status'] = False
        ui.pushbuffer()
        statusfunc(ui, repo, *pats, **opts)
        # filter out empty strings
        lines_with_status = [ line for
                              line in ui.popbuffer().split(delimiter) if line ]
    else:
        lines_with_status = lines

    # apply color to output and display it
    for i in xrange(0, len(lines)):
        status = _status_abbreviations[lines_with_status[i][0]]
        effects = _status_effects[status]
        if effects:
            lines[i] = render_effects(lines[i], *effects)
        sys.stdout.write(lines[i] + delimiter)
    return retval

_status_abbreviations = { 'M': 'modified',
                          'A': 'added',
                          'R': 'removed',
                          '!': 'deleted',
                          '?': 'unknown',
                          'I': 'ignored',
                          'C': 'clean',
                          ' ': 'copied', }

_status_effects = { 'modified': ('blue', 'bold'),
                    'added': ('green', 'bold'),
                    'removed': ('red', 'bold'),
                    'deleted': ('cyan', 'bold', 'underline'),
                    'unknown': ('magenta', 'bold', 'underline'),
                    'ignored': ('black', 'bold'),
                    'clean': ('none', ),
                    'copied': ('none', ), }

def colorqseries(qseriesfunc, ui, repo, *dummy, **opts):
    '''run the qseries command with colored output'''
    ui.pushbuffer()
    retval = qseriesfunc(ui, repo, **opts)
    patches = ui.popbuffer().splitlines()
    for patch in patches:
        if opts['missing']:
            effects = _patch_effects['missing']
        # Determine if patch is applied.  Search for beginning of output
        # line in the applied patch list, in case --summary has been used
        # and output line isn't just the patch name.
        elif [ applied for applied in repo.mq.applied
               if patch.startswith(applied.name) ]:
            effects = _patch_effects['applied']
        else:
            effects = _patch_effects['unapplied']
        sys.stdout.write(render_effects(patch, *effects) + '\n')
    return retval

_patch_effects = { 'applied': ('blue', 'bold', 'underline'),
                   'missing': ('red', 'bold'),
                   'unapplied': ('black', 'bold'), }

def uisetup(ui):
    '''Initialize the extension.'''
    nocoloropt = ('', 'no-color', None, _("don't colorize output"))
    _decoratecmd(ui, 'status', commands.table, colorstatus, nocoloropt)
    _configcmdeffects(ui, 'status', _status_effects);
    if ui.config('extensions', 'hgext.mq', default=None) is not None:
        from hgext import mq
        _decoratecmd(ui, 'qseries', mq.cmdtable, colorqseries, nocoloropt)
        _configcmdeffects(ui, 'qseries', _patch_effects);

def _decoratecmd(ui, cmd, table, delegate, *delegateoptions):
    '''Replace the function that implements cmd in table with a decorator.

    The decorator that becomes the new implementation of cmd calls
    delegate.  The delegate's first argument is the replaced function,
    followed by the normal Mercurial command arguments (ui, repo, ...).  If
    the delegate adds command options, supply them as delegateoptions.
    '''
    cmdkey, cmdentry = _cmdtableitem(ui, cmd, table)
    decorator = lambda ui, repo, *args, **opts: \
                    _colordecorator(delegate, cmdentry[0],
                                    ui, repo, *args, **opts)
    # make sure 'hg help cmd' still works
    decorator.__doc__ = cmdentry[0].__doc__
    decoratorentry = (decorator,) + cmdentry[1:]
    for option in delegateoptions:
        decoratorentry[1].append(option)
    table[cmdkey] = decoratorentry

def _cmdtableitem(ui, cmd, table):
    '''Return key, value from table for cmd, or None if not found.'''
    aliases, entry = cmdutil.findcmd(ui, cmd, table)
    for candidatekey, candidateentry in table.iteritems():
        if candidateentry is entry:
            return candidatekey, entry

def _colordecorator(colorfunc, nocolorfunc, ui, repo, *args, **opts):
    '''Delegate to colorfunc or nocolorfunc, depending on conditions.

    Delegate to colorfunc unless --no-color option is set or output is not
    to a tty.
    '''
    if opts['no_color'] or not sys.stdout.isatty():
        return nocolorfunc(ui, repo, *args, **opts)
    return colorfunc(nocolorfunc, ui, repo, *args, **opts)

def _configcmdeffects(ui, cmdname, effectsmap):
    '''Override default effects for cmdname with those from .hgrc file.

    Entries in the .hgrc file are in the [color] section, and look like
    'cmdname'.'status' (for instance, 'status.modified = blue bold inverse').
    '''
    for status in effectsmap:
        effects = ui.config('color', cmdname + '.' + status)
        if effects:
            effectsmap[status] = re.split('\W+', effects)
