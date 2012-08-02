# help.py - help data for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import gettext, _
import itertools, sys, os
import extensions, revset, fileset, templatekw, templatefilters, filemerge
import encoding, util, minirst

def listexts(header, exts, indent=1):
    '''return a text listing of the given extensions'''
    rst = []
    if exts:
        rst.append('\n%s\n\n' % header)
        for name, desc in sorted(exts.iteritems()):
            rst.append('%s:%s: %s\n' % (' ' * indent, name, desc))
    return rst

def extshelp():
    rst = loaddoc('extensions')().splitlines(True)
    rst.extend(listexts(_('enabled extensions:'), extensions.enabled()))
    rst.extend(listexts(_('disabled extensions:'), extensions.disabled()))
    doc = ''.join(rst)
    return doc

def optrst(options, verbose):
    data = []
    multioccur = False
    for option in options:
        if len(option) == 5:
            shortopt, longopt, default, desc, optlabel = option
        else:
            shortopt, longopt, default, desc = option
            optlabel = _("VALUE") # default label

        if _("DEPRECATED") in desc and not verbose:
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

    rst = minirst.maketable(data, 1)

    if multioccur:
        rst.append(_("\n[+] marked option can be specified multiple times\n"))

    return ''.join(rst)

def topicmatch(kw):
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
        if (sum(map(lowercontains, names))
            or lowercontains(header)
            or lowercontains(doc())):
            results['topics'].append((names[0], header))
    import commands # avoid cycle
    for cmd, entry in commands.table.iteritems():
        if cmd.startswith('debug'):
            continue
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
            cmdname = cmd.split('|')[0].lstrip('^')
            results['commands'].append((cmdname, summary))
    for name, docs in itertools.chain(
        extensions.enabled().iteritems(),
        extensions.disabled().iteritems()):
        # extensions.load ignores the UI argument
        mod = extensions.load(None, name, '')
        if lowercontains(name) or lowercontains(docs):
            # extension docs are already translated
            results['extensions'].append((name, docs.splitlines()[0]))
        for cmd, entry in getattr(mod, 'cmdtable', {}).iteritems():
            if kw in cmd or (len(entry) > 2 and lowercontains(entry[2])):
                cmdname = cmd.split('|')[0].lstrip('^')
                if entry[0].__doc__:
                    cmddoc = gettext(entry[0].__doc__).splitlines()[0]
                else:
                    cmddoc = _('(no help text available)')
                results['extensioncommands'].append((cmdname, cmddoc))
    return results

def loaddoc(topic):
    """Return a delayed loader for help/topic.txt."""

    def loader():
        if util.mainfrozen():
            module = sys.executable
        else:
            module = __file__
        base = os.path.dirname(module)

        for dir in ('.', '..'):
            docdir = os.path.join(base, dir, 'help')
            if os.path.isdir(docdir):
                break

        path = os.path.join(docdir, topic + ".txt")
        doc = gettext(util.readfile(path))
        for rewriter in helphooks.get(topic, []):
            doc = rewriter(topic, doc)
        return doc

    return loader

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
])

# Map topics to lists of callable taking the current topic help and
# returning the updated version
helphooks = {}

def addtopichook(topic, rewriter):
    helphooks.setdefault(topic, []).append(rewriter)

def makeitemsdoc(topic, doc, marker, items):
    """Extract docstring from the items key to function mapping, build a
    .single documentation block and use it to overwrite the marker in doc
    """
    entries = []
    for name in sorted(items):
        text = (items[name].__doc__ or '').rstrip()
        if not text:
            continue
        text = gettext(text)
        lines = text.splitlines()
        doclines = [(lines[0])]
        for l in lines[1:]:
            # Stop once we find some Python doctest
            if l.strip().startswith('>>>'):
                break
            doclines.append('  ' + l.strip())
        entries.append('\n'.join(doclines))
    entries = '\n\n'.join(entries)
    return doc.replace(marker, entries)

def addtopicsymbols(topic, marker, symbols):
    def add(topic, doc):
        return makeitemsdoc(topic, doc, marker, symbols)
    addtopichook(topic, add)

addtopicsymbols('filesets', '.. predicatesmarker', fileset.symbols)
addtopicsymbols('merge-tools', '.. internaltoolsmarker', filemerge.internals)
addtopicsymbols('revsets', '.. predicatesmarker', revset.symbols)
addtopicsymbols('templates', '.. keywordsmarker', templatekw.dockeywords)
addtopicsymbols('templates', '.. filtersmarker', templatefilters.filters)
