#!/bin/sh

mkdir temp
cd temp

svnadmin create repo
svn co file://`pwd`/repo wc
cd wc

mkdir -p trunk branches
svn add trunk branches
svn ci -m'initial structure'
cd trunk
echo a>a
svn add a
svn ci -mci1 a
cd ..
svn up
svn cp trunk branches/foo
svn ci -m'branch foo'
cd branches/foo
ln -s a fnord
svn add fnord
svn ci -msymlink fnord
svn up ../..
echo foo > exe
chmod +x exe
svn add exe
svn ci -mexecutable exe
svn up ../..
cd ../../trunk
svn merge ../branches/foo
svn ci -mmerge
svn up

pwd
cd ../../..
svnadmin dump temp/repo > addspecial.svndump
echo
echo 'Complete.'
echo 'You probably want to clean up temp now.'
echo 'Dump in addspecial.svndump'
exit 0
