Manifest
--------

*hg manifest*

The **manifest** is the file that describes the contents of the repository at a particular changeset ID. It primarily contains a list of file names and revisions of those files that are present. The manifest ID identifies the version of the manifest that goes with a particular changeset. The manifest ID is a nodeid. Multiple changesets may refer to the same manifest revision.

A manifest describes the state of a project by listing each file and its nodeid to specify which version.  Recreating a particular state means simply looking up its manifest and reconstructing the listed file versions from their revlogs.  The manifest is conceptually a file.  All of its versions, which collectively represent the entire project history, are stored in a revlog (see the file ``.hg/store/00manifest.d``) and an associated index (``.hg/store/00manifest.i``).

A manifest looks something like this:

::

   $ hg debugdata --manifest 10
   .hgignore 708b3e37a5d2b260e303a6d8c0386ca0c18bf41d
   MANIFEST.in ecf027f3b56bf7fadf8e3dd75cfcd67d8b58deb1
   PKG-INFO 9b3ed8f2b81095a13064402e930565f083346e9a
   README d6f7553376154241dea40dc2af96a8b357f0e991
   hg 06763db6de79098e8cdf14726ca506fdf16749be
   mercurial/__init__.py b80de5d138758541c5f05265ad144ab9fa86d1db
   mercurial/byterange.py 17f5a9fbd99622f31a392c33ac1e903925dc80ed
   mercurial/fancyopts.py b6f52e23e356748c5039313d8b639cda16bf67ba
   mercurial/hg.py 1890959ebcf6b06e3e932e5e47bb35eeb30ea2ed
   mercurial/mdiff.py a05f65c44bfbeec6a42336cd2ff0b30217899ca3
   mercurial/revlog.py edd628e83c50e15004dce0207aaab63f8ceb8c97
   mercurial/transaction.py 9d180df101dc14ce3dd582fd998b36c98b3e39aa
   notes.txt 703afcec5edb749cf5cec67831f554d6da13f2fb
   setup.py ccf3f6daf0f13101ca73631f7a1769e328b472c9
   tkmerge 3c922edb43a9c143682f7bc7b00f98b3c756ebe7

The above command displays the actual contents of the manifest revision 10.

The manifest includes one line for each tracked file. Note that the two fields in each line are separated by *null character* ('\0'), not a space. Also note that *every line* ends in a linefeed ('\n'), including the last line. 

hg also offers a *manifest* command which displays all of the tracked files in the current revision:

::

   $ hg --debug manifest
   44754b8b0fc10af6beb2e369e1ab9049f45367ba 644   .hgignore
   16eb79a9f9f03fb89bcc4dc33446f11d43091674 644   .hgsigs
   b76e3114c5fecfa319d62a3aaef06eeab82e193b 644   .hgtags
   7c8afb9501740a450c549b4b1f002c803c45193a 644   CONTRIBUTORS
   5ac863e17c7035f1d11828d848fb2ca450d89794 644   COPYING
   1c2110687d65b7448b73c4a4c7c4b28f957eaf21 644   Makefile
   e4907aefc8dd5710417fb9887099a83fc14f7749 644   README
   73870a44b18b40b153acba2ed238d4bd305902c2 644   contrib/bash_completion
   fd3294ffa3b6095ec4b77f88241f468891324dab 755 * contrib/buildrpm
   dc0c4b232a1d0b5ec4f6c87f020c664b69b6cca8 755 * contrib/convert-repo
   2956444ba8a357c730fb3e9801c3d054351894f2 644   contrib/dumprevlog
   78f7c038716f258f451528e1e8241d895419f2ee 644   contrib/git-viz/git-cat-file
   78f7c038716f258f451528e1e8241d895419f2ee 644   contrib/git-viz/git-diff-tree
   78f7c038716f258f451528e1e8241d895419f2ee 644   contrib/git-viz/git-rev-list
   78f7c038716f258f451528e1e8241d895419f2ee 644   contrib/git-viz/git-rev-tree
   b58aa4c0ea58bb671532171ac4a2cd5957661531 644   contrib/git-viz/hg-viz
   70ceb076d0e3e8d1b688d726d55ef00402d92ddc 755 * contrib/hg-relink
   2943e43127ba9d2e2bdd4627a36787d0b56b5e6b 755 * contrib/hg-ssh
   560cd9ba449052f0222402155ea1d7f8bb0f87d2 755 * contrib/hgdiff
   1f9e835e1be5a6fb1b781b3be18967231bdb18a9 755 * contrib/hgk
   ...

Help text: http://www.selenic.com/mercurial/hg.1.html#manifest

