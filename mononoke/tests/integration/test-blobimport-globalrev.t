# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hg init repo-hg --config format.usefncache=False

  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > commitextras=
  > treemanifest=
  > [treemanifest]
  > server=True
  > EOF

  $ touch file1
  $ hg add
  adding file1
  $ hg commit -m "adding extra globalrev" --extra global_rev=9999999991

  $ touch file2
  $ hg add
  adding file2
  $ hg commit -m "adding extra convert_revision" --extra convert_revision=svn:uuid/path@9999999992

  $ touch file3
  $ hg add
  adding file3
  $ hg commit -m "no extras, expect to skip writing to bonsai_globalrev_mapping table" --extra convert_revision=svn:uuid/path@9999999993

  $ touch file4
  $ hg add
  adding file4
  $ hg commit -m "adding both extra global_rev and convert_revision" --extra global_rev=9999999994 --extra convert_revision=svn:uuid/path@9999999995

  $ touch file5
  $ hg add
  adding file5
  $ hg commit -m "both extras, but global_rev lower than globalrev of commit introducting globalrev" --extra global_rev=1000147969 --extra convert_revision=svn:uuid/path@9999999996


  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo --has-globalrev
  $ get_bonsai_globalrev_mapping
  ADB923EB43189CF56394F61995A9E1FA5CD003CED7167B6FDDB03E94229DB10F|9999999991
  BEA29B6994B07C79FD1B641BC5EFDDFC8955C43E673BE83242D0A1C55D026AC2|9999999992
  BE6299EC5215E9F0905DCDC1AD65E883650979A61C927966D47981CEF4202A29|9999999993
  2A7C72680EAB0AD955B3DA5B54BC7488291A28A4BB56EA0315F39B9C2357D211|9999999994
  436936A88197ECF372BA07A4A304FD78C37C40C19F9A4846EAD6189602BF2FD2|9999999996
