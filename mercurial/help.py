# help.py - help data for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import gettext, _
import sys, os
import extensions, revset, fileset, templatekw, templatefilters
import util

def listexts(header, exts, indent=1):
    '''return a text listing of the given extensions'''
    if not exts:
        return ''
    maxlength = max(len(e) for e in exts)
    result = '\n%s\n\n' % header
    for name, desc in sorted(exts.iteritems()):
        result += '%s%-*s %s\n' % (' ' * indent, maxlength + 2,
                                   ':%s:' % name, desc)
    return result

def extshelp():
    doc = loaddoc('extensions')()
    doc += listexts(_('enabled extensions:'), extensions.enabled())
    doc += listexts(_('disabled extensions:'), extensions.disabled())
    return doc

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
    (['revs', 'revisions'], _('Specifying Single Revisions'),
     loaddoc('revisions')),
    (['mrevs', 'multirevs'], _('Specifying Multiple Revisions'),
     loaddoc('multirevs')),
    (['revset', 'revsets'], _("Specifying Revision Sets"), loaddoc('revsets')),
    (['fileset', 'filesets'], _("Specifying File Sets"), loaddoc('filesets')),
    (['diffs'], _('Diff Formats'), loaddoc('diffs')),
    (['merge-tools'], _('Merge Tools'), loaddoc('merge-tools')),
    (['templating', 'templates'], _('Template Usage'),
     loaddoc('templates')),
    (['urls'], _('URL Paths'), loaddoc('urls')),
    (["extensions"], _("Using additional features"), extshelp),
   (["subrepo", "subrepos"], _("Subrepositories"), loaddoc('subrepos')),
   (["hgweb"], _("Configuring hgweb"), loaddoc('hgweb')),
   (["glossary"], _("Glossary"), loaddoc('glossary')),
   (["hgignore", "ignore"], _("syntax for Mercurial ignore files"),
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
        lines[1:] = [('  ' + l.strip()) for l in lines[1:]]
        entries.append('\n'.join(lines))
    entries = '\n\n'.join(entries)
    return doc.replace(marker, entries)

def addtopicsymbols(topic, marker, symbols):
    def add(topic, doc):
        return makeitemsdoc(topic, doc, marker, symbols)
    addtopichook(topic, add)

addtopicsymbols('filesets', '.. predicatesmarker', fileset.symbols)
addtopicsymbols('revsets', '.. predicatesmarker', revset.symbols)
addtopicsymbols('templates', '.. keywordsmarker', templatekw.keywords)
addtopicsymbols('templates', '.. filtersmarker', templatefilters.filters)
