// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::path::PathBuf;

use mononoke_types::MPath;

use crate::fsencode::fncache_fsencode;

fn check_fsencode_with_dotencode(path: &[u8], expected: &str) {
    let mut elements = vec![];
    let path = &MPath::new(path).unwrap();
    elements.extend(path.into_iter().cloned());

    assert_eq!(fncache_fsencode(&elements, true), PathBuf::from(expected));
}

#[test]
fn test_fsencode_from_core_hg() {
    let toencode = b"data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;=[]^`{}";
    let expected = "data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;=[]^`{}";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;=[]^`{}xxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "data/abcdefghijklmnopqrstuvwxyz0123456789 !#%&'()+,-.;=[]^`{}xxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/\"23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~2223456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxfc7e3ec7b0687ee06ed8c32fef0eb0c1980259f5";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/less <, greater >, colon :, double-quote \", backslash \\, pipe |, question-mark ?, asterisk *";
    let expected = "data/less ~3c, greater ~3e, colon ~3a, double-quote ~22, backslash ~5c, pipe ~7c, question-mark ~3f, asterisk ~2a";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let expected = "data/_a_b_c_d_e_f_g_h_i_j_k_l_m_n_o_p_q_r_s_t_u_v_w_x_y_z";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/_";
    let expected = "data/__";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/~";
    let expected = "data/~7e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/\x02\x03\x04\x05\x06\x07\x08\t\x0b\x0c\r\x0e\x0f\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f";
    let expected =
        "data/~02~03~04~05~06~07~08~09~0b~0c~0d~0e~0f~11~12~13~14~15~16~17~18~19~1a~1b~1c~1d~1e~1f";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/~\x7f\x80\x81\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x8b\x8c\x8d\x8e\x8f\x90\x91\x92\x93\x94\x95\x96\x97\x98\x99\x9a\x9b\x9c\x9d\x9e\x9f";
    let expected = "data/~7e~7f~80~81~82~83~84~85~86~87~88~89~8a~8b~8c~8d~8e~8f~90~91~92~93~94~95~96~97~98~99~9a~9b~9c~9d~9e~9f";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/\xa0\xa1\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xab\xac\xad\xae\xaf\xb0\xb1\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xbb\xbc\xbd\xbe\xbf";
    let expected = "data/~a0~a1~a2~a3~a4~a5~a6~a7~a8~a9~aa~ab~ac~ad~ae~af~b0~b1~b2~b3~b4~b5~b6~b7~b8~b9~ba~bb~bc~bd~be~bf";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/\xc0\xc1\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xcb\xcc\xcd\xce\xcf\xd0\xd1\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xdb\xdc\xdd\xde\xdf";
    let expected = "data/~c0~c1~c2~c3~c4~c5~c6~c7~c8~c9~ca~cb~cc~cd~ce~cf~d0~d1~d2~d3~d4~d5~d6~d7~d8~d9~da~db~dc~dd~de~df";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf0\xf1\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xfb\xfc\xfd\xfe\xff";
    let expected = "data/~e0~e1~e2~e3~e4~e5~e6~e7~e8~e9~ea~eb~ec~ed~ee~ef~f0~f1~f2~f3~f4~f5~f6~f7~f8~f9~fa~fb~fc~fd~fe~ff";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/x.h.i/x.hg/x.i/x.d/foo";
    let expected = "data/x.h.i.hg/x.hg.hg/x.i.hg/x.d.hg/foo";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/a.hg/a.i/a.d/foo";
    let expected = "data/a.hg.hg/a.i.hg/a.d.hg/foo";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/au.hg/au.i/au.d/foo";
    let expected = "data/au.hg.hg/au.i.hg/au.d.hg/foo";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/aux.hg/aux.i/aux.d/foo";
    let expected = "data/au~78.hg.hg/au~78.i.hg/au~78.d.hg/foo";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/auxy.hg/auxy.i/auxy.d/foo";
    let expected = "data/auxy.hg.hg/auxy.i.hg/auxy.d.hg/foo";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/x.hg";
    let expected = "data/foo/x.hg";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/x.i";
    let expected = "data/foo/x.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/x.d";
    let expected = "data/foo/x.d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/a.hg";
    let expected = "data/foo/a.hg";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/a.i";
    let expected = "data/foo/a.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/a.d";
    let expected = "data/foo/a.d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/au.hg";
    let expected = "data/foo/au.hg";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/au.i";
    let expected = "data/foo/au.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/au.d";
    let expected = "data/foo/au.d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/aux.hg";
    let expected = "data/foo/au~78.hg";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/aux.i";
    let expected = "data/foo/au~78.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/aux.d";
    let expected = "data/foo/au~78.d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/auxy.hg";
    let expected = "data/foo/auxy.hg";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/auxy.i";
    let expected = "data/foo/auxy.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo/auxy.d";
    let expected = "data/foo/auxy.d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/.hg/.i/.d/foo";
    let expected = "data/~2ehg.hg/~2ei.hg/~2ed.hg/foo";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/aux.bla/bla.aux/prn/PRN/lpt/com3/nul/coma/foo.NUL/normal.c.i";
    let expected =
        "data/au~78.bla/bla.aux/pr~6e/_p_r_n/lpt/co~6d3/nu~6c/coma/foo._n_u_l/normal.c.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/AUX/SECOND/X.PRN/FOURTH/FI:FTH/SIXTH/SEVENTH/EIGHTH/NINETH/TENTH/ELEVENTH/LOREMIPSUM.TXT.i";
    let expected = "dh/au~78/second/x.prn/fourth/fi~3afth/sixth/seventh/eighth/nineth/tenth/loremia20419e358ddff1bf8751e38288aff1d7c32ec05.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/enterprise/openesbaddons/contrib-imola/corba-bc/netbeansplugin/wsdlExtension/src/main/java/META-INF/services/org.netbeans.modules.xml.wsdl.bindingsupport.spi.ExtensibilityElementTemplateProvider.i";
    let expected = "dh/enterpri/openesba/contrib-/corba-bc/netbeans/wsdlexte/src/main/java/org.net7018f27961fdf338a598a40c4683429e7ffb9743.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/AUX.THE-QUICK-BROWN-FOX-JU:MPS-OVER-THE-LAZY-DOG-THE-QUICK-BROWN-FOX-JUMPS-OVER-THE-LAZY-DOG.TXT.i";
    let expected = "dh/au~78.the-quick-brown-fox-ju~3amps-over-the-lazy-dog-the-quick-brown-fox-jud4dcadd033000ab2b26eb66bae1906bcb15d4a70.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/Project Planning/Resources/AnotherLongDirectoryName/Followedbyanother/AndAnother/AndThenAnExtremelyLongFileName.txt";
    let expected = "dh/project_/resource/anotherl/followed/andanoth/andthenanextremelylongfilenaf93030515d9849cfdca52937c2204d19f83913e5.txt";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/Project.Planning/Resources/AnotherLongDirectoryName/Followedbyanother/AndAnother/AndThenAnExtremelyLongFileName.txt";
    let expected = "dh/project_/resource/anotherl/followed/andanoth/andthenanextremelylongfilena0fd7c506f5c9d58204444fc67e9499006bd2d445.txt";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/foo.../foo   / /a./_. /__/.x../    bla/.FOO/something.i";
    let expected =
        "data/foo..~2e/foo  ~20/~20/a~2e/__.~20/____/~2ex.~2e/~20   bla/~2e_f_o_o/something.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/c/co/com/com0/com1/com2/com3/com4/com5/com6/com7/com8/com9";
    let expected =
        "data/c/co/com/com0/co~6d1/co~6d2/co~6d3/co~6d4/co~6d5/co~6d6/co~6d7/co~6d8/co~6d9";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/C/CO/COM/COM0/COM1/COM2/COM3/COM4/COM5/COM6/COM7/COM8/COM9";
    let expected = "data/_c/_c_o/_c_o_m/_c_o_m0/_c_o_m1/_c_o_m2/_c_o_m3/_c_o_m4/_c_o_m5/_c_o_m6/_c_o_m7/_c_o_m8/_c_o_m9";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/c.x/co.x/com.x/com0.x/com1.x/com2.x/com3.x/com4.x/com5.x/com6.x/com7.x/com8.x/com9.x";
    let expected = "data/c.x/co.x/com.x/com0.x/co~6d1.x/co~6d2.x/co~6d3.x/co~6d4.x/co~6d5.x/co~6d6.x/co~6d7.x/co~6d8.x/co~6d9.x";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode =
        b"data/x.c/x.co/x.com0/x.com1/x.com2/x.com3/x.com4/x.com5/x.com6/x.com7/x.com8/x.com9";
    let expected =
        "data/x.c/x.co/x.com0/x.com1/x.com2/x.com3/x.com4/x.com5/x.com6/x.com7/x.com8/x.com9";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/cx/cox/comx/com0x/com1x/com2x/com3x/com4x/com5x/com6x/com7x/com8x/com9x";
    let expected = "data/cx/cox/comx/com0x/com1x/com2x/com3x/com4x/com5x/com6x/com7x/com8x/com9x";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/xc/xco/xcom0/xcom1/xcom2/xcom3/xcom4/xcom5/xcom6/xcom7/xcom8/xcom9";
    let expected = "data/xc/xco/xcom0/xcom1/xcom2/xcom3/xcom4/xcom5/xcom6/xcom7/xcom8/xcom9";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/l/lp/lpt/lpt0/lpt1/lpt2/lpt3/lpt4/lpt5/lpt6/lpt7/lpt8/lpt9";
    let expected =
        "data/l/lp/lpt/lpt0/lp~741/lp~742/lp~743/lp~744/lp~745/lp~746/lp~747/lp~748/lp~749";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/L/LP/LPT/LPT0/LPT1/LPT2/LPT3/LPT4/LPT5/LPT6/LPT7/LPT8/LPT9";
    let expected = "data/_l/_l_p/_l_p_t/_l_p_t0/_l_p_t1/_l_p_t2/_l_p_t3/_l_p_t4/_l_p_t5/_l_p_t6/_l_p_t7/_l_p_t8/_l_p_t9";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/l.x/lp.x/lpt.x/lpt0.x/lpt1.x/lpt2.x/lpt3.x/lpt4.x/lpt5.x/lpt6.x/lpt7.x/lpt8.x/lpt9.x";
    let expected = "data/l.x/lp.x/lpt.x/lpt0.x/lp~741.x/lp~742.x/lp~743.x/lp~744.x/lp~745.x/lp~746.x/lp~747.x/lp~748.x/lp~749.x";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/x.l/x.lp/x.lpt/x.lpt0/x.lpt1/x.lpt2/x.lpt3/x.lpt4/x.lpt5/x.lpt6/x.lpt7/x.lpt8/x.lpt9";
    let expected =
        "data/x.l/x.lp/x.lpt/x.lpt0/x.lpt1/x.lpt2/x.lpt3/x.lpt4/x.lpt5/x.lpt6/x.lpt7/x.lpt8/x.lpt9";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/lx/lpx/lptx/lpt0x/lpt1x/lpt2x/lpt3x/lpt4x/lpt5x/lpt6x/lpt7x/lpt8x/lpt9x";
    let expected = "data/lx/lpx/lptx/lpt0x/lpt1x/lpt2x/lpt3x/lpt4x/lpt5x/lpt6x/lpt7x/lpt8x/lpt9x";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/xl/xlp/xlpt/xlpt0/xlpt1/xlpt2/xlpt3/xlpt4/xlpt5/xlpt6/xlpt7/xlpt8/xlpt9";
    let expected = "data/xl/xlp/xlpt/xlpt0/xlpt1/xlpt2/xlpt3/xlpt4/xlpt5/xlpt6/xlpt7/xlpt8/xlpt9";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/con/p/pr/prn/a/au/aux/n/nu/nul";
    let expected = "data/co~6e/p/pr/pr~6e/a/au/au~78/n/nu/nu~6c";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/CON/P/PR/PRN/A/AU/AUX/N/NU/NUL";
    let expected = "data/_c_o_n/_p/_p_r/_p_r_n/_a/_a_u/_a_u_x/_n/_n_u/_n_u_l";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/con.x/p.x/pr.x/prn.x/a.x/au.x/aux.x/n.x/nu.x/nul.x";
    let expected = "data/co~6e.x/p.x/pr.x/pr~6e.x/a.x/au.x/au~78.x/n.x/nu.x/nu~6c.x";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/x.con/x.p/x.pr/x.prn/x.a/x.au/x.aux/x.n/x.nu/x.nul";
    let expected = "data/x.con/x.p/x.pr/x.prn/x.a/x.au/x.aux/x.n/x.nu/x.nul";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/conx/px/prx/prnx/ax/aux/auxx/nx/nux/nulx";
    let expected = "data/conx/px/prx/prnx/ax/au~78/auxx/nx/nux/nulx";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/xcon/xp/xpr/xprn/xa/xau/xaux/xn/xnu/xnul";
    let expected = "data/xcon/xp/xpr/xprn/xa/xau/xaux/xn/xnu/xnul";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/a./au./aux./auxy./aux.";
    let expected = "data/a~2e/au~2e/au~78~2e/auxy~2e/au~78~2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/c./co./con./cony./con.";
    let expected = "data/c~2e/co~2e/co~6e~2e/cony~2e/co~6e~2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/p./pr./prn./prny./prn.";
    let expected = "data/p~2e/pr~2e/pr~6e~2e/prny~2e/pr~6e~2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/n./nu./nul./nuly./nul.";
    let expected = "data/n~2e/nu~2e/nu~6c~2e/nuly~2e/nu~6c~2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/l./lp./lpt./lpt1./lpt1y./lpt1.";
    let expected = "data/l~2e/lp~2e/lpt~2e/lp~741~2e/lpt1y~2e/lp~741~2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/lpt9./lpt9y./lpt9.";
    let expected = "data/lp~749~2e/lpt9y~2e/lp~749~2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/com./com1./com1y./com1.";
    let expected = "data/com~2e/co~6d1~2e/com1y~2e/co~6d1~2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/com9./com9y./com9.";
    let expected = "data/co~6d9~2e/com9y~2e/co~6d9~2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/a /au /aux /auxy /aux ";
    let expected = "data/a~20/au~20/aux~20/auxy~20/aux~20";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789-123456789-123456789-123456789-123456789-unhashed--xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "data/123456789-123456789-123456789-123456789-123456789-unhashed--xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789-123456789-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/123456789-123456789-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxe9c55002b50bf5181e7a6fc1f60b126e2a6fcf71";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789-123456789-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxy-123456789-123456";
    let expected = "dh/123456789-123456789-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxd24fa4455faf8a94350c18e5eace7c2bb17af706";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/A23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/a23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxcbbc657029b41b94ed510d05feb6716a5c03bc6b";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/Z23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/z23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxx938f32a725c89512833fb96b6602dd9ebff51ddd";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/a23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "data/a23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/z23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "data/z23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/_23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/_23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxx9921a01af50feeabc060ce00eee4cba6efc31d2b";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/~23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~7e23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx9cec6f97d569c10995f785720044ea2e4227481b";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/<23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~3c23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxee67d8f275876ca1ef2500fc542e63c885c4e62d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/>23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~3e23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx387a85a5b1547cc9136310c974df716818458ddb";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/:23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~3a23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx2e4154fb571d13d22399c58cc4ef4858e4b75999";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/\\23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~5c23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx944e1f2b7110687e116e0d151328ac648b06ab4a";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/|23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~7c23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx28b23dd3fd0242946334126ab62bcd772aac32f4";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/?23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~3f23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxa263022d3994d2143d98f94f431eef8b5e7e0f8a";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/*23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~2a23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx0e7e6020e3c00ba7bb7893d84ca2966fbf53e140";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/ 23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~2023456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx92acbc78ef8c0b796111629a02601f07d8aec4ea";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/.23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~2e23456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxdbe19cc6505b3515ab9228cebf877ad07075168f";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-1234 ";
    let expected = "dh/123456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxx0025dc73e04f97426db4893e3bf67d581dc6d066";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-1234.";
    let expected = "dh/123456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxx85a16cf03ee7feba8a5abc626f1ba9886d01e89d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/ x/456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~20x/456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx1b3a3b712b2ac00d6af14ae8b4c14fdbf904f516";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/.x/456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/~2ex/456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx39dbc4c193a5643a8936fc69c3363cd7ac91ab14";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/x /456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/x~20/456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx2253c341df0b5290790ad312cd8499850f2273e5";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/x./456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/x~2e/456789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxcc0324d696d34562b44b5138db08ee1594ccc583";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/x.i/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/x.i.hg/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxa4c4399bdf81c67dbbbb7060aa0124d8dea94f74";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/x.d/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/x.d.hg/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxx1303fa90473b230615f5b3ea7b660e881ae5270a";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/x.hg/5789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/x.hg.hg/5789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxx26d724a8af68e7a4e4455e6602ea9adbd0eb801f";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/con/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/co~6e/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxc0794d4f4c605a2617900eb2563d7113cf6ea7d3";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/prn/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/pr~6e/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx64db876e1a9730e27236cb9b167aff942240e932";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/aux/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/au~78/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx8a178558405ca6fb4bbd75446dfa186f06751a0d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/nul/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/nu~6c/56789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxc5e51b6fec1bd07bd243b053a0c3f7209855b886";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/com1/6789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/co~6d1/6789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx32f5f44ece3bb62b9327369ca84cc19c86259fcd";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/com9/6789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/co~6d9/6789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxx734360b28c66a3230f55849fe8926206d229f990";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/lpt1/6789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/lp~741/6789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxe6f16ab4b6b0637676b2842b3345c9836df46ef7";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/lpt9/6789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "dh/lp~749/6789-123456789-123456789-123456789-123456789-xxxxxxxxx-xxxxxxxxx-xxxxxa475814c51acead3e44f2ff801f0c4903f986157";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789-123456789-123456789-123456789-123456789-/com/com0/lpt/lpt0/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    let expected = "data/123456789-123456789-123456789-123456789-123456789-/com/com0/lpt/lpt0/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12345";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/-123456789-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/-123456789-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxx4e9e9e384d00929a93b6835fbf976eb32321ff3c";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789/123456789-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/123456789-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxx1f4e4ec5f2be76e109bfaa8e31c062fe426d5490";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/9-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/9-123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxx3332d8329d969cf835542a9f2cbcfb385b6cf39d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789/123456789/123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/123456789-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx9699559798247dffa18717138859be5f8874840e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/89-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/89-123456789-123456789-hashed----xxxxxxxxx-xxxxxxxf0a2b053bb1369cce02f78c217d6a7aaea18c439";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789/123456789/123456789/123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/123456789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-1c6f8284967384ec13985a046d3553179d9d03cd";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/789-123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/789-123456789-hashed----xxxxxxxxx-xxxxxxx0d30c99049d8f0ff97b94d4ef302027e8d54c6fd";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789/123456789/123456789/123456789/123456789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/123456789-hashed----xxxxxxxxx-xxxxxxxxx-x46162779e1a771810b37a737f82ae7ed33771402";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/6789-hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/6789-hashed----xxxxxxxxx-xxxxxxxbfe752ddc8b003c2790c66a9f2eb1ea75c114390";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789/123456789/123456789/123456789/123456789/hashed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/hashed----xxxxxxxxx-xxxxxxxxx-xxb94c27b3532fa880cdd572b1c514785cab7b6ff2";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/ed----xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/ed----xxxxxxxxx-xxxxxxxcd8cc5483a0f3be409e0e5d4bf9e36e113c59235";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789/123456789/123456789/123456789/123456789/123456789/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/xxxxxxxxx-xxxxxxxxx-xxx47dd6f616f833a142da00701b334cebbf640da06";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/xxxxxx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/xxxxxx-xxxxxxx1c8ed635229fc22efe51035feeadeb4c8a0ecb82";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789/123456789/123456789/123456789/123456789/123456789/123456789/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/xxxxxxxxx-xxxx298ff7d33f8ce6db57930837ffea2fb2f48bb926";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345678/xxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/xxxxxxx-xxxxxxc8996ccd41b471f768057181a4d59d2febe7277d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/123456789/123456789/123456789/123456789/123456789/123456789/123456789/123456789/xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/xxxxxxxxx-xxxx4fa04a839a6bda93e1c21c713f2edcbd16e8890d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxx4d43d1ccaa20efbfe99ec779dc063611536ff2c5";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678x/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxx0f9efce65189cc60fd90fe4ffd49d7b58bbe0f2e";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678x/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxx945ca395708cafdd54a94501859beabd3e243921";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678x/12345678/12345678/12345678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxxac62bf6898c4fd0502146074547c11caa751a327";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678x/12345678/12345678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxx2ae5a2baed7983fae8974d0ca06c6bf08b9aee92";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678x/12345678/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxx214aba07b6687532a43d1e9eaf6e88cfca96b68c";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678x/12345678/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxxe7a022ae82f0f55cf4e0498e55ba59ea4ebb55bf";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678x/12345/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxxxxb51ce61164996a80f36ce3cfe64b62d519aedae3";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/123456/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/xxxxxxxxx-xxxx11fa9873cc6c3215eae864528b5530a04efc6cfe";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/1234./-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/-xxxxxxxxx-xxx602df9b45bec564e2e1f0645d5140dddcc76ed58";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/1234 /-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/-xxxxxxxxx-xxxd99ff212bc84b4d1f70cd6b0071e3ef69d4e12ce";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12./xx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12~2e/xx-xxxxx7baeb5ed7f14a586ee1cacecdbcbff70032d1b3c";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12 /xx-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-123456";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12~20/xx-xxxxxcf79ca9795f77d7f75745da36807e5d772bd5182";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.345.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxc10ad03b5755ed524f5286aab1815dfe07729438.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.345.d";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxx9eec83381f2b39ef5ac8b4ecdf2c94f7983f57c8.d";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxb7796dc7d175cfb0bb8a7728f58f6ebec9042568.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.34567.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxb515857a6bfeef017c4894d8df42458ac65d55b8.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.345678.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxb05a0f247bc0a776211cd6a32ab714fd9cc09f2b.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxf192b48bff08d9e0e12035fb52bc58c70de72c94.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789-.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxx435551e0ed4c7b083b9ba83cee916670e02e80ad.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789-1.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxa7f74eb98d8d58b716356dfd26e2f9aaa65d6a9a.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789-12.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxed68d9bd43b931f0b100267fee488d65a0c66f62.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789-123.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxx5cea44de2b642d2ba2b4a30693ffb1049644d698.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789-1234.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxx68462f62a7f230b39c1b5400d73ec35920990b7e.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789-12345.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxx4cb852a314c6da240a83eec94761cdd71c6ec22e.i";
    check_fsencode_with_dotencode(&toencode[..], expected);

    let toencode = b"data/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789-12345-ABCDEFGHIJKLMNOPRSTUVWXYZ-abcdefghjiklmnopqrstuvwxyz-ABCDEFGHIJKLMNOPRSTUVWXYZ-1234567890-xxxxxxxxx-xxxxxxxxx-xxxxxxxx-xxxxxxxxx-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww-wwwwwwwww.i";
    let expected = "dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/-xxxxx93352aa50377751d9e5ebdf52da1e6e69a6887a6.i";
    check_fsencode_with_dotencode(&toencode[..], expected);
}
