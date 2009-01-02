#!/bin/sh
#
# Generate externals.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
mkdir branches
mkdir externals
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import project-orig $svnurl -m "init project"

svn co $svnurl project
cd project/externals
mkdir project1
echo a > project1/a
svn add project1
mkdir project2
echo a > project2/b
svn add project2
svn ci -m "configure externals projects"
cd ../trunk
# Add an external reference
echo a > a
svn add a
cat > externals <<EOF
../externals/project1 deps/project1
EOF
svn propset -F externals svn:externals .
svn ci -m "set externals on ."
# Add another one
cat > externals <<EOF
../externals/project1 deps/project1
../externals/project2 deps/project2
EOF
svn propset -F externals svn:externals .
svn ci -m "update externals on ."
# Suppress an external and add one on a subdir
cat > externals <<EOF
../externals/project2 deps/project2
EOF
svn propset -F externals svn:externals .
mkdir subdir
mkdir subdir2
svn add subdir subdir2
cat > externals <<EOF
../externals/project1 deps/project1
EOF
svn propset -F externals svn:externals subdir subdir2
svn ci -m "add on subdir"
# Suppress the subdirectory
svn rm subdir
svn ci -m 'remove externals subdir'
# Remove the property on subdir2
svn propdel svn:externals subdir2
svn ci -m 'remove externals subdir2'
cd ../..

svnadmin dump testrepo > ../externals.svndump
