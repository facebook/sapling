gethgversion() {
    make clean
    make local || make local PURE=--pure
    HG="$PWD/hg"

    $HG version > /dev/null || { echo 'abort: hg version failed!'; exit 1 ; }

    hgversion=`$HG version | sed -ne 's/.*(version \(.*\))$/\1/p'`

    if echo $hgversion | grep -- '-' > /dev/null 2>&1; then
        # nightly build case, version is like 1.3.1+250-20b91f91f9ca
        version=`echo $hgversion | cut -d- -f1`
        release=`echo $hgversion | cut -d- -f2 | sed -e 's/+.*//'`
    else
        # official tag, version is like 1.3.1
        version=`echo $hgversion | sed -e 's/+.*//'`
        release='0'
    fi
}
