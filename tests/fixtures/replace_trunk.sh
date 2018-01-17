#!/bin/sh

RSVN="`pwd`/rsvn.py"
export PATH=/bin:/usr/bin
mkdir temp
cd temp

svnadmin create repo
svn co file://`pwd`/repo wc

cd wc
mkdir trunk branches
cd trunk
for a in alpha beta gamma ; do
    echo $a > $a
done
cd ..
svn add *
svn ci -m 'initial'

svn up
svn cp trunk branches/test
svn ci -m 'branch'

svn up
echo foo >> branches/test/alpha
svn ci -m 'Mod.'

cd ..
echo rdelete trunk > tmp
echo rcopy branches/test trunk >> tmp
python $RSVN --message=blah --username=evil `pwd`/repo < tmp

svnadmin dump repo > ../replace_trunk_with_branch.svndump
