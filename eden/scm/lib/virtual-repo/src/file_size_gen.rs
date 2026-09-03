/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Generate file sizes used by synthetic repos.

#[derive(Clone, Copy, Debug)]
struct QuantileAnchor {
    rank: u64,
    size: u64,
}

// Measured from the file sizes in fbsource at revision
// c45402e8a6e5cf255f64bd361615b7e8aa3f5bbc. The measured bands and p10
// through p99.99 define the anchors through rank 0.9999. The remaining
// anchors fit the measured 70,098-byte mean and 6,204,166,650-byte maximum.
const QUANTILE_ANCHORS: &[QuantileAnchor] = &[
    QuantileAnchor { rank: 0, size: 0 },
    QuantileAnchor {
        rank: 88_355_219_025_805_221,
        size: 0,
    },
    QuantileAnchor {
        rank: 1_557_637_477_513_621_439,
        size: 255,
    },
    QuantileAnchor {
        rank: 1_844_674_407_370_955_162,
        size: 307,
    },
    QuantileAnchor {
        rank: 2_853_948_768_057_062_631,
        size: 511,
    },
    QuantileAnchor {
        rank: 4_611_686_018_427_387_904,
        size: 872,
    },
    QuantileAnchor {
        rank: 5_256_350_862_965_718_782,
        size: 1_023,
    },
    QuantileAnchor {
        rank: 9_223_372_036_854_775_808,
        size: 2_283,
    },
    QuantileAnchor {
        rank: 11_661_785_390_544_672_730,
        size: 4_095,
    },
    QuantileAnchor {
        rank: 13_835_058_055_282_163_711,
        size: 7_609,
    },
    QuantileAnchor {
        rank: 15_806_610_555_018_860_987,
        size: 16_383,
    },
    QuantileAnchor {
        rank: 16_602_069_666_338_596_454,
        size: 25_980,
    },
    QuantileAnchor {
        rank: 17_510_740_696_326_338_436,
        size: 65_535,
    },
    QuantileAnchor {
        rank: 17_524_406_870_024_074_034,
        size: 66_841,
    },
    QuantileAnchor {
        rank: 18_120_427_889_941_238_965,
        size: 262_143,
    },
    QuantileAnchor {
        rank: 18_220_235_101_463_316_220,
        size: 524_287,
    },
    QuantileAnchor {
        rank: 18_262_276_632_972_456_099,
        size: 583_064,
    },
    QuantileAnchor {
        rank: 18_333_297_495_209_887_891,
        size: 1_048_575,
    },
    QuantileAnchor {
        rank: 18_414_917_626_828_576_529,
        size: 4_194_303,
    },
    QuantileAnchor {
        rank: 18_428_297_329_635_842_063,
        size: 7_485_452,
    },
    QuantileAnchor {
        rank: 18_437_707_918_997_975_544,
        size: 16_777_215,
    },
    QuantileAnchor {
        rank: 18_444_532_721_357_953_035,
        size: 67_108_863,
    },
    QuantileAnchor {
        rank: 18_444_899_399_302_180_660,
        size: 75_464_128,
    },
    QuantileAnchor {
        rank: 18_445_083_866_742_917_755,
        size: 78_445_625,
    },
    QuantileAnchor {
        rank: 18_445_821_736_505_866_137,
        size: 97_375_551,
    },
    QuantileAnchor {
        rank: 18_446_559_606_268_814_519,
        size: 175_998_564,
    },
    QuantileAnchor {
        rank: 18_446_725_626_965_477_905,
        size: 410_466_473,
    },
    QuantileAnchor {
        rank: 18_446_742_229_035_144_244,
        size: 957_296_025,
    },
    QuantileAnchor {
        rank: 18_446_743_889_242_110_878,
        size: 2_232_620_050,
    },
    QuantileAnchor {
        rank: u64::MAX,
        size: 6_204_166_650,
    },
];

/// Convert a uniformly distributed value into the measured fbsource file-size
/// distribution.
pub fn generate_file_size(x: u64) -> u64 {
    let upper_index = QUANTILE_ANCHORS.partition_point(|anchor| anchor.rank < x);
    if upper_index == 0 {
        return QUANTILE_ANCHORS[0].size;
    }

    let lower = QUANTILE_ANCHORS[upper_index - 1];
    let upper = QUANTILE_ANCHORS[upper_index];
    let rank_offset = x - lower.rank;
    let rank_span = upper.rank - lower.rank;
    let size_span = upper.size - lower.size;
    lower.size + ((size_span as u128 * rank_offset as u128) / rank_span as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_match_measured_distribution() {
        for anchor in QUANTILE_ANCHORS {
            assert_eq!(generate_file_size(anchor.rank), anchor.size);
        }
    }

    #[test]
    fn distribution_is_monotonic() {
        for anchors in QUANTILE_ANCHORS.windows(2) {
            assert!(anchors[0].rank < anchors[1].rank);
            assert!(anchors[0].size <= anchors[1].size);
        }
    }

    #[test]
    fn estimated_mean_matches_fbsource() {
        let weighted_sum = QUANTILE_ANCHORS
            .windows(2)
            .map(|anchors| {
                let rank_span = (anchors[1].rank - anchors[0].rank) as f64;
                let average_size = (anchors[0].size + anchors[1].size) as f64 / 2.0;
                rank_span * average_size
            })
            .sum::<f64>();
        let mean = weighted_sum / u64::MAX as f64;

        assert!((mean - 70_098.0).abs() < 1.0, "estimated mean was {mean}");
    }
}
