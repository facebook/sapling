# Extract version number into 4 parts, some of which may be empty:
#
# version: the numeric part of the most recent tag. Will always look like 1.3.
#
# type: if an rc build, "rc", otherwise empty
#
# distance: the distance from the nearest tag, or empty if built from a tag
#
# node: the node|short hg was built from, or empty if built from a tag
gethgversion() {
    make clean
    make local || make local PURE=--pure
    HG="$PWD/hg"

    $HG version > /dev/null || { echo 'abort: hg version failed!'; exit 1 ; }

    hgversion=`$HG version | sed -ne 's/.*(version \(.*\))$/\1/p'`

    if echo $hgversion | grep + > /dev/null 2>&1 ; then
        tmp=`echo $hgversion | cut -d+ -f 2`
        hgversion=`echo $hgversion | cut -d+ -f 1`
        distance=`echo $tmp | cut -d- -f 1`
        node=`echo $tmp | cut -d- -f 2`
    else
        distance=''
        node=''
    fi
    if echo $hgversion | grep -- '-' > /dev/null 2>&1; then
        version=`echo $hgversion | cut -d- -f1`
        type=`echo $hgversion | cut -d- -f2`
    else
        version=$hgversion
        type=''
    fi
}
