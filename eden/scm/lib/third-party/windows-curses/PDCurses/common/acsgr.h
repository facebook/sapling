/* ACS Unicode mapping with punchouts for box characters et al. */

chtype acs_map[128] =
{
    PDC_ACS(0), PDC_ACS(1), PDC_ACS(2), PDC_ACS(3), PDC_ACS(4),
    PDC_ACS(5), PDC_ACS(6), PDC_ACS(7), PDC_ACS(8), PDC_ACS(9),
    PDC_ACS(10), PDC_ACS(11), PDC_ACS(12), PDC_ACS(13), PDC_ACS(14),
    PDC_ACS(15), PDC_ACS(16), PDC_ACS(17), PDC_ACS(18), PDC_ACS(19),
    PDC_ACS(20), PDC_ACS(21), PDC_ACS(22), PDC_ACS(23), PDC_ACS(24),
    PDC_ACS(25), PDC_ACS(26), PDC_ACS(27), PDC_ACS(28), PDC_ACS(29),
    PDC_ACS(30), PDC_ACS(31), ' ', '!', '"', '#', '$', '%', '&', '\'',
    '(', ')', '*',

    0x2192, 0x2190, 0x2191, 0x2193,

    '/',

    ACS_BLOCK,

    '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=',
    '>', '?', '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J',
    'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W',
    'X', 'Y', 'Z', '[', '\\', ']', '^', '_',

    0x2666, 0x2592,

    'b', 'c', 'd', 'e',

    0x00b0, 0x00b1, 0x2591, 0x00a4, ACS_LRCORNER, ACS_URCORNER,
    ACS_ULCORNER, ACS_LLCORNER, ACS_PLUS, ACS_S1, ACS_S3, ACS_HLINE,
    ACS_S7, ACS_S9, ACS_LTEE, ACS_RTEE, ACS_BTEE, ACS_TTEE, ACS_VLINE,
    0x2264, 0x2265, 0x03c0, 0x2260, 0x00a3, 0x00b7,

    PDC_ACS(127)
};
