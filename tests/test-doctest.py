# this is hack to make sure no escape characters are inserted into the output
import os
if 'TERM' in os.environ:
    del os.environ['TERM']
import doctest

import mercurial.util
doctest.testmod(mercurial.util)
# Only run doctests for the current platform
doctest.testmod(mercurial.util.platform)

import mercurial.changelog
doctest.testmod(mercurial.changelog)

import mercurial.dagparser
doctest.testmod(mercurial.dagparser, optionflags=doctest.NORMALIZE_WHITESPACE)

import mercurial.match
doctest.testmod(mercurial.match)

import mercurial.store
doctest.testmod(mercurial.store)

import mercurial.ui
doctest.testmod(mercurial.ui)

import mercurial.url
doctest.testmod(mercurial.url)

import mercurial.dispatch
doctest.testmod(mercurial.dispatch)

import mercurial.encoding
doctest.testmod(mercurial.encoding)

import mercurial.hgweb.hgwebdir_mod
doctest.testmod(mercurial.hgweb.hgwebdir_mod)

import hgext.convert.cvsps
doctest.testmod(hgext.convert.cvsps)

import mercurial.revset
doctest.testmod(mercurial.revset)

import mercurial.minirst
doctest.testmod(mercurial.minirst)

import mercurial.templatefilters
doctest.testmod(mercurial.templatefilters)
