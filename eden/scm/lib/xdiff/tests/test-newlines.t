# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

Setup
  $ . "$TESTDIR/setup.sh"


test behaviour around handling newlines

  $ cat << EOF > a
  > #[something]
  > fn one() {
  > }
  > 
  > #[something]
  > fn three() {
  > }
  > EOF

  $ cat << EOF > b
  > #[something]
  > fn one() {
  > }
  > 
  > #[something]
  > fn two() {
  > }
  > 
  > #[something]
  > fn three() {
  > }
  > EOF

  $ xdiff a b
  diff --git a/a b/b
  --- a/a
  +++ b/b
  @@ -2,6 +2,10 @@
   fn one() {
   }
   
  +#[something]
  +fn two() {
  +}
  +
   #[something]
   fn three() {
   }
