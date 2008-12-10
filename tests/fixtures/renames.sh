#!/bin/sh
#
# Generate renames.svndump
#

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
mkdir -p da/db
echo c > da/daf
echo d > da/db/dbf
# Entries to test delete + copy
echo deleted > deletedfile
mkdir deleteddir
echo deleteddir > deleteddir/f
# Entries to test copy before change
echo changed > changed
mkdir changeddir
echo changed2 > changeddir/f
# Entries unchanged in the rest of history
echo unchanged > unchanged
mkdir unchangeddir
echo unchanged2 > unchangeddir/f
# One of the files will be changed afterwards, to test
# group copies detection
mkdir groupdir
echo a > groupdir/a
echo b > groupdir/b
svn add a b da deletedfile deleteddir changed changeddir unchanged unchangeddir groupdir
svn ci -m "add a and b"
# Remove files to be copied later
svn rm deletedfile
svn rm deleteddir
# Update files to be copied before this change
echo changed >> changed
echo changed2 >> changeddir/f
# Update one of the groupdir files
echo a >> groupdir/a
svn ci -m "delete files and dirs"
cd ../branches
svn cp ../trunk branch1
svn ci -m "create branch1"
cd branch1
echo c > c
svn add c
svn ci -m "add c"
cd ../../trunk
# Regular copy and rename
svn cp a a1
svn mv a a2
# Copy and update of source and dest
svn cp b b1
echo b >> b
echo c >> b1
# Directory copy and renaming
svn cp da da1
svn mv da da2
# Test one copy operation in branch
cd ../branches/branch1
svn cp c c1
echo c >> c1
cd ../..
svn ci -m "rename and copy a, b and da"
cd trunk
# Copy across branch
svn cp ../branches/branch1/c c
svn ci -m "copy b from branch1"
# Copy deleted stuff from the past
svn cp $svnurl/trunk/deletedfile@2 deletedfile
svn cp $svnurl/trunk/deleteddir@2 deleteddir
svn ci -m "copy stuff from the past"
# Copy data from the past before it was changed
svn cp $svnurl/trunk/changed@2 changed2
svn cp $svnurl/trunk/changeddir@2 changeddir2
# Harder, copy from the past before change and change it again
# This confused the stupid diff path
svn cp $svnurl/trunk/changed@2 changed3
echo changed3 >> changed3
svn ci -m "copy stuff from the past before change"
# Copy unchanged stuff from the past. Since no changed occured in these files
# between the source and parent revision, we record them as copy from parent
# instead of source rev.
svn cp $svnurl/trunk/unchanged@2 unchanged2
svn cp $svnurl/trunk/unchangeddir@2 unchangeddir2
svn ci -m "copy unchanged stuff from the past"
# Copy groupdir, unfortunately one file was changed after r2 so the
# copy should not be recorded at all
svn cp $svnurl/trunk/groupdir@2 groupdir2
svn ci -m "copy groupdir from the past"
cd ../..

svnadmin dump testrepo > ../renames.svndump
