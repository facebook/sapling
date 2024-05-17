# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_sqlite"
  $ mononoke_testtool drawdag -R repo  << 'EOF' | tee "${TESTTMP}/drawdag.vars.sh"
  > F
  > |
  > E
  > |
  > D H
  > | |
  > C G
  > |/
  > B
  > |
  > A
  > # modify: A a_file original_content
  > # modify: C c_file old_content
  > # modify: E a_file fixed_content
  > # modify: G g_file g_content
  > # modify: H h_file h_content
  > # copy: D c_prime d_content C c_file
  > # copy: E d_prime e_content D c_prime
  > EOF
  A=1a21e175bd7b7537dee83095eeccf66dea393e734eeb35f93bea530c9dc7e528
  B=b1b014e3dca4b51c7006519227137e122b172d31f419e3608b909481a5c60146
  C=74a33dfc64c14f4fc8ca6150b8bc565431403302b99c2a1dc14fc83eeb7a8938
  D=37567336930d36d383a4ed0058077657e4a5b4bea1c6f7cb98ce41aa21eaa13d
  E=f0249abfb89eb2db2b4ffd4ad0334c25d2b6d96183823127f79688852e6194af
  F=b9b331eb2659c4fd90b10d4488edbe335a1382af2db532584bf3f283b558d89b
  G=074c10beddc42f9e0e41f1adea1dbecf76e01dbe015a5b656205b1245ac7b7fc
  H=25d57e3947ea0f29fd5899a4afe4f17460a2ba65d9307f07349ab188068e6d36

  $ source "${TESTTMP}/drawdag.vars.sh"

Check that D and E lack mutable rename information
  $ mononoke_newadmin mutable-renames -R repo check-commit $D
  No mutable renames associated with this commit
  $ mononoke_newadmin mutable-renames -R repo check-commit $E
  No mutable renames associated with this commit

Copy immutable to mutable on D, and check it
  $ mononoke_newadmin mutable-renames -R repo copy-immutable $D
  Creating entry for `c_file` copied to `c_prime`
  $ mononoke_newadmin mutable-renames -R repo check-commit $D
  Commit has mutable renames associated with some paths
  $ mononoke_newadmin mutable-renames -R repo get $D --path c_prime
  Source path `c_file`, source bonsai CS 74a33dfc64c14f4fc8ca6150b8bc565431403302b99c2a1dc14fc83eeb7a8938, source unode Leaf(FileUnodeId(Blake2(48f83239679afb0c0207fc2bf510bc61b5e39db53f9eba1dc795a79c3422085a)))

Confirm that this didn't change E
  $ mononoke_newadmin mutable-renames -R repo check-commit $E
  No mutable renames associated with this commit

Add a mutable change on E, and check that the immutable changes and this change get copied across
  $ mononoke_newadmin mutable-renames -R repo add --src-commit-id $A --src-path a_file --dst-commit-id $E --dst-path a_file
  Creating entry for `c_prime` copied to `d_prime`
  Creating entry for source file `a_file` to destination file `a_file`
  $ mononoke_newadmin mutable-renames -R repo check-commit $E
  Commit has mutable renames associated with some paths
  $ mononoke_newadmin mutable-renames -R repo get $E --path a_file
  Source path `a_file`, source bonsai CS 1a21e175bd7b7537dee83095eeccf66dea393e734eeb35f93bea530c9dc7e528, source unode Leaf(FileUnodeId(Blake2(2c50f7bb2096b424b82e1c7ebfad7d2361dddf3c1d09ce2dba20e2baa3d388f2)))
  $ mononoke_newadmin mutable-renames -R repo get $E --path d_prime
  Source path `c_prime`, source bonsai CS 37567336930d36d383a4ed0058077657e4a5b4bea1c6f7cb98ce41aa21eaa13d, source unode Leaf(FileUnodeId(Blake2(f6fc8af942343b8de8fbea05804113bfc45e34670a84cfa2272fc5ef606598ae)))

Confirm that we can add a rename of a file in G to A
  $ mononoke_newadmin mutable-renames -R repo add --src-commit-id $A --src-path a_file --dst-commit-id $G --dst-path g_file
  Creating entry for source file `a_file` to destination file `g_file`

This must *never* work - if it does, we create a cycle where G's "ancestor" is H, whose ancestor is G...
  $ mononoke_newadmin mutable-renames -R repo add --src-commit-id $H --src-path h_file --dst-commit-id $G --dst-path g_file
  Creating entry for source file `h_file` to destination file `g_file`
  Error: 25d57e3947ea0f29fd5899a4afe4f17460a2ba65d9307f07349ab188068e6d36 is a potential descendant of 074c10beddc42f9e0e41f1adea1dbecf76e01dbe015a5b656205b1245ac7b7fc - rejecting to avoid loops in history
  [1]

This could be made to work, because F is not actually a descendant of G
  $ mononoke_newadmin mutable-renames -R repo add --src-commit-id $F --src-path a_file --dst-commit-id $G --dst-path g_file
  Creating entry for source file `a_file` to destination file `g_file`
  Error: b9b331eb2659c4fd90b10d4488edbe335a1382af2db532584bf3f283b558d89b is a potential descendant of 074c10beddc42f9e0e41f1adea1dbecf76e01dbe015a5b656205b1245ac7b7fc - rejecting to avoid loops in history
  [1]
But, if the above is made to work, this can't be allowed to work, because it creates a loop (following parents and including mutable history goes F E H G F E ...)!
  $ mononoke_newadmin mutable-renames -R repo add --src-commit-id $H --src-path a_file --dst-commit-id $E --dst-path a_file
  Creating entry for source file `a_file` to destination file `a_file`
