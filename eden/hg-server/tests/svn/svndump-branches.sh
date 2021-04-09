#!/bin/sh
#
# Use this script to generate branches.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
mkdir branches
cd ..

svnadmin create svn-repo
svnurl=file://`pwd`/svn-repo
svn import project-orig $svnurl -m "init projA"

svn co $svnurl project
cd project
echo a > trunk/a
echo b > trunk/b
echo c > trunk/c
mkdir trunk/dir
echo e > trunk/dir/e
# Add a file within branches, used to confuse branch detection
echo d > branches/notinbranch
svn add trunk/a trunk/b trunk/c trunk/dir branches/notinbranch
svn ci -m hello
svn up

# Branch to old
svn copy trunk branches/old
svn rm branches/old/c
svn rm branches/old/dir
svn ci -m "branch trunk, remove c and dir"
svn up

# Update trunk
echo a >> trunk/a
svn ci -m "change a"

# Update old branch
echo b >> branches/old/b
svn ci -m "change b"

# Create a cross-branch revision
svn move trunk/b branches/old/c
echo c >> branches/old/c
svn ci -m "move and update c"

# Update old branch again
echo b >> branches/old/b
svn ci -m "change b again"

# Move back and forth between branch of similar names
# This used to generate fake copy records
svn up
svn move branches/old branches/old2
svn ci -m "move to old2"
svn move branches/old2 branches/old
svn ci -m "move back to old"

# Update trunk again
echo a > trunk/a
svn ci -m "last change to a"

# Branch again from a converted revision
svn copy -r 1 $svnurl/trunk branches/old3
svn ci -m "branch trunk@1 into old3"
cd ..

svnadmin dump svn-repo > ../branches.svndump
