#!/usr/bin/env python
#
# checkseclevel - checking section title levels in each online help document

from __future__ import absolute_import

import optparse
import os
import sys

# import from the live mercurial repo
os.environ['HGMODULEPOLICY'] = 'py'
sys.path.insert(0, "..")
from mercurial import demandimport; demandimport.enable()
from mercurial import (
    commands,
    extensions,
    help,
    minirst,
    ui as uimod,
)

table = commands.table
helptable = help.helptable

level2mark = ['"', '=', '-', '.', '#']
reservedmarks = ['"']

mark2level = {}
for m, l in zip(level2mark, xrange(len(level2mark))):
    if m not in reservedmarks:
        mark2level[m] = l

initlevel_topic = 0
initlevel_cmd = 1
initlevel_ext = 1
initlevel_ext_cmd = 3

def showavailables(ui, initlevel):
    ui.warn(('    available marks and order of them in this help: %s\n') %
            (', '.join(['%r' % (m * 4) for m in level2mark[initlevel + 1:]])))

def checkseclevel(ui, doc, name, initlevel):
    ui.note(('checking "%s"\n') % name)
    blocks, pruned = minirst.parse(doc, 0, ['verbose'])
    errorcnt = 0
    curlevel = initlevel
    for block in blocks:
        if block['type'] != 'section':
            continue
        mark = block['underline']
        title = block['lines'][0]
        if (mark not in mark2level) or (mark2level[mark] <= initlevel):
            ui.warn(('invalid section mark %r for "%s" of %s\n') %
                    (mark * 4, title, name))
            showavailables(ui, initlevel)
            errorcnt += 1
            continue
        nextlevel = mark2level[mark]
        if curlevel < nextlevel and curlevel + 1 != nextlevel:
            ui.warn(('gap of section level at "%s" of %s\n') %
                    (title, name))
            showavailables(ui, initlevel)
            errorcnt += 1
            continue
        ui.note(('appropriate section level for "%s %s"\n') %
                (mark * (nextlevel * 2), title))
        curlevel = nextlevel

    return errorcnt

def checkcmdtable(ui, cmdtable, namefmt, initlevel):
    errorcnt = 0
    for k, entry in cmdtable.items():
        name = k.split("|")[0].lstrip("^")
        if not entry[0].__doc__:
            ui.note(('skip checking %s: no help document\n') %
                    (namefmt % name))
            continue
        errorcnt += checkseclevel(ui, entry[0].__doc__,
                                  namefmt % name,
                                  initlevel)
    return errorcnt

def checkhghelps(ui):
    errorcnt = 0
    for names, sec, doc in helptable:
        if callable(doc):
            doc = doc(ui)
        errorcnt += checkseclevel(ui, doc,
                                  '%s help topic' % names[0],
                                  initlevel_topic)

    errorcnt += checkcmdtable(ui, table, '%s command', initlevel_cmd)

    for name in sorted(extensions.enabled().keys() +
                       extensions.disabled().keys()):
        mod = extensions.load(ui, name, None)
        if not mod.__doc__:
            ui.note(('skip checking %s extension: no help document\n') % name)
            continue
        errorcnt += checkseclevel(ui, mod.__doc__,
                                  '%s extension' % name,
                                  initlevel_ext)

        cmdtable = getattr(mod, 'cmdtable', None)
        if cmdtable:
            errorcnt += checkcmdtable(ui, cmdtable,
                                      '%s command of ' + name + ' extension',
                                      initlevel_ext_cmd)
    return errorcnt

def checkfile(ui, filename, initlevel):
    if filename == '-':
        filename = 'stdin'
        doc = sys.stdin.read()
    else:
        with open(filename) as fp:
            doc = fp.read()

    ui.note(('checking input from %s with initlevel %d\n') %
            (filename, initlevel))
    return checkseclevel(ui, doc, 'input from %s' % filename, initlevel)

def main():
    optparser = optparse.OptionParser("""%prog [options]

This checks all help documents of Mercurial (topics, commands,
extensions and commands of them), if no file is specified by --file
option.
""")
    optparser.add_option("-v", "--verbose",
                         help="enable additional output",
                         action="store_true")
    optparser.add_option("-d", "--debug",
                         help="debug mode",
                         action="store_true")
    optparser.add_option("-f", "--file",
                         help="filename to read in (or '-' for stdin)",
                         action="store", default="")

    optparser.add_option("-t", "--topic",
                         help="parse file as help topic",
                         action="store_const", dest="initlevel", const=0)
    optparser.add_option("-c", "--command",
                         help="parse file as help of core command",
                         action="store_const", dest="initlevel", const=1)
    optparser.add_option("-e", "--extension",
                         help="parse file as help of extension",
                         action="store_const", dest="initlevel", const=1)
    optparser.add_option("-C", "--extension-command",
                         help="parse file as help of extension command",
                         action="store_const", dest="initlevel", const=3)

    optparser.add_option("-l", "--initlevel",
                         help="set initial section level manually",
                         action="store", type="int", default=0)

    (options, args) = optparser.parse_args()

    ui = uimod.ui()
    ui.setconfig('ui', 'verbose', options.verbose, '--verbose')
    ui.setconfig('ui', 'debug', options.debug, '--debug')

    if options.file:
        if checkfile(ui, options.file, options.initlevel):
            sys.exit(1)
    else:
        if checkhghelps(ui):
            sys.exit(1)

if __name__ == "__main__":
    main()
