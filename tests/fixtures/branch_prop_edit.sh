#!/bin/sh
mkdir temp
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc
cd wc
mkdir branches trunk
svn add *
svn ci -m 'branches trunk'
svn up

cd trunk
for a in alpha beta gamma ; do
    echo $a > $a
    svn add $a
done
svn ci -m 'Files.'
cd ..
svn up

svn cp trunk branches/dev_branch
svn ci -m 'make a branch'
svn up

cd branches/dev_branch
echo epsilon > epsilon
svn add epsilon
svn ci -m 'Add a file on the branch.'
svn up
cd ../..

cd branches/dev_branch
svn ps 'svn:ignore' 'delta' .
svn ci -m 'Commit bogus propchange.'
svn up
cd ../../..

pwd
svnadmin dump repo > ../branch_prop_edit.svndump
cd ..
echo 'Dump created in branch_prop_edit.svndump. You can probably delete temp.'
exit 0
