#!/bin/sh
mkdir temp || exit 1
cd temp
svnadmin create repo
svn co file://`pwd`/repo wc
pushd wc
mkdir -p project/trunk
svn add project
svn ci -m 'trunk'
cd project/trunk
echo a > a
mkdir narf
svn add a narf
svn ci -m 'file and empty dir'
popd
svnadmin dump repo > ../empty_dir_in_trunk_not_repo_root.svndump
echo 'dump in empty_dir_in_trunk_not_repo_root.svndump'
echo 'you can probably delete temp now'
