from __future__ import absolute_import, print_function
from mercurial import (
    store,
)

def show(s):
    # show test input
    print("A = '%s'" % s.encode("string_escape"))

    # show the result of the C implementation, if available
    h = store._pathencode(s)
    print("B = '%s'" % h.encode("string_escape"))

    # compare it with reference implementation in Python
    r = store._hybridencode(s, True)
    if h != r:
        print("R = '%s'" % r.encode("string_escape"))
    print()

show("data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;=[]^`{}")

print("uppercase char X is encoded as _x")
show("data/ABCDEFGHIJKLMNOPQRSTUVWXYZ")

print("underbar is doubled")
show("data/_")

print("tilde is character-encoded")
show("data/~")

print("characters in ASCII code range 1..31")
show('data/\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f'
          '\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f')

print("characters in ASCII code range 126..255")
show('data/\x7e\x7f'
          '\x80\x81\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x8b\x8c\x8d\x8e\x8f'
          '\x90\x91\x92\x93\x94\x95\x96\x97\x98\x99\x9a\x9b\x9c\x9d\x9e\x9f')
show('data/\xa0\xa1\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xab\xac\xad\xae\xaf'
          '\xb0\xb1\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xbb\xbc\xbd\xbe\xbf')
show('data/\xc0\xc1\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xcb\xcc\xcd\xce\xcf'
          '\xd0\xd1\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xdb\xdc\xdd\xde\xdf')
show('data/\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef'
          '\xf0\xf1\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xfb\xfc\xfd\xfe\xff')

print("Windows reserved characters")
show('data/less <, greater >, colon :, double-quote ", backslash \\'
           ', pipe |, question-mark ?, asterisk *')

print("encoding directories ending in .hg, .i or .d with '.hg' suffix")
show('data/x.h.i/x.hg/x.i/x.d/foo')
show('data/a.hg/a.i/a.d/foo')
show('data/au.hg/au.i/au.d/foo')
show('data/aux.hg/aux.i/aux.d/foo')
show('data/auxy.hg/auxy.i/auxy.d/foo')

print("but these are not encoded on *filenames*")
show('data/foo/x.hg')
show('data/foo/x.i')
show('data/foo/x.d')
show('data/foo/a.hg')
show('data/foo/a.i')
show('data/foo/a.d')
show('data/foo/au.hg')
show('data/foo/au.i')
show('data/foo/au.d')
show('data/foo/aux.hg')
show('data/foo/aux.i')
show('data/foo/aux.d')
show('data/foo/auxy.hg')
show('data/foo/auxy.i')
show('data/foo/auxy.d')

print("plain .hg, .i and .d directories have the leading dot encoded")
show('data/.hg/.i/.d/foo')

show('data/aux.bla/bla.aux/prn/PRN/lpt/com3/nul/coma/foo.NUL/normal.c.i')

show('data/AUX/SECOND/X.PRN/FOURTH/FI:FTH/SIXTH/SEVENTH/EIGHTH/NINETH/'
     'TENTH/ELEVENTH/LOREMIPSUM.TXT.i')
show('data/enterprise/openesbaddons/contrib-imola/corba-bc/netbeansplugin/'
     'wsdlExtension/src/main/java/META-INF/services/org.netbeans.modules'
     '.xml.wsdl.bindingsupport.spi.ExtensibilityElementTemplateProvider.i')
show('data/AUX.THE-QUICK-BROWN-FOX-JU:MPS-OVER-THE-LAZY-DOG-THE-QUICK-'
     'BROWN-FOX-JUMPS-OVER-THE-LAZY-DOG.TXT.i')
show('data/Project Planning/Resources/AnotherLongDirectoryName/'
     'Followedbyanother/AndAnother/AndThenAnExtremelyLongFileName.txt')
show('data/Project.Planning/Resources/AnotherLongDirectoryName/'
     'Followedbyanother/AndAnother/AndThenAnExtremelyLongFileName.txt')
show('data/foo.../foo   / /a./_. /__/.x../    bla/.FOO/something.i')

show('data/c/co/com/com0/com1/com2/com3/com4/com5/com6/com7/com8/com9')
show('data/C/CO/COM/COM0/COM1/COM2/COM3/COM4/COM5/COM6/COM7/COM8/COM9')
show('data/c.x/co.x/com.x/com0.x/com1.x/com2.x/com3.x/com4.x/com5.x'
                                        '/com6.x/com7.x/com8.x/com9.x')
show('data/x.c/x.co/x.com0/x.com1/x.com2/x.com3/x.com4/x.com5'
                                        '/x.com6/x.com7/x.com8/x.com9')
show('data/cx/cox/comx/com0x/com1x/com2x/com3x/com4x/com5x'
                                            '/com6x/com7x/com8x/com9x')
show('data/xc/xco/xcom0/xcom1/xcom2/xcom3/xcom4/xcom5'
                                            '/xcom6/xcom7/xcom8/xcom9')

show('data/l/lp/lpt/lpt0/lpt1/lpt2/lpt3/lpt4/lpt5/lpt6/lpt7/lpt8/lpt9')
show('data/L/LP/LPT/LPT0/LPT1/LPT2/LPT3/LPT4/LPT5/LPT6/LPT7/LPT8/LPT9')
show('data/l.x/lp.x/lpt.x/lpt0.x/lpt1.x/lpt2.x/lpt3.x/lpt4.x/lpt5.x'
                                        '/lpt6.x/lpt7.x/lpt8.x/lpt9.x')
show('data/x.l/x.lp/x.lpt/x.lpt0/x.lpt1/x.lpt2/x.lpt3/x.lpt4/x.lpt5'
                                        '/x.lpt6/x.lpt7/x.lpt8/x.lpt9')
show('data/lx/lpx/lptx/lpt0x/lpt1x/lpt2x/lpt3x/lpt4x/lpt5x'
                                            '/lpt6x/lpt7x/lpt8x/lpt9x')
show('data/xl/xlp/xlpt/xlpt0/xlpt1/xlpt2/xlpt3/xlpt4/xlpt5'
                                            '/xlpt6/xlpt7/xlpt8/xlpt9')

show('data/con/p/pr/prn/a/au/aux/n/nu/nul')
show('data/CON/P/PR/PRN/A/AU/AUX/N/NU/NUL')
show('data/con.x/p.x/pr.x/prn.x/a.x/au.x/aux.x/n.x/nu.x/nul.x')
show('data/x.con/x.p/x.pr/x.prn/x.a/x.au/x.aux/x.n/x.nu/x.nul')
show('data/conx/px/prx/prnx/ax/aux/auxx/nx/nux/nulx')
show('data/xcon/xp/xpr/xprn/xa/xau/xaux/xn/xnu/xnul')

show('data/a./au./aux./auxy./aux.')
show('data/c./co./con./cony./con.')
show('data/p./pr./prn./prny./prn.')
show('data/n./nu./nul./nuly./nul.')
show('data/l./lp./lpt./lpt1./lpt1y./lpt1.')
show('data/lpt9./lpt9y./lpt9.')
show('data/com./com1./com1y./com1.')
show('data/com9./com9y./com9.')

show('data/a /au /aux /auxy /aux ')

print("largest unhashed path")
show('data/123456789-123456789-123456789-123456789-123456789-'
          'unhashed--xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("shortest hashed path")
show('data/123456789-123456789-123456789-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("changing one char in part that's hashed away produces a different hash")
show('data/123456789-123456789-123456789-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxy-'
          '123456789-123456')

print("uppercase hitting length limit due to encoding")
show('data/A23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/Z23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("compare with lowercase not hitting limit")
show('data/a23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/z23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("not hitting limit with any of these")
show("data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;="
          "[]^`{}xxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-"
          "123456789-12345")

print("underbar hitting length limit due to encoding")
show('data/_23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("tilde hitting length limit due to encoding")
show('data/~23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("Windows reserved characters hitting length limit")
show('data/<23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/>23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/:23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/"23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/\\23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/|23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/?23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/*23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("initial space hitting length limit")
show('data/ 23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("initial dot hitting length limit")
show('data/.23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("trailing space in filename hitting length limit")
show('data/123456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-1234 ')

print("trailing dot in filename hitting length limit")
show('data/123456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-1234.')

print("initial space in directory hitting length limit")
show('data/ x/456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("initial dot in directory hitting length limit")
show('data/.x/456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("trailing space in directory hitting length limit")
show('data/x /456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("trailing dot in directory hitting length limit")
show('data/x./456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("with directories that need direncoding, hitting length limit")
show('data/x.i/56789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/x.d/56789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/x.hg/5789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("Windows reserved filenames, hitting length limit")
show('data/con/56789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/prn/56789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/aux/56789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/nul/56789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/com1/6789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/com9/6789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/lpt1/6789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/lpt9/6789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("non-reserved names, just not hitting limit")
show('data/123456789-123456789-123456789-123456789-123456789-'
          '/com/com0/lpt/lpt0/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print("hashed path with largest untruncated 1st dir")
show('data/12345678/-123456789-123456789-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with smallest truncated 1st dir")
show('data/123456789/123456789-123456789-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with largest untruncated two dirs")
show('data/12345678/12345678/9-123456789-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with smallest truncated two dirs")
show('data/123456789/123456789/123456789-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with largest untruncated three dirs")
show('data/12345678/12345678/12345678/89-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with smallest truncated three dirs")
show('data/123456789/123456789/123456789/123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with largest untruncated four dirs")
show('data/12345678/12345678/12345678/12345678/789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with smallest truncated four dirs")
show('data/123456789/123456789/123456789/123456789/123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with largest untruncated five dirs")
show('data/12345678/12345678/12345678/12345678/12345678/6789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with smallest truncated five dirs")
show('data/123456789/123456789/123456789/123456789/123456789/'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with largest untruncated six dirs")
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/ed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with smallest truncated six dirs")
show('data/123456789/123456789/123456789/123456789/123456789/'
          '123456789/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with largest untruncated seven dirs")
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/xxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with smallest truncated seven dirs")
show('data/123456789/123456789/123456789/123456789/123456789/'
          '123456789/123456789/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with largest untruncated eight dirs")
print("(directory 8 is dropped because it hits _maxshortdirslen)")
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345678/xxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with smallest truncated eight dirs")
print("(directory 8 is dropped because it hits _maxshortdirslen)")
show('data/123456789/123456789/123456789/123456789/123456789/'
          '123456789/123456789/123456789/xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with largest non-dropped directory 8")
print("(just not hitting the _maxshortdirslen boundary)")
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("...adding one truncated char to dir 1..7 won't drop dir 8")
show('data/12345678x/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')
show('data/12345678/12345678x/12345678/12345678/12345678/12345'
          '678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')
show('data/12345678/12345678/12345678x/12345678/12345678/12345'
          '678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')
show('data/12345678/12345678/12345678/12345678x/12345678/12345'
          '678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')
show('data/12345678/12345678/12345678/12345678/12345678x/12345'
          '678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678x/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678x/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path with shortest dropped directory 8")
print("(just hitting the _maxshortdirslen boundary)")
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/123456/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("hashed path that drops dir 8 due to dot or space at end is")
print("encoded, and thus causing to hit _maxshortdirslen")
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/1234./-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/1234 /-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print("... with dir 8 short enough for encoding")
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12./xx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12 /xx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print('''Extensions are replicated on hashed paths. Note that
we only get to encode files that end in .i or .d inside the
store. Encoded filenames are thus bound in length.''')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.345.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.345.d')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.34567.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.345678.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456789.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456789-.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456789-1.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456789-12.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456789-123.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456789-1234.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456789-12345.i')
show('data/12345678/12345678/12345678/12345678/12345678/12345'
          '678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12.3456789-12345-ABCDEFGHIJKLMNOPRSTUVWX'
          'YZ-abcdefghjiklmnopqrstuvwxyz-ABCDEFGHIJKLMNOPRSTU'
          'VWXYZ-1234567890-xxxxxxxxx-xxxxxxxxx-xxxxxxxx-xxxx'
          'xxxxx-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwww'
          'wwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww.i')

print("paths outside data/ can be encoded")
show('metadata/dir/00manifest.i')
show('metadata/12345678/12345678/12345678/12345678/12345678/'
          '12345678/12345678/12345678/12345678/12345678/12345678/'
          '12345678/12345678/00manifest.i')
