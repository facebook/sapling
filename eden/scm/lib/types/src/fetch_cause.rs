/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FetchCause {
    // Unknown orginination from EdenFS
    EdenUnknown,
    // The fetch originated from a Eden Thrift prefetch endpoint
    EdenPrefetch,
    // The fetch originated from a Eden Thrift endpoint
    EdenThrift,
    // The fetch originated from FUSE/NFS/PrjFS
    EdenFs,
    // The fetch originated from a mixed EdenFS causes
    EdenMixed,
    // The fetch originated from prefetch based on apparent EdenFS walk
    EdenWalkPrefetch,
    // The fetch originated from a Sapling prefetch
    SaplingPrefetch,
    // Unknown orginination from Sapling
    SaplingUnknown,
    // Unknown originiation, usually from Sapling (the default)
    Unspecified,
}

impl FetchCause {
    pub fn to_str(&self) -> &str {
        match self {
            FetchCause::EdenUnknown => "edenfs-unknown",
            FetchCause::EdenPrefetch => "edenfs-prefetch",
            FetchCause::EdenThrift => "edenfs-thrift",
            FetchCause::EdenFs => "edenfs-fs",
            FetchCause::EdenMixed => "edenfs-mixed",
            FetchCause::EdenWalkPrefetch => "eden-walk-prefetch",
            FetchCause::SaplingPrefetch => "sl-prefetch",
            FetchCause::SaplingUnknown => "sl-unknown",
            FetchCause::Unspecified => "unspecified",
        }
    }

    pub fn is_prefetch(&self) -> bool {
        match self {
            FetchCause::EdenPrefetch => true,
            FetchCause::SaplingPrefetch => true,
            FetchCause::EdenWalkPrefetch => true,

            FetchCause::EdenUnknown => false,
            FetchCause::EdenThrift => false,
            FetchCause::EdenFs => false,
            FetchCause::EdenMixed => false,
            FetchCause::SaplingUnknown => false,
            FetchCause::Unspecified => false,
        }
    }
}

impl std::str::FromStr for FetchCause {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "edenfs-unknown" => Ok(FetchCause::EdenUnknown),
            "edenfs-prefetch" => Ok(FetchCause::EdenPrefetch),
            "edenfs-thrift" => Ok(FetchCause::EdenThrift),
            "edenfs-fs" => Ok(FetchCause::EdenFs),
            "edenfs-mixed" => Ok(FetchCause::EdenMixed),
            "eden-walk-prefetch" => Ok(FetchCause::EdenWalkPrefetch),
            "sl-prefetch" => Ok(FetchCause::SaplingPrefetch),
            "sl-unknown" => Ok(FetchCause::SaplingUnknown),
            "unspecified" => Ok(FetchCause::Unspecified),
            _ => Err(anyhow::anyhow!("Invalid FetchCause string")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    #[test]
    fn test_fetch_cause_serialisation_deserialisation() {
        let variants = [
            FetchCause::EdenUnknown,
            FetchCause::EdenPrefetch,
            FetchCause::EdenThrift,
            FetchCause::EdenFs,
            FetchCause::EdenMixed,
            FetchCause::EdenWalkPrefetch,
            FetchCause::SaplingPrefetch,
            FetchCause::SaplingUnknown,
            FetchCause::Unspecified,
        ];

        for variant in variants {
            let string = variant.to_str();
            let parsed = FetchCause::from_str(string).unwrap();

            assert_eq!(variant, parsed);
        }
    }
}
