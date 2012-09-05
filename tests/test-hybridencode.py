from mercurial import store

auxencode = lambda f: store._auxencode(f, True)
hybridencode = lambda f: store._hybridencode(f, auxencode)

enc = hybridencode # used for 'dotencode' repo format

def show(s):
    print "A = '%s'" % s.encode("string_escape")
    print "B = '%s'" % enc(s).encode("string_escape")
    print

show("data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;=[]^`{}")

print "uppercase char X is encoded as _x"
show("data/ABCDEFGHIJKLMNOPQRSTUVWXYZ")

print "underbar is doubled"
show("data/_")

print "tilde is character-encoded"
show("data/~")

print "characters in ASCII code range 1..31"
show('data/\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f'
          '\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f')

print "characters in ASCII code range 126..255 (only partially tested)"
show('data/\x7e \x7f \x80 \x81 \x82 \x83 .. \xfd \xfe \xff')

print "Windows reserved characters"
show('data/less <, greater >, colon :, double-quote ", backslash \\'
           ', pipe |, question-mark ?, asterisk *')

print "encoding directories ending in .hg, .i or .d with '.hg' suffix"
show('data/x.hg/x.i/x.d/foo')

print "but these are not encoded on *filenames*"
show('data/foo/x.hg')
show('data/foo/x.i')
show('data/foo/x.d')

print "plain .hg, .i and .d directories have the leading dot encoded"
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

print "largest unhashed path"
show('data/123456789-123456789-123456789-123456789-123456789-'
          'unhashed--xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "shortest hashed path"
show('data/123456789-123456789-123456789-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-123456')

print "changing one char in part that's hashed away produces a different hash"
show('data/123456789-123456789-123456789-123456789-123456789-'
          'hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxy-'
          '123456789-123456')

print "uppercase hitting length limit due to encoding"
show('data/A23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/Z23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "compare with lowercase not hitting limit"
show('data/a23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/z23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "not hitting limit with any of these"
show("data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;="
          "[]^`{}xxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-"
          "123456789-12345")

print "underbar hitting length limit due to encoding"
show('data/_23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "tilde hitting length limit due to encoding"
show('data/~23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "Windows reserved characters hitting length limit"
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

print "initial space hitting length limit"
show('data/ 23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "initial dot hitting length limit"
show('data/.23456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "trailing space in filename hitting length limit"
show('data/123456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-1234 ')

print "trailing dot in filename hitting length limit"
show('data/123456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-1234.')

print "initial space in directory hitting length limit"
show('data/ x/456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "initial dot in directory hitting length limit"
show('data/.x/456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "trailing space in directory hitting length limit"
show('data/x /456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "trailing dot in directory hitting length limit"
show('data/x./456789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "with directories that need direncoding, hitting length limit"
show('data/x.i/56789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/x.d/56789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
show('data/x.hg/5789-123456789-123456789-123456789-123456789-'
          'xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')

print "Windows reserved filenames, hitting length limit"
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

print "non-reserved names, just not hitting limit"
show('data/123456789-123456789-123456789-123456789-123456789-'
          '/com/com0/lpt/lpt0/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-'
          '123456789-12345')
