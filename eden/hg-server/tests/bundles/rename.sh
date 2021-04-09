#!/bin/sh

#  @  3: 'move2'
#  |
#  o  2: 'move1'
#  |
#  | o  1: 'change'
#  |/
#  o  0: 'add'

hg init copies
cd copies
echo a > a
echo b > b
echo c > c
hg ci -Am add
echo a >> a
echo b >> b
echo c >> c
hg ci -m change
hg up -qC 0
hg cp a d
hg mv b e
hg mv c f
hg ci -m move1
hg mv e g
hg mv f c
hg ci -m move2
hg bundle -a ../renames.hg
cd ..
