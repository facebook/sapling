#!/usr/bin/env python
#
# checkseclevel - checking section title levels in each online help documents

import sys, os
import optparse

# import from the live mercurial repo
sys.path.insert(0, "..")
# fall back to pure modules if required C extensions are not available
sys.path.append(os.path.join('..', 'mercurial', 'pure'))
from mercurial import demandimport; demandimport.enable()
from mercurial.commands import table
from mercurial.help import helptable
from mercurial import extensions
from mercurial import minirst
from mercurial import util

_verbose = False

def verbose(msg):
    if _verbose:
        print msg

def error(msg):
    sys.stderr.write('%s\n' % msg)

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

def showavailables(initlevel):
    error('    available marks and order of them in this help: %s' %
          (', '.join(['%r' % (m * 4) for m in level2mark[initlevel + 1:]])))

def checkseclevel(doc, name, initlevel):
    verbose('checking "%s"' % name)
    blocks, pruned = minirst.parse(doc, 0, ['verbose'])
    errorcnt = 0
    curlevel = initlevel
    for block in blocks:
        if block['type'] != 'section':
            continue
        mark = block['underline']
        title = block['lines'][0]
        if (mark not in mark2level) or (mark2level[mark] <= initlevel):
            error('invalid section mark %r for "%s" of %s' %
                  (mark * 4, title, name))
            showavailables(initlevel)
            errorcnt += 1
            continue
        nextlevel = mark2level[mark]
        if curlevel < nextlevel and curlevel + 1 != nextlevel:
            error('gap of section level at "%s" of %s' %
                  (title, name))
            showavailables(initlevel)
            errorcnt += 1
            continue
        verbose('appropriate section level for "%s %s"' %
                (mark * (nextlevel * 2), title))
        curlevel = nextlevel

    return errorcnt

def checkcmdtable(cmdtable, namefmt, initlevel):
    errorcnt = 0
    for k, entry in cmdtable.items():
        name = k.split("|")[0].lstrip("^")
        if not entry[0].__doc__:
            verbose('skip checking %s: no help document' %
                    (namefmt % name))
            continue
        errorcnt += checkseclevel(entry[0].__doc__,
                                  namefmt % name,
                                  initlevel)
    return errorcnt

def checkhghelps():
    errorcnt = 0
    for names, sec, doc in helptable:
        if util.safehasattr(doc, '__call__'):
            doc = doc()
        errorcnt += checkseclevel(doc,
                                  '%s help topic' % names[0],
                                  initlevel_topic)

    errorcnt += checkcmdtable(table, '%s command', initlevel_cmd)

    for name in sorted(extensions.enabled().keys() +
                       extensions.disabled().keys()):
        mod = extensions.load(None, name, None)
        if not mod.__doc__:
            verbose('skip checking %s extension: no help document' % name)
            continue
        errorcnt += checkseclevel(mod.__doc__,
                                  '%s extension' % name,
                                  initlevel_ext)

        cmdtable = getattr(mod, 'cmdtable', None)
        if cmdtable:
            errorcnt += checkcmdtable(cmdtable,
                                      '%s command of ' + name + ' extension',
                                      initlevel_ext_cmd)
    return errorcnt

def checkfile(filename, initlevel):
    if filename == '-':
        filename = 'stdin'
        doc = sys.stdin.read()
    else:
        fp = open(filename)
        try:
            doc = fp.read()
        finally:
            fp.close()

    verbose('checking input from %s with initlevel %d' %
            (filename, initlevel))
    return checkseclevel(doc, 'input from %s' % filename, initlevel)

if __name__ == "__main__":
    optparser = optparse.OptionParser("""%prog [options]

This checks all help documents of Mercurial (topics, commands,
extensions and commands of them), if no file is specified by --file
option.
""")
    optparser.add_option("-v", "--verbose",
                         help="enable additional output",
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

    _verbose = options.verbose

    if options.file:
        if checkfile(options.file, options.initlevel):
            sys.exit(1)
    else:
        if checkhghelps():
            sys.exit(1)
