import doctest

import mercurial.changelog
# test doctest from changelog

doctest.testmod(mercurial.changelog)

import mercurial.httprepo
doctest.testmod(mercurial.httprepo)

import mercurial.util
doctest.testmod(mercurial.util)
