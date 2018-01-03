#!/bin/sh
#
# Generate renames.svndump
#

set -e

rm -rf temp

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
mkdir branches
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import project-orig $svnurl -m "init project"

svn co $svnurl project
cd project/trunk
# Entries for regular tests
echo a > a
echo b > b
ln -s a linka
ln -s b linkb
mkdir -p da/db
echo c > da/daf
ln -s daf da/dalink
echo d > da/db/dbf
ln -s ../daf da/db/dblink
# Entries to test delete + copy
echo deleted > deletedfile
ln -s b deletedlink
mkdir deleteddir
echo deleteddir > deleteddir/f
ln -s f deleteddir/link
# Entries to test copy before change
echo changed > changed
ln -s changed changedlink
mkdir changeddir
echo changed2 > changeddir/f
ln -s f changeddir/link
# Entries unchanged in the rest of history
echo unchanged > unchanged
ln -s unchanged unchangedlink
mkdir unchangeddir
echo unchanged2 > unchangeddir/f
ln -s f unchangeddir/link
# One of the files will be changed afterwards, to test
# group copies detection
mkdir groupdir
echo a > groupdir/a
echo b > groupdir/b
ln -s a groupdir/linka
ln -s b groupdir/linkb
svn add a b linka linkb da deleted* changed* unchanged* groupdir
svn ci -m "add everything"
# Remove files to be copied later
svn rm deletedfile
svn rm deleteddir
svn rm deletedlink
# Update files to be copied before this change
echo changed >> changed
echo changed2 >> changeddir/f
ln -sfn changeddir/f changedlink
ln -sfn ../changed changeddir/link
# Update one of the groupdir files
echo a >> groupdir/a
ln -sfn ../a groupdir/linka
svn ci -m "delete files and dirs"
cd ../branches
svn cp ../trunk branch1
svn ci -m "create branch1"
cd branch1
echo c > c
ln -s c linkc
svn add c linkc
svn ci -m "add c and linkc"
cd ../../trunk
# Regular copy and rename
svn cp a a1
svn cp linka linka1
svn mv a a2
svn mv linka linka2
# Copy and update of source and dest
svn cp b b1
svn cp linkb linkb1
echo b >> b
echo c >> b1
ln -sfn bb linkb
ln -sfn bc linkb1
# Directory copy and renaming
svn cp da da1
svn mv da da2
# Test one copy operation in branch
cd ../branches/branch1
svn cp c c1
svn cp linkc linkc1
echo c >> c1
ln -sfn cc linkc1
cd ../..
svn ci -m "rename and copy a, b, c and da, plus their links"
cd trunk
# Copy across branch
svn cp ../branches/branch1/c c
svn cp ../branches/branch1/linkc linkc
svn ci -m "copy c from branch1"
# Copy deleted stuff from the past
svn cp $svnurl/trunk/deletedfile@2 deletedfile
svn cp $svnurl/trunk/deleteddir@2 deleteddir
svn cp $svnurl/trunk/deletedlink@2 deletedlink
svn ci -m "copy stuff from the past"
# Copy data from the past before it was changed
svn cp $svnurl/trunk/changed@2 changed2
svn cp $svnurl/trunk/changeddir@2 changeddir2
svn cp $svnurl/trunk/changedlink@2 changedlink2
# Harder, copy from the past before change and change it again
# This confused the stupid diff path
svn cp $svnurl/trunk/changed@2 changed3
svn cp $svnurl/trunk/changedlink@2 changedlink3
echo changed3 >> changed3
ln -sfn changed3 changedlink3
svn ci -m "copy stuff from the past before change"
# Copy unchanged stuff from the past. Since no changed occured in these files
# between the source and parent revision, we record them as copy from parent
# instead of source rev.
svn cp $svnurl/trunk/unchanged@2 unchanged2
svn cp $svnurl/trunk/unchangeddir@2 unchangeddir2
svn cp $svnurl/trunk/unchangedlink@2 unchangedlink2
svn ci -m "copy unchanged stuff from the past"
# Copy groupdir, unfortunately one file was changed after r2 so the
# copy should not be recorded at all
svn cp $svnurl/trunk/groupdir@2 groupdir2
svn ci -m "copy groupdir from the past"
cd ../..

svnadmin dump testrepo > ../renames.svndump
