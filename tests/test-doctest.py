# this is hack to make sure no escape characters are inserted into the output
import os
if 'TERM' in os.environ:
    del os.environ['TERM']
import doctest

import mercurial.changelog
doctest.testmod(mercurial.changelog)

import mercurial.dagparser
doctest.testmod(mercurial.dagparser, optionflags=doctest.NORMALIZE_WHITESPACE)

import mercurial.match
doctest.testmod(mercurial.match)

import mercurial.url
doctest.testmod(mercurial.url)

import mercurial.util
doctest.testmod(mercurial.util)

import mercurial.encoding
doctest.testmod(mercurial.encoding)

import mercurial.hgweb.hgwebdir_mod
doctest.testmod(mercurial.hgweb.hgwebdir_mod)

import hgext.convert.cvsps
doctest.testmod(hgext.convert.cvsps)
