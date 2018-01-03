#!/bin/sh
#
# Generate rename-closed-branch-dir.svndump
#

mkdir temp
cd temp

mkdir project
cd project
mkdir trunk
mkdir branches
mkdir tags
cd ..

svnadmin create testrepo
CURRENT_DIR=`pwd`
svnurl=file://"$CURRENT_DIR"/testrepo
#svn import project-orig $svnurl -m "init project"

svn co $svnurl project
cd project
svn add *
svn ci -m "init project"

cd trunk
echo a > a.txt
svn add a.txt
svn ci -m "add a.txt in trunk"

# Create a branch
svn up
cd ../branches
svn copy ../trunk async-db
svn ci -m "add branch async-db"
svn up

# Implement feature
cd async-db
echo b > b.txt
svn add b.txt
svn ci -m "Async functionality"

# Merge feature branch
cd ../../trunk
svn merge $svnurl/branches/async-db
svn ci -m "Merged branch async-db"
cd ..
svn up

# Create branch folder for unnecessary branches
svn mkdir $svnurl/branches/dead -m "Create branch folder for unnecessary branches"
svn up

#  We don't need the 'async-db' branch, anymore.
svn copy $svnurl/branches/async-db $svnurl/branches/dead -m "We don't need the 'async-db' branch, anymore."
svn up

# Rename 'dead' folder to 'closed'
svn move $svnurl/branches/dead $svnurl/branches/closed -m "Renamed 'dead' folder to 'closed'"
svn up

# Move 'branches/closed' to 'tags/closed'
svn move $svnurl/branches/closed $svnurl/tags/closed -m "Moved 'branches/closed' to 'tags/closed'."
svn up

# Dump repository
cd ..
svnadmin dump testrepo > ../rename-closed-branch-dir.svndump
