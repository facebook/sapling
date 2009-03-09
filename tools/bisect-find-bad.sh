#!/bin/bash
/bin/rm -rf *
svn export `hg svn info 2> /dev/null | grep '^URL: ' | sed 's/URL: //'` -`hg svn parent | sed 's/.*: //;s/ .*//'` . --force
if [ `hg st | wc -l` = 0 ] ; then
    exit 0
else
    hg revert --all
    hg purge
    exit 1
fi
