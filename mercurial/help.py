# help.py - help data for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import itertools
import os
import textwrap

from .i18n import (
    _,
    gettext,
)
from . import (
    cmdutil,
    encoding,
    error,
    extensions,
    filemerge,
    fileset,
    minirst,
    revset,
    templatefilters,
    templatekw,
    templater,
    util,
)
from .hgweb import (
    webcommands,
)

_exclkeywords = [
    "(DEPRECATED)",
    "(EXPERIMENTAL)",
    # i18n: "(DEPRECATED)" is a keyword, must be translated consistently
    _("(DEPRECATED)"),
    # i18n: "(EXPERIMENTAL)" is a keyword, must be translated consistently
    _("(EXPERIMENTAL)"),
    ]

def listexts(header, exts, indent=1, showdeprecated=False):
    '''return a text listing of the given extensions'''
    rst = []
    if exts:
        for name, desc in sorted(exts.iteritems()):
            if not showdeprecated and any(w in desc for w in _exclkeywords):
                continue
            rst.append('%s:%s: %s\n' % (' ' * indent, name, desc))
    if rst:
        rst.insert(0, '\n%s\n\n' % header)
    return rst

def extshelp(ui):
    rst = loaddoc('extensions')(ui).splitlines(True)
    rst.extend(listexts(
        _('enabled extensions:'), extensions.enabled(), showdeprecated=True))
    rst.extend(listexts(_('disabled extensions:'), extensions.disabled()))
    doc = ''.join(rst)
    return doc

def optrst(header, options, verbose):
    data = []
    multioccur = False
    for option in options:
        if len(option) == 5:
            shortopt, longopt, default, desc, optlabel = option
        else:
            shortopt, longopt, default, desc = option
            optlabel = _("VALUE") # default label

        if not verbose and any(w in desc for w in _exclkeywords):
            continue

        so = ''
        if shortopt:
            so = '-' + shortopt
        lo = '--' + longopt
        if default:
            desc += _(" (default: %s)") % default

        if isinstance(default, list):
            lo += " %s [+]" % optlabel
            multioccur = True
        elif (default is not None) and not isinstance(default, bool):
            lo += " %s" % optlabel

        data.append((so, lo, desc))

    if multioccur:
        header += (_(" ([+] can be repeated)"))

    rst = ['\n%s:\n\n' % header]
    rst.extend(minirst.maketable(data, 1))

    return ''.join(rst)

def indicateomitted(rst, omitted, notomitted=None):
    rst.append('\n\n.. container:: omitted\n\n    %s\n\n' % omitted)
    if notomitted:
        rst.append('\n\n.. container:: notomitted\n\n    %s\n\n' % notomitted)

def filtercmd(ui, cmd, kw, doc):
    if not ui.debugflag and cmd.startswith("debug") and kw != "debug":
        return True
    if not ui.verbose and doc and any(w in doc for w in _exclkeywords):
        return True
    return False

def topicmatch(ui, kw):
    """Return help topics matching kw.

    Returns {'section': [(name, summary), ...], ...} where section is
    one of topics, commands, extensions, or extensioncommands.
    """
    kw = encoding.lower(kw)
    def lowercontains(container):
        return kw in encoding.lower(container)  # translated in helptable
    results = {'topics': [],
               'commands': [],
               'extensions': [],
               'extensioncommands': [],
               }
    for names, header, doc in helptable:
        # Old extensions may use a str as doc.
        if (sum(map(lowercontains, names))
            or lowercontains(header)
            or (callable(doc) and lowercontains(doc(ui)))):
            results['topics'].append((names[0], header))
    from . import commands # avoid cycle
    for cmd, entry in commands.table.iteritems():
        if len(entry) == 3:
            summary = entry[2]
        else:
            summary = ''
        # translate docs *before* searching there
        docs = _(getattr(entry[0], '__doc__', None)) or ''
        if kw in cmd or lowercontains(summary) or lowercontains(docs):
            doclines = docs.splitlines()
            if doclines:
                summary = doclines[0]
            cmdname = cmd.partition('|')[0].lstrip('^')
            if filtercmd(ui, cmdname, kw, docs):
                continue
            results['commands'].append((cmdname, summary))
    for name, docs in itertools.chain(
        extensions.enabled(False).iteritems(),
        extensions.disabled().iteritems()):
        if not docs:
            continue
        mod = extensions.load(ui, name, '')
        name = name.rpartition('.')[-1]
        if lowercontains(name) or lowercontains(docs):
            # extension docs are already translated
            results['extensions'].append((name, docs.splitlines()[0]))
        for cmd, entry in getattr(mod, 'cmdtable', {}).iteritems():
            if kw in cmd or (len(entry) > 2 and lowercontains(entry[2])):
                cmdname = cmd.partition('|')[0].lstrip('^')
                if entry[0].__doc__:
                    cmddoc = gettext(entry[0].__doc__).splitlines()[0]
                else:
                    cmddoc = _('(no help text available)')
                if filtercmd(ui, cmdname, kw, cmddoc):
                    continue
                results['extensioncommands'].append((cmdname, cmddoc))
    return results

def loaddoc(topic, subdir=None):
    """Return a delayed loader for help/topic.txt."""

    def loader(ui):
        docdir = os.path.join(util.datapath, 'help')
        if subdir:
            docdir = os.path.join(docdir, subdir)
        path = os.path.join(docdir, topic + ".txt")
        doc = gettext(util.readfile(path))
        for rewriter in helphooks.get(topic, []):
            doc = rewriter(ui, topic, doc)
        return doc

    return loader

internalstable = sorted([
    (['bundles'], _('container for exchange of repository data'),
     loaddoc('bundles', subdir='internals')),
    (['changegroups'], _('representation of revlog data'),
     loaddoc('changegroups', subdir='internals')),
    (['requirements'], _('repository requirements'),
     loaddoc('requirements', subdir='internals')),
    (['revlogs'], _('revision storage mechanism'),
     loaddoc('revlogs', subdir='internals')),
])

def internalshelp(ui):
    """Generate the index for the "internals" topic."""
    lines = []
    for names, header, doc in internalstable:
        lines.append(' :%s: %s\n' % (names[0], header))

    return ''.join(lines)

helptable = sorted([
    (["config", "hgrc"], _("Configuration Files"), loaddoc('config')),
    (["dates"], _("Date Formats"), loaddoc('dates')),
    (["patterns"], _("File Name Patterns"), loaddoc('patterns')),
    (['environment', 'env'], _('Environment Variables'),
     loaddoc('environment')),
    (['revisions', 'revs'], _('Specifying Single Revisions'),
     loaddoc('revisions')),
    (['multirevs', 'mrevs'], _('Specifying Multiple Revisions'),
     loaddoc('multirevs')),
    (['revsets', 'revset'], _("Specifying Revision Sets"), loaddoc('revsets')),
    (['filesets', 'fileset'], _("Specifying File Sets"), loaddoc('filesets')),
    (['diffs'], _('Diff Formats'), loaddoc('diffs')),
    (['merge-tools', 'mergetools'], _('Merge Tools'), loaddoc('merge-tools')),
    (['templating', 'templates', 'template', 'style'], _('Template Usage'),
     loaddoc('templates')),
    (['urls'], _('URL Paths'), loaddoc('urls')),
    (["extensions"], _("Using Additional Features"), extshelp),
    (["subrepos", "subrepo"], _("Subrepositories"), loaddoc('subrepos')),
    (["hgweb"], _("Configuring hgweb"), loaddoc('hgweb')),
    (["glossary"], _("Glossary"), loaddoc('glossary')),
    (["hgignore", "ignore"], _("Syntax for Mercurial Ignore Files"),
     loaddoc('hgignore')),
    (["phases"], _("Working with Phases"), loaddoc('phases')),
    (['scripting'], _('Using Mercurial from scripts and automation'),
     loaddoc('scripting')),
    (['internals'], _("Technical implementation topics"),
     internalshelp),
])

# Maps topics with sub-topics to a list of their sub-topics.
subtopics = {
    'internals': internalstable,
}

# Map topics to lists of callable taking the current topic help and
# returning the updated version
helphooks = {}

def addtopichook(topic, rewriter):
    helphooks.setdefault(topic, []).append(rewriter)

def makeitemsdoc(ui, topic, doc, marker, items, dedent=False):
    """Extract docstring from the items key to function mapping, build a
    single documentation block and use it to overwrite the marker in doc.
    """
    entries = []
    for name in sorted(items):
        text = (items[name].__doc__ or '').rstrip()
        if (not text
            or not ui.verbose and any(w in text for w in _exclkeywords)):
            continue
        text = gettext(text)
        if dedent:
            text = textwrap.dedent(text)
        lines = text.splitlines()
        doclines = [(lines[0])]
        for l in lines[1:]:
            # Stop once we find some Python doctest
            if l.strip().startswith('>>>'):
                break
            if dedent:
                doclines.append(l.rstrip())
            else:
                doclines.append('  ' + l.strip())
        entries.append('\n'.join(doclines))
    entries = '\n\n'.join(entries)
    return doc.replace(marker, entries)

def addtopicsymbols(topic, marker, symbols, dedent=False):
    def add(ui, topic, doc):
        return makeitemsdoc(ui, topic, doc, marker, symbols, dedent=dedent)
    addtopichook(topic, add)

addtopicsymbols('filesets', '.. predicatesmarker', fileset.symbols)
addtopicsymbols('merge-tools', '.. internaltoolsmarker',
                filemerge.internalsdoc)
addtopicsymbols('revsets', '.. predicatesmarker', revset.symbols)
addtopicsymbols('templates', '.. keywordsmarker', templatekw.keywords)
addtopicsymbols('templates', '.. filtersmarker', templatefilters.filters)
addtopicsymbols('templates', '.. functionsmarker', templater.funcs)
addtopicsymbols('hgweb', '.. webcommandsmarker', webcommands.commands,
                dedent=True)

def help_(ui, name, unknowncmd=False, full=True, subtopic=None, **opts):
    '''
    Generate the help for 'name' as unformatted restructured text. If
    'name' is None, describe the commands available.
    '''

    from . import commands # avoid cycle

    def helpcmd(name, subtopic=None):
        try:
            aliases, entry = cmdutil.findcmd(name, commands.table,
                                             strict=unknowncmd)
        except error.AmbiguousCommand as inst:
            # py3k fix: except vars can't be used outside the scope of the
            # except block, nor can be used inside a lambda. python issue4617
            prefix = inst.args[0]
            select = lambda c: c.lstrip('^').startswith(prefix)
            rst = helplist(select)
            return rst

        rst = []

        # check if it's an invalid alias and display its error if it is
        if getattr(entry[0], 'badalias', None):
            rst.append(entry[0].badalias + '\n')
            if entry[0].unknowncmd:
                try:
                    rst.extend(helpextcmd(entry[0].cmdname))
                except error.UnknownCommand:
                    pass
            return rst

        # synopsis
        if len(entry) > 2:
            if entry[2].startswith('hg'):
                rst.append("%s\n" % entry[2])
            else:
                rst.append('hg %s %s\n' % (aliases[0], entry[2]))
        else:
            rst.append('hg %s\n' % aliases[0])
        # aliases
        if full and not ui.quiet and len(aliases) > 1:
            rst.append(_("\naliases: %s\n") % ', '.join(aliases[1:]))
        rst.append('\n')

        # description
        doc = gettext(entry[0].__doc__)
        if not doc:
            doc = _("(no help text available)")
        if util.safehasattr(entry[0], 'definition'):  # aliased command
            source = entry[0].source
            if entry[0].definition.startswith('!'):  # shell alias
                doc = (_('shell alias for::\n\n    %s\n\ndefined by: %s\n') %
                       (entry[0].definition[1:], source))
            else:
                doc = (_('alias for: hg %s\n\n%s\n\ndefined by: %s\n') %
                       (entry[0].definition, doc, source))
        doc = doc.splitlines(True)
        if ui.quiet or not full:
            rst.append(doc[0])
        else:
            rst.extend(doc)
        rst.append('\n')

        # check if this command shadows a non-trivial (multi-line)
        # extension help text
        try:
            mod = extensions.find(name)
            doc = gettext(mod.__doc__) or ''
            if '\n' in doc.strip():
                msg = _('(use "hg help -e %s" to show help for '
                        'the %s extension)') % (name, name)
                rst.append('\n%s\n' % msg)
        except KeyError:
            pass

        # options
        if not ui.quiet and entry[1]:
            rst.append(optrst(_("options"), entry[1], ui.verbose))

        if ui.verbose:
            rst.append(optrst(_("global options"),
                              commands.globalopts, ui.verbose))

        if not ui.verbose:
            if not full:
                rst.append(_('\n(use "hg %s -h" to show more help)\n')
                           % name)
            elif not ui.quiet:
                rst.append(_('\n(some details hidden, use --verbose '
                               'to show complete help)'))

        return rst


    def helplist(select=None, **opts):
        # list of commands
        if name == "shortlist":
            header = _('basic commands:\n\n')
        elif name == "debug":
            header = _('debug commands (internal and unsupported):\n\n')
        else:
            header = _('list of commands:\n\n')

        h = {}
        cmds = {}
        for c, e in commands.table.iteritems():
            f = c.partition("|")[0]
            if select and not select(f):
                continue
            if (not select and name != 'shortlist' and
                e[0].__module__ != commands.__name__):
                continue
            if name == "shortlist" and not f.startswith("^"):
                continue
            f = f.lstrip("^")
            doc = e[0].__doc__
            if filtercmd(ui, f, name, doc):
                continue
            doc = gettext(doc)
            if not doc:
                doc = _("(no help text available)")
            h[f] = doc.splitlines()[0].rstrip()
            cmds[f] = c.lstrip("^")

        rst = []
        if not h:
            if not ui.quiet:
                rst.append(_('no commands defined\n'))
            return rst

        if not ui.quiet:
            rst.append(header)
        fns = sorted(h)
        for f in fns:
            if ui.verbose:
                commacmds = cmds[f].replace("|",", ")
                rst.append(" :%s: %s\n" % (commacmds, h[f]))
            else:
                rst.append(' :%s: %s\n' % (f, h[f]))

        ex = opts.get
        anyopts = (ex('keyword') or not (ex('command') or ex('extension')))
        if not name and anyopts:
            exts = listexts(_('enabled extensions:'), extensions.enabled())
            if exts:
                rst.append('\n')
                rst.extend(exts)

            rst.append(_("\nadditional help topics:\n\n"))
            topics = []
            for names, header, doc in helptable:
                topics.append((names[0], header))
            for t, desc in topics:
                rst.append(" :%s: %s\n" % (t, desc))

        if ui.quiet:
            pass
        elif ui.verbose:
            rst.append('\n%s\n' % optrst(_("global options"),
                                         commands.globalopts, ui.verbose))
            if name == 'shortlist':
                rst.append(_('\n(use "hg help" for the full list '
                             'of commands)\n'))
        else:
            if name == 'shortlist':
                rst.append(_('\n(use "hg help" for the full list of commands '
                             'or "hg -v" for details)\n'))
            elif name and not full:
                rst.append(_('\n(use "hg help %s" to show the full help '
                             'text)\n') % name)
            elif name and cmds and name in cmds.keys():
                rst.append(_('\n(use "hg help -v -e %s" to show built-in '
                             'aliases and global options)\n') % name)
            else:
                rst.append(_('\n(use "hg help -v%s" to show built-in aliases '
                             'and global options)\n')
                           % (name and " " + name or ""))
        return rst

    def helptopic(name, subtopic=None):
        # Look for sub-topic entry first.
        header, doc = None, None
        if subtopic and name in subtopics:
            for names, header, doc in subtopics[name]:
                if subtopic in names:
                    break

        if not header:
            for names, header, doc in helptable:
                if name in names:
                    break
            else:
                raise error.UnknownCommand(name)

        rst = [minirst.section(header)]

        # description
        if not doc:
            rst.append("    %s\n" % _("(no help text available)"))
        if callable(doc):
            rst += ["    %s\n" % l for l in doc(ui).splitlines()]

        if not ui.verbose:
            omitted = _('(some details hidden, use --verbose'
                         ' to show complete help)')
            indicateomitted(rst, omitted)

        try:
            cmdutil.findcmd(name, commands.table)
            rst.append(_('\nuse "hg help -c %s" to see help for '
                       'the %s command\n') % (name, name))
        except error.UnknownCommand:
            pass
        return rst

    def helpext(name, subtopic=None):
        try:
            mod = extensions.find(name)
            doc = gettext(mod.__doc__) or _('no help text available')
        except KeyError:
            mod = None
            doc = extensions.disabledext(name)
            if not doc:
                raise error.UnknownCommand(name)

        if '\n' not in doc:
            head, tail = doc, ""
        else:
            head, tail = doc.split('\n', 1)
        rst = [_('%s extension - %s\n\n') % (name.rpartition('.')[-1], head)]
        if tail:
            rst.extend(tail.splitlines(True))
            rst.append('\n')

        if not ui.verbose:
            omitted = _('(some details hidden, use --verbose'
                         ' to show complete help)')
            indicateomitted(rst, omitted)

        if mod:
            try:
                ct = mod.cmdtable
            except AttributeError:
                ct = {}
            modcmds = set([c.partition('|')[0] for c in ct])
            rst.extend(helplist(modcmds.__contains__))
        else:
            rst.append(_('(use "hg help extensions" for information on enabling'
                       ' extensions)\n'))
        return rst

    def helpextcmd(name, subtopic=None):
        cmd, ext, mod = extensions.disabledcmd(ui, name,
                                               ui.configbool('ui', 'strict'))
        doc = gettext(mod.__doc__).splitlines()[0]

        rst = listexts(_("'%s' is provided by the following "
                              "extension:") % cmd, {ext: doc}, indent=4,
                       showdeprecated=True)
        rst.append('\n')
        rst.append(_('(use "hg help extensions" for information on enabling '
                   'extensions)\n'))
        return rst


    rst = []
    kw = opts.get('keyword')
    if kw or name is None and any(opts[o] for o in opts):
        matches = topicmatch(ui, name or '')
        helpareas = []
        if opts.get('extension'):
            helpareas += [('extensions', _('Extensions'))]
        if opts.get('command'):
            helpareas += [('commands', _('Commands'))]
        if not helpareas:
            helpareas = [('topics', _('Topics')),
                         ('commands', _('Commands')),
                         ('extensions', _('Extensions')),
                         ('extensioncommands', _('Extension Commands'))]
        for t, title in helpareas:
            if matches[t]:
                rst.append('%s:\n\n' % title)
                rst.extend(minirst.maketable(sorted(matches[t]), 1))
                rst.append('\n')
        if not rst:
            msg = _('no matches')
            hint = _('try "hg help" for a list of topics')
            raise error.Abort(msg, hint=hint)
    elif name and name != 'shortlist':
        queries = []
        if unknowncmd:
            queries += [helpextcmd]
        if opts.get('extension'):
            queries += [helpext]
        if opts.get('command'):
            queries += [helpcmd]
        if not queries:
            queries = (helptopic, helpcmd, helpext, helpextcmd)
        for f in queries:
            try:
                rst = f(name, subtopic)
                break
            except error.UnknownCommand:
                pass
        else:
            if unknowncmd:
                raise error.UnknownCommand(name)
            else:
                msg = _('no such help topic: %s') % name
                hint = _('try "hg help --keyword %s"') % name
                raise error.Abort(msg, hint=hint)
    else:
        # program name
        if not ui.quiet:
            rst = [_("Mercurial Distributed SCM\n"), '\n']
        rst.extend(helplist(None, **opts))

    return ''.join(rst)
