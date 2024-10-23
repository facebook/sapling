/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::io::BufRead;
use std::io::Result;
use std::io::Write;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use base64::alphabet::STANDARD;
use base64::engine::general_purpose::GeneralPurpose;
use base64::engine::general_purpose::GeneralPurposeConfig;
use base64::engine::DecodePaddingMode;
use base64::Engine;
use blake3::Hasher as Blake3Hasher;
use minibytes::Bytes;

use crate::Blake3;
use crate::CasDigest;
use crate::FileType;
use crate::HgId;
use crate::Id20;
use crate::PathComponentBuf;
use crate::Sha1;

// Bring back the pre 0.20 bevahiour and allow either padded or un-padded base64 strings at decode time.
const STANDARD_INDIFFERENT: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Clone, Debug, PartialEq)]
pub struct AugmentedFileNode {
    pub file_type: FileType,
    pub filenode: HgId,
    pub content_blake3: Blake3,
    pub content_sha1: Sha1,
    pub total_size: u64,
    pub file_header_metadata: Option<Bytes>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AugmentedDirectoryNode {
    pub treenode: HgId,
    pub augmented_manifest_id: Blake3,
    pub augmented_manifest_size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AugmentedTreeEntry {
    FileNode(AugmentedFileNode),
    DirectoryNode(AugmentedDirectoryNode),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AugmentedTree {
    pub hg_node_id: HgId,
    // The computed_hg_node_id can be used for sha1 hash validation of
    // a sapling tree blob. This is only for the old root trees where
    // hg_node_id can be different (hash of the content of flat manifest)
    pub computed_hg_node_id: Option<HgId>,
    pub p1: Option<HgId>,
    pub p2: Option<HgId>,
    pub entries: Vec<(PathComponentBuf, AugmentedTreeEntry)>,
}

impl AugmentedTree {
    pub fn sapling_tree_blob_size(&self) -> usize {
        let mut size: usize = 0;
        for (path, subentry) in self.entries.iter() {
            size += path.len() + 2;
            match subentry {
                AugmentedTreeEntry::DirectoryNode(_) => {
                    size += HgId::hex_len() + 1;
                }
                AugmentedTreeEntry::FileNode(_) => {
                    size += HgId::hex_len();
                }
            };
        }
        size
    }

    pub fn write_sapling_tree_blob(&self, mut w: impl Write) -> Result<()> {
        for (path, subentry) in self.entries.iter() {
            w.write_all(path.as_ref())?;
            w.write_all(b"\0")?;
            match subentry {
                AugmentedTreeEntry::DirectoryNode(directory) => {
                    w.write_all(directory.treenode.to_hex().as_bytes())?;
                    w.write_all(b"t")?;
                }
                AugmentedTreeEntry::FileNode(file) => {
                    w.write_all(file.filenode.to_hex().as_bytes())?;
                }
            };
            w.write_all(b"\n")?;
        }
        Ok(())
    }

    // The format of the content addressed manifest blob is as follows:
    //
    // entry ::= <path> '\0' <hg-node-hex> <type> ' ' <entry-value> '\n'
    //
    // entry-value ::= <cas-blake3-hex> ' ' <size-dec> ' ' <sha1-hex> ' ' <base64(file_header_metadata) (if present) or '-'>
    //               | <cas-blake3-hex> ' ' <size-dec>
    //
    // tree ::= <version> ' ' <sha1-hex> ' ' <computed_sha1-hex (if different) or '-'> ' ' <p1-hex or '-'> ' ' <p2-hex or '-'> '\n' <entry>*

    /// Method estimates the size of the serialize in bytes, so that it can be used to preallocate memory.
    pub fn augmented_tree_blob_size(&self) -> usize {
        let mut size: usize = 3;
        size += HgId::hex_len();
        size += 1;
        if let Some(_computed_nodeid) = self.computed_hg_node_id {
            size += HgId::hex_len();
        } else {
            size += 1;
        }
        size += 1;
        if let Some(_p1) = &self.p1 {
            size += HgId::hex_len();
        } else {
            size += 1;
        }
        size += 1;
        if let Some(_p2) = &self.p2 {
            size += HgId::hex_len();
        } else {
            size += 1;
        }
        size += 1;
        for (path, subentry) in self.entries.iter() {
            size += path.len() + 1;
            match subentry {
                AugmentedTreeEntry::FileNode(f) => {
                    size += HgId::hex_len() + Blake3::hex_len() + 3;
                    size += f.total_size.to_string().len() + 1;
                    size += Id20::hex_len();
                    size += 1;
                    if let Some(file_header_metadata) = &f.file_header_metadata {
                        let n = file_header_metadata.len();
                        size += ((n + 2) / 3) * 4; // base64 encoding overhead
                    } else {
                        size += 1;
                    };
                }
                AugmentedTreeEntry::DirectoryNode(d) => {
                    size += HgId::hex_len() + Blake3::hex_len() + 3;
                    size += d.augmented_manifest_size.to_string().len();
                }
            };
            size += 1;
        }
        size
    }

    /// Serialize the AugmentedTree to a blob.
    pub fn try_serialize(&self, mut w: impl Write) -> Result<()> {
        w.write_all(b"v1 ")?;
        w.write_all(self.hg_node_id.to_hex().as_ref())?;
        w.write_all(b" ")?;
        if let Some(computed_nodeid) = self.computed_hg_node_id {
            w.write_all(computed_nodeid.to_hex().as_ref())?;
        } else {
            w.write_all(b"-")?;
        }
        w.write_all(b" ")?;
        if let Some(p1) = &self.p1 {
            w.write_all(p1.to_hex().as_ref())?;
        } else {
            w.write_all(b"-")?;
        };
        w.write_all(b" ")?;
        if let Some(p2) = &self.p2 {
            w.write_all(p2.to_hex().as_ref())?;
        } else {
            w.write_all(b"-")?;
        };
        w.write_all(b"\n")?;
        for (path, subentry) in self.entries.iter() {
            w.write_all(path.as_ref())?;
            w.write_all(b"\0")?;
            match subentry {
                AugmentedTreeEntry::FileNode(file) => {
                    w.write_all(file.filenode.to_hex().as_ref())?;
                    w.write_all(match file.file_type {
                        FileType::Regular => b"r",
                        FileType::Executable => b"x",
                        FileType::Symlink => b"l",
                        FileType::GitSubmodule => {
                            return Err(io::Error::new(
                                io::ErrorKind::Other,
                                anyhow!("submodules not supported in augmented manifests"),
                            ));
                        }
                    })?;
                    w.write_all(b" ")?;
                    w.write_all(file.content_blake3.to_hex().as_ref())?;
                    w.write_all(b" ")?;
                    w.write_all(file.total_size.to_string().as_bytes())?;
                    w.write_all(b" ")?;
                    w.write_all(file.content_sha1.to_hex().as_ref())?;
                    w.write_all(b" ")?;
                    if let Some(file_header_metadata) = &file.file_header_metadata {
                        w.write_all(
                            base64::engine::general_purpose::STANDARD
                                .encode(file_header_metadata)
                                .as_bytes(),
                        )?;
                    } else {
                        w.write_all(b"-")?;
                    };
                }
                AugmentedTreeEntry::DirectoryNode(directory) => {
                    w.write_all(directory.treenode.to_hex().as_ref())?;
                    w.write_all(b"t")?;
                    w.write_all(b" ")?;
                    w.write_all(directory.augmented_manifest_id.to_hex().as_ref())?;
                    w.write_all(b" ")?;
                    w.write_all(directory.augmented_manifest_size.to_string().as_ref())?;
                }
            }
            w.write_all(b"\n")?;
        }
        Ok(())
    }

    /// Constructs an AugmentedTree from serialised blob in the format used by Mononoke to store the Augmented Trees in CAS.
    pub fn try_deserialize(mut reader: impl BufRead) -> anyhow::Result<Self, Error> {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let mut header = line.split(' ');

        let version = header
            .next()
            .ok_or(anyhow!("augmented tree: missing version"))?;
        if version != "v1" {
            return Err(anyhow!("augmented tree: unsupported version"));
        }

        let hg_node_id = HgId::from_hex(
            header
                .next()
                .ok_or(anyhow!("augmented tree: missing node id"))?
                .as_ref(),
        )?;

        let computed_hg_node_id = header
            .next()
            .ok_or(anyhow!("augmented tree: missing computed node id"))?;

        let computed_hg_node_id = if computed_hg_node_id == "-" {
            None
        } else {
            Some(HgId::from_hex(computed_hg_node_id.as_ref())?)
        };

        let p1 = header.next().ok_or(anyhow!("augmented tree: missing p1"))?;
        let p1 = if p1 == "-" {
            None
        } else {
            Some(HgId::from_hex(p1.as_ref())?)
        };

        let p2 = header
            .next()
            .ok_or(anyhow!("augmented tree: missing p2"))?
            .trim();
        let p2 = if p2 == "-" {
            None
        } else {
            Some(HgId::from_hex(p2.as_ref())?)
        };

        anyhow::Ok(Self {
            hg_node_id,
            computed_hg_node_id,
            p1,
            p2,
            entries: reader
                .lines()
                .map(|line| {
                    let line = line?;
                    let line = line.trim();

                    let (path, rest) = line
                        .split_once('\0')
                        .ok_or(anyhow!("augmented tree: invalid format of a child entry"))?;

                    let mut parts = rest.split(' ');
                    let id = parts.next().ok_or(anyhow!(
                        "augmented tree: missing id part in a child entry"
                    ))?;

                    let mut id = id.to_string();
                    let flag = id.pop().ok_or(anyhow!(
                        "augmented tree: missing flag part in a child entry"
                    ))?;
                    let hgid = HgId::from_hex(id.as_ref())?;
                    let blake3 = parts.next().ok_or(anyhow!(
                        "augmented tree: missing blake3 part in a child entry"
                    ))?;
                    let blake3 = Blake3::from_hex(blake3.as_ref())?;

                    let size = parts
                        .next()
                        .ok_or(anyhow!(
                            "augmented tree: missing size part in a child entry"
                        ))?
                        .trim();
                    let size = size.parse::<u64>()?;

                    match flag {
                        't' => Ok((
                            path.to_string().try_into()?,
                            AugmentedTreeEntry::DirectoryNode(AugmentedDirectoryNode {
                                treenode: hgid,
                                augmented_manifest_id: blake3,
                                augmented_manifest_size: size,
                            }),
                        )),
                        _ => {
                            let sha1 = parts
                                .next()
                                .ok_or(anyhow!(
                                    "augmented tree: missing sha1 part in a child entry"
                                ))?
                                .trim();

                            let sha1 = Sha1::from_hex(sha1.as_ref())?;

                            let file_header_metadata = parts
                            .next()
                            .ok_or(anyhow!(
                                "augmented tree: missing file_header_metadata part in a child entry"
                            ))?
                            .trim();

                            let file_header_metadata = if file_header_metadata == "-" {
                                None
                            } else {
                                Some(Bytes::from(
                                    STANDARD_INDIFFERENT.decode(file_header_metadata)?,
                                ))
                            };

                            Ok((
                                path.to_string().try_into()?,
                                AugmentedTreeEntry::FileNode(AugmentedFileNode {
                                    file_type: match flag {
                                        'l' => FileType::Symlink,
                                        'x' => FileType::Executable,
                                        'r' => FileType::Regular,
                                        _ => bail!("augmented tree: invalid flag '{flag}' in a child entry for tree {hg_node_id}")
                                    },
                                    filenode: hgid,
                                    content_blake3: blake3,
                                    content_sha1: sha1,
                                    total_size: size,
                                    file_header_metadata,
                                }),
                            ))
                        }
                    }
                })
                .collect::<anyhow::Result<Vec<(PathComponentBuf, AugmentedTreeEntry)>, Error>>()?,
        })
    }

    pub fn compute_content_addressed_digest(&self) -> anyhow::Result<CasDigest> {
        let mut calculator = AugmentedTreeDigestCalculator::new();
        self.try_serialize(&mut calculator)?;
        calculator.finalize()
    }
}

struct AugmentedTreeDigestCalculator {
    hasher: Blake3Hasher,
    size: u64,
}

impl AugmentedTreeDigestCalculator {
    fn new() -> Self {
        #[cfg(fbcode_build)]
        let key = blake3_constants::BLAKE3_HASH_KEY;
        #[cfg(not(fbcode_build))]
        let key = b"20220728-2357111317192329313741#";
        Self {
            hasher: Blake3Hasher::new_keyed(key),
            size: 0,
        }
    }

    fn finalize(self) -> anyhow::Result<CasDigest> {
        let hash = Blake3::from_slice(self.hasher.finalize().as_bytes())?;
        Ok(CasDigest {
            hash,
            size: self.size,
        })
    }
}

impl Write for AugmentedTreeDigestCalculator {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(buf);
        self.size += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// The type for storing AugmentedTreeEntries together with its digest.
#[derive(Debug, Clone, PartialEq)]
pub struct AugmentedTreeWithDigest {
    pub augmented_manifest_id: Blake3,
    pub augmented_manifest_size: u64,
    pub augmented_tree: AugmentedTree,
}

impl AugmentedTreeWithDigest {
    pub fn serialized_tree_blob_size(&self) -> usize {
        self.augmented_tree.augmented_tree_blob_size()
            + Blake3::hex_len()
            + self.augmented_manifest_size.to_string().len()
            + 2
    }

    pub fn try_serialize(&self, mut w: impl Write) -> Result<()> {
        // Prepend the augmented manifest id and size header to the blob.
        w.write_all(self.augmented_manifest_id.to_hex().as_bytes())?;
        w.write_all(b" ")?;
        w.write_all(self.augmented_manifest_size.to_string().as_ref())?;
        w.write_all(b"\n")?;
        self.augmented_tree.try_serialize(&mut w)
    }

    pub fn try_deserialize(mut reader: impl BufRead) -> anyhow::Result<Self, Error> {
        let CasDigest { hash, size } = Self::try_deserialize_digest(&mut reader)?;
        let augmented_tree = AugmentedTree::try_deserialize(reader)?;
        anyhow::Ok(Self {
            augmented_manifest_id: hash,
            augmented_tree,
            augmented_manifest_size: size,
        })
    }

    /// Deserializes just the header of an AugmentedTreeWithDigest.
    pub fn try_deserialize_digest<R: BufRead>(reader: &mut R) -> anyhow::Result<CasDigest> {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let mut header = line.split(' ');
        let augmented_manifest_id = Blake3::from_hex(
            header
                .next()
                .ok_or(anyhow!("augmented tree: missing augmented_manifest_id"))?
                .trim()
                .as_ref(),
        )?;
        let augmented_manifest_size = header
            .next()
            .ok_or(anyhow!("augmented tree: missing augmented_manifest_size"))?
            .trim()
            .parse::<u64>()?;
        Ok(CasDigest {
            hash: augmented_manifest_id,
            size: augmented_manifest_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    #[test]
    fn test_augmented_manifest_parsing() {
        let mut reader = std::io::Cursor::new(concat!(
            "v1 1111111111111111111111111111111111111111 - 2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\n",
            "a.rs\x004444444444444444444444444444444444444444r 4444444444444444444444444444444444444444444444444444444444444444 10 4444444444444444444444444444444444444444 -\n",
            "b.rs\x002222222222222222222222222222222222222222r 2222222222222222222222222222222222222222222222222222222222222222 1000 2121212121212121212121212121212121212121 -\n",
            "dir_1\x003333333333333333333333333333333333333333t 3333333333333333333333333333333333333333333333333333333333333333 10\n",
            "dir_2\x001111111111111111111111111111111111111111t 1111111111111111111111111111111111111111111111111111111111111111 10000\n"
        ));
        let augmented_tree_entry =
            AugmentedTree::try_deserialize(&mut reader).expect("parsing failed");

        assert_eq!(augmented_tree_entry.entries.len(), 4);
        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        augmented_tree_entry
            .write_sapling_tree_blob(&mut buf)
            .expect("writing failed");
        assert_eq!(
            std::str::from_utf8(&buf),
            Ok(concat!(
                "a.rs\x004444444444444444444444444444444444444444\n",
                "b.rs\x002222222222222222222222222222222222222222\n",
                "dir_1\x003333333333333333333333333333333333333333t\n",
                "dir_2\x001111111111111111111111111111111111111111t\n"
            ))
        );
        // validate that the size calculation is correct
        assert_eq!(buf.len(), augmented_tree_entry.sapling_tree_blob_size());
    }

    #[test]
    fn test_augmented_manifest_parsing_2() {
        // test on a real example from the repo
        let mut reader = std::io::Cursor::new(concat!(
            "v1 2d6429cc6d9576d412493d30c700c58a4ac38fbe - 5d09d8b81f6c097d294cb081389428baa9ef96f4 -\n",
            "AssetWithZoneReclassifications.php\x002f09c0be8738b7256452133d790cc39f9da885b8r 8b2e323f74febd9dce4583c5af41b76d6cc79c8fc87b2400aa090df5af497a35 1137 7a207d1d8ae552303b111bd6030b074a673b918f -\n",
            "ZoneAssetReclassificationsAnnotation.php\x00a7006d0256d90b83b8e8834e3a8d74a57f669364r c3bb30c1b5462c56d178c457a43a30655c305780ae5b2b6fd5711a9288ddd5ae 2940 ba9327007f237bd7f2453ff02aadc1449ac483b9 -\n",
            "ZonePolicySetGenerator.php\x0032c4117a356ddd5a284dd55866e4c609e4002c99r eb0c0415ecb4c5461eda8cd52b0d8a5a4bad3ea8bad56b9ad5e6f78ded05de35 6569 b67a3f5383d979f780844812b947b77b4348e475 -\n",
            "__tests__\x009f0e8ffab4c1e1adfdea446d3c91b3c8ad525685t 8be9967f8ce1a6c8799f372acb81f57208a7eee78a6e60b6fa6426785fee31d6 248\n",
            "bounded_policies\x00e057d09012b275e5aa8f3d31ecb334ea7bd0e2dft 79d40367d5845d90c764b4febd8f4671d84645ca749156e11bdf4ed4683fdf3d 274\n",
            "config\x00ef0295d493a767db31bd2ad6e3c118a5ec2dc094t c1ca36f561ced429b3fdcbb46ea9959cd0db0f1d8c2a06a217aefcacdec53656 1139\n",
            "enforcement\x00519f107814932a6aaedf68e82a673710461c1a16t 7f275e452b69bdec740341663d123665d48d68ef20d3fd64bf66747c9cd291b3 1872\n",
            "integration\x0019ff1a891b88dad37743bb22fe29709b15ac195dt 1657e8938e7b48109879d1fcc649c80384e7dcec735bc7361dae86677f6b888f 415\n",
            "reclassifications\x00f1e2432047947bb339f2d6a2608eca729703bb65t a35546f72429461323d102a6ddfca129e8c7b74fd7cb9b76ec0efa9b4abf250d 649\n",
            "row_level_policy\x000238567d1c15525d8b2f2d366bd4aa306e20d8dft 3291f1c1ae09629423da93672b2fc52f009ab372a68d57844fef6d7c523d4527 937\n"
        ));
        let augmented_tree_entry =
            AugmentedTree::try_deserialize(&mut reader).expect("parsing failed");

        assert_eq!(augmented_tree_entry.entries.len(), 10);
        assert_eq!(
            augmented_tree_entry.hg_node_id,
            HgId::from_hex(b"2d6429cc6d9576d412493d30c700c58a4ac38fbe").unwrap()
        );
        assert_eq!(
            augmented_tree_entry.p1,
            Some(HgId::from_hex(b"5d09d8b81f6c097d294cb081389428baa9ef96f4").unwrap())
        );
        let first_child = augmented_tree_entry.entries.first().unwrap();
        assert_eq!(
            first_child.0,
            "AssetWithZoneReclassifications.php"
                .to_string()
                .try_into()
                .unwrap()
        );
        assert_matches!(first_child.1, AugmentedTreeEntry::FileNode(_));
        assert_matches!(
            augmented_tree_entry
                .entries
                .get(2)
                .expect("index 2 out of range")
                .1,
            AugmentedTreeEntry::FileNode(_)
        );
        assert_matches!(
            augmented_tree_entry
                .entries
                .get(3)
                .expect("index 3 out of range")
                .1,
            AugmentedTreeEntry::DirectoryNode(_)
        );
        assert_eq!(
            augmented_tree_entry
                .entries
                .get(3)
                .expect("index 3 out of range")
                .1,
            AugmentedTreeEntry::DirectoryNode(AugmentedDirectoryNode {
                treenode: HgId::from_hex(b"9f0e8ffab4c1e1adfdea446d3c91b3c8ad525685").unwrap(),
                augmented_manifest_id: Blake3::from_hex(
                    b"8be9967f8ce1a6c8799f372acb81f57208a7eee78a6e60b6fa6426785fee31d6"
                )
                .unwrap(),
                augmented_manifest_size: 248,
            })
        );

        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        augmented_tree_entry
            .write_sapling_tree_blob(&mut buf)
            .expect("writing failed");
        assert_eq!(
            std::str::from_utf8(&buf),
            Ok(concat!(
                "AssetWithZoneReclassifications.php\x002f09c0be8738b7256452133d790cc39f9da885b8\n",
                "ZoneAssetReclassificationsAnnotation.php\x00a7006d0256d90b83b8e8834e3a8d74a57f669364\n",
                "ZonePolicySetGenerator.php\x0032c4117a356ddd5a284dd55866e4c609e4002c99\n",
                "__tests__\x009f0e8ffab4c1e1adfdea446d3c91b3c8ad525685t\n",
                "bounded_policies\x00e057d09012b275e5aa8f3d31ecb334ea7bd0e2dft\n",
                "config\x00ef0295d493a767db31bd2ad6e3c118a5ec2dc094t\n",
                "enforcement\x00519f107814932a6aaedf68e82a673710461c1a16t\n",
                "integration\x0019ff1a891b88dad37743bb22fe29709b15ac195dt\n",
                "reclassifications\x00f1e2432047947bb339f2d6a2608eca729703bb65t\n",
                "row_level_policy\x000238567d1c15525d8b2f2d366bd4aa306e20d8dft\n"
            ))
        );
        assert_eq!(buf.len(), augmented_tree_entry.sapling_tree_blob_size());
    }

    #[test]
    fn test_augmented_manifest_parsing_roundtrips() {
        let tree = concat!(
            "v1 2d6429cc6d9576d412493d30c700c58a4ac38fbe - 5d09d8b81f6c097d294cb081389428baa9ef96f4 -\n",
            "AssetWithZoneReclassifications.php\x002f09c0be8738b7256452133d790cc39f9da885b8r 8b2e323f74febd9dce4583c5af41b76d6cc79c8fc87b2400aa090df5af497a35 1137 7a207d1d8ae552303b111bd6030b074a673b918f -\n",
            "ZoneAssetReclassificationsAnnotation.php\x00a7006d0256d90b83b8e8834e3a8d74a57f669364r c3bb30c1b5462c56d178c457a43a30655c305780ae5b2b6fd5711a9288ddd5ae 2940 ba9327007f237bd7f2453ff02aadc1449ac483b9 -\n",
            "ZonePolicySetGenerator.php\x0032c4117a356ddd5a284dd55866e4c609e4002c99r eb0c0415ecb4c5461eda8cd52b0d8a5a4bad3ea8bad56b9ad5e6f78ded05de35 6569 b67a3f5383d979f780844812b947b77b4348e475 -\n",
            "__tests__\x009f0e8ffab4c1e1adfdea446d3c91b3c8ad525685t 8be9967f8ce1a6c8799f372acb81f57208a7eee78a6e60b6fa6426785fee31d6 248\n",
            "bounded_policies\x00e057d09012b275e5aa8f3d31ecb334ea7bd0e2dft 79d40367d5845d90c764b4febd8f4671d84645ca749156e11bdf4ed4683fdf3d 274\n",
            "config\x00ef0295d493a767db31bd2ad6e3c118a5ec2dc094t c1ca36f561ced429b3fdcbb46ea9959cd0db0f1d8c2a06a217aefcacdec53656 1139\n",
            "enforcement\x00519f107814932a6aaedf68e82a673710461c1a16t 7f275e452b69bdec740341663d123665d48d68ef20d3fd64bf66747c9cd291b3 1872\n",
            "integration\x0019ff1a891b88dad37743bb22fe29709b15ac195dt 1657e8938e7b48109879d1fcc649c80384e7dcec735bc7361dae86677f6b888f 415\n",
            "reclassifications\x00f1e2432047947bb339f2d6a2608eca729703bb65t a35546f72429461323d102a6ddfca129e8c7b74fd7cb9b76ec0efa9b4abf250d 649\n",
            "row_level_policy\x000238567d1c15525d8b2f2d366bd4aa306e20d8dft 3291f1c1ae09629423da93672b2fc52f009ab372a68d57844fef6d7c523d4527 937\n"
        );
        let mut reader = std::io::Cursor::new(tree);
        let augmented_tree_entry =
            AugmentedTree::try_deserialize(&mut reader).expect("parsing failed");

        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        augmented_tree_entry
            .try_serialize(&mut buf)
            .expect("writing failed");

        assert_eq!(std::str::from_utf8(&buf), Ok(tree));
        assert_eq!(augmented_tree_entry.augmented_tree_blob_size(), tree.len());
    }

    #[test]
    fn test_augmented_manifest_parsing_computed_hg_node_id() {
        let mut reader = std::io::Cursor::new(concat!(
            "v1 1111111111111111111111111111111111111111 4444444444444444444444444444444444444444 2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\n",
            "a.rs\x004444444444444444444444444444444444444444r 4444444444444444444444444444444444444444444444444444444444444444 10 4444444444444444444444444444444444444444 -\n",
            "b.rs\x002222222222222222222222222222222222222222r 2222222222222222222222222222222222222222222222222222222222222222 1000 2121212121212121212121212121212121212121 -\n",
            "dir_1\x003333333333333333333333333333333333333333t 3333333333333333333333333333333333333333333333333333333333333333 10\n",
            "dir_2\x001111111111111111111111111111111111111111t 1111111111111111111111111111111111111111111111111111111111111111 10000\n"
        ));
        let augmented_tree_entry =
            AugmentedTree::try_deserialize(&mut reader).expect("parsing failed");

        assert_eq!(augmented_tree_entry.entries.len(), 4);
        assert_eq!(
            augmented_tree_entry.computed_hg_node_id,
            Some(HgId::from_hex(b"4444444444444444444444444444444444444444").expect("bad hgid"))
        );
        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        augmented_tree_entry
            .try_serialize(&mut buf)
            .expect("writing failed");
        assert_eq!(buf.len(), augmented_tree_entry.augmented_tree_blob_size());
    }

    #[test]
    fn test_augmented_manifest_parsing_file_header_metadata() {
        let mut reader = std::io::Cursor::new(concat!(
            "v1 1111111111111111111111111111111111111111 4444444444444444444444444444444444444444 2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\n",
            "a.rs\x004444444444444444444444444444444444444444r 4444444444444444444444444444444444444444444444444444444444444444 10 4444444444444444444444444444444444444444 AQpjb3B5OiBmYmNvZGUvZWRlbi9zY20vbGliL3JldmlzaW9uc3RvcmUvVEFSR0VUUwpjb3B5cmV2OiBhNDU5NTA0ZjY3NmE1ZmVjNWFiM2QxYTE0ZjQ2MTY0MzAzOTFjMDNlCgEK\n",
            "b.rs\x002222222222222222222222222222222222222222r 2222222222222222222222222222222222222222222222222222222222222222 1000 2121212121212121212121212121212121212121 -\n",
            "dir_1\x003333333333333333333333333333333333333333t 3333333333333333333333333333333333333333333333333333333333333333 10\n",
            "dir_2\x001111111111111111111111111111111111111111t 1111111111111111111111111111111111111111111111111111111111111111 10000\n"
        ));
        let augmented_tree_entry =
            AugmentedTree::try_deserialize(&mut reader).expect("parsing failed");

        assert_eq!(augmented_tree_entry.entries.len(), 4);

        assert_eq!(
            augmented_tree_entry.entries.first().unwrap().1,
            AugmentedTreeEntry::FileNode(AugmentedFileNode {
                file_type: FileType::Regular,
                filenode: HgId::from_hex(b"4444444444444444444444444444444444444444")
                    .expect("bad hgid"),
                content_blake3: Blake3::from_hex(
                    b"4444444444444444444444444444444444444444444444444444444444444444"
                )
                .expect("bad blake3"),
                content_sha1: Sha1::from_hex(b"4444444444444444444444444444444444444444")
                    .expect("bad id20"),
                total_size: 10,
                file_header_metadata: Some(Bytes::from(
                    "\x01\ncopy: fbcode/eden/scm/lib/revisionstore/TARGETS\ncopyrev: a459504f676a5fec5ab3d1a14f4616430391c03e\n\x01\n"
                ))
            })
        );

        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        augmented_tree_entry
            .try_serialize(&mut buf)
            .expect("writing failed");
        assert_eq!(buf.len(), augmented_tree_entry.augmented_tree_blob_size());
    }

    #[test]
    fn test_augmented_tree_digest_calculation() {
        let mut reader = std::io::Cursor::new(concat!(
            "v1 1111111111111111111111111111111111111111 - 2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\n",
            "a.rs\x004444444444444444444444444444444444444444r 4444444444444444444444444444444444444444444444444444444444444444 10 4444444444444444444444444444444444444444 -\n",
            "b.rs\x002222222222222222222222222222222222222222r 2222222222222222222222222222222222222222222222222222222222222222 1000 2121212121212121212121212121212121212121 -\n",
            "dir_1\x003333333333333333333333333333333333333333t 3333333333333333333333333333333333333333333333333333333333333333 10\n",
            "dir_2\x001111111111111111111111111111111111111111t 1111111111111111111111111111111111111111111111111111111111111111 10000\n"
        ));
        let augmented_tree_entry =
            AugmentedTree::try_deserialize(&mut reader).expect("parsing failed");
        let CasDigest { hash, size } = augmented_tree_entry
            .compute_content_addressed_digest()
            .expect("digest calculation failed");
        assert_eq!(
            (hash.to_hex().as_str(), size),
            (
                "163e6a8b60b1f0c7042adbdfcc932f11ff3de5003905ab4c5d564431c31e6f32",
                681
            )
        );
    }

    #[test]
    fn test_augmented_tree_with_digest_parsing_roundtrip() {
        let mut reader = std::io::Cursor::new(concat!(
            "v1 1111111111111111111111111111111111111111 - 2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\n",
            "a.rs\x004444444444444444444444444444444444444444r 4444444444444444444444444444444444444444444444444444444444444444 10 4444444444444444444444444444444444444444 -\n",
            "b.rs\x002222222222222222222222222222222222222222r 2222222222222222222222222222222222222222222222222222222222222222 1000 2121212121212121212121212121212121212121 -\n",
            "dir_1\x003333333333333333333333333333333333333333t 3333333333333333333333333333333333333333333333333333333333333333 10\n",
            "dir_2\x001111111111111111111111111111111111111111t 1111111111111111111111111111111111111111111111111111111111111111 10000\n"
        ));

        // Parse initial augmented tree entry.
        let augmented_tree_entry =
            AugmentedTree::try_deserialize(&mut reader).expect("parsing failed");

        // Calculate digest
        let CasDigest { hash, size } = augmented_tree_entry
            .compute_content_addressed_digest()
            .expect("digest calculation failed");

        // Create augmented tree entry with digest.
        let augmented_tree_with_digest = AugmentedTreeWithDigest {
            augmented_manifest_id: hash,
            augmented_manifest_size: size,
            augmented_tree: augmented_tree_entry,
        };

        // Serialize and deserialize, check the stucts are identical.
        let mut buf: Vec<u8> = Vec::with_capacity(1024);
        augmented_tree_with_digest
            .try_serialize(&mut buf)
            .expect("writing failed");

        // Check the size of serialized tree blob is correct.
        assert_eq!(
            buf.len(),
            augmented_tree_with_digest.serialized_tree_blob_size()
        );

        let reader1 = std::io::Cursor::new(buf);
        let augmented_tree_with_digest2 =
            AugmentedTreeWithDigest::try_deserialize(reader1).expect("parsing failed");
        assert_eq!(augmented_tree_with_digest, augmented_tree_with_digest2);
    }

    #[test]
    fn test_augmented_tree_file_types() {
        let serialized_manifest = concat!(
            "v1 1111111111111111111111111111111111111111 - 2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\n",
            "bin\x004444444444444444444444444444444444444444x 4444444444444444444444444444444444444444444444444444444444444444 10 4444444444444444444444444444444444444444 -\n",
            "link\x002222222222222222222222222222222222222222l 2222222222222222222222222222222222222222222222222222222222222222 1000 2121212121212121212121212121212121212121 -\n",
        );

        // Parse initial augmented tree entry.
        let augmented_tree =
            AugmentedTree::try_deserialize(serialized_manifest.as_bytes()).expect("parsing failed");

        assert_eq!(
            augmented_tree.entries,
            vec![
                (
                    "bin".to_string().try_into().unwrap(),
                    AugmentedTreeEntry::FileNode(AugmentedFileNode {
                        file_type: FileType::Executable,
                        filenode: HgId::from_hex(b"4444444444444444444444444444444444444444")
                            .expect("bad hgid"),
                        content_blake3: Blake3::from_hex(
                            b"4444444444444444444444444444444444444444444444444444444444444444"
                        )
                        .expect("bad blake3"),
                        content_sha1: Sha1::from_hex(b"4444444444444444444444444444444444444444")
                            .expect("bad id20"),
                        total_size: 10,
                        file_header_metadata: None,
                    })
                ),
                (
                    "link".to_string().try_into().unwrap(),
                    AugmentedTreeEntry::FileNode(AugmentedFileNode {
                        file_type: FileType::Symlink,
                        filenode: HgId::from_hex(b"2222222222222222222222222222222222222222")
                            .expect("bad hgid"),
                        content_blake3: Blake3::from_hex(
                            b"2222222222222222222222222222222222222222222222222222222222222222"
                        )
                        .expect("bad blake3"),
                        content_sha1: Sha1::from_hex(b"2121212121212121212121212121212121212121")
                            .expect("bad id20"),
                        total_size: 1000,
                        file_header_metadata: None,
                    })
                ),
            ],
        );

        let mut buf: Vec<u8> = Vec::new();
        augmented_tree
            .try_serialize(&mut buf)
            .expect("writing failed");

        assert_eq!(&buf, serialized_manifest.as_bytes());
    }

    #[test]
    fn test_filename_with_space() {
        let serialized_manifest = concat!(
            "v1 1111111111111111111111111111111111111111 - 2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\n",
            "hi there\x004444444444444444444444444444444444444444r 4444444444444444444444444444444444444444444444444444444444444444 10 4444444444444444444444444444444444444444 -\n",
        );

        // Parse initial augmented tree entry.
        let augmented_tree =
            AugmentedTree::try_deserialize(serialized_manifest.as_bytes()).expect("parsing failed");

        assert_eq!(
            augmented_tree.entries,
            vec![(
                "hi there".to_string().try_into().unwrap(),
                AugmentedTreeEntry::FileNode(AugmentedFileNode {
                    file_type: FileType::Regular,
                    filenode: HgId::from_hex(b"4444444444444444444444444444444444444444")
                        .expect("bad hgid"),
                    content_blake3: Blake3::from_hex(
                        b"4444444444444444444444444444444444444444444444444444444444444444"
                    )
                    .expect("bad blake3"),
                    content_sha1: Sha1::from_hex(b"4444444444444444444444444444444444444444")
                        .expect("bad id20"),
                    total_size: 10,
                    file_header_metadata: None,
                })
            ),],
        );

        let mut buf: Vec<u8> = Vec::new();
        augmented_tree
            .try_serialize(&mut buf)
            .expect("writing failed");

        assert_eq!(&buf, serialized_manifest.as_bytes());
    }
}
