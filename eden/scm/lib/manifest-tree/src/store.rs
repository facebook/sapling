/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::from_utf8;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use bytes::Bytes;
use bytes::BytesMut;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::FsNodeMetadata;
use storemodel::TreeFormat;
pub use storemodel::TreeStore;
use types::HgId;
use types::Key;
use types::PathComponent;
use types::PathComponentBuf;
use types::RepoPath;

#[derive(Clone)]
pub struct InnerStore {
    tree_store: Arc<dyn TreeStore + Send + Sync>,
}

impl InnerStore {
    pub fn new(tree_store: Arc<dyn TreeStore + Send + Sync>) -> Self {
        InnerStore { tree_store }
    }

    pub fn format(&self) -> TreeFormat {
        self.tree_store.format()
    }

    pub fn get_entry(&self, path: &RepoPath, hgid: HgId) -> Result<Entry> {
        tracing::debug_span!(
            "tree::store::get",
            id = AsRef::<str>::as_ref(&hgid.to_hex())
        )
        .in_scope(|| {
            let bytes = self.tree_store.get(path, hgid)?;
            Ok(Entry(bytes))
        })
    }

    pub fn insert_entry(&self, path: &RepoPath, hgid: HgId, entry: Entry) -> Result<()> {
        tracing::debug_span!(
            "tree::store::insert",
            path = path.as_str(),
            id = AsRef::<str>::as_ref(&hgid.to_hex())
        )
        .in_scope(|| self.tree_store.insert(path, hgid, entry.0))
    }

    pub fn prefetch(&self, keys: impl IntoIterator<Item = Key>) -> Result<()> {
        let keys: Vec<Key> = keys.into_iter().collect();
        tracing::debug_span!(
            "tree::store::prefetch",
            ids = {
                let ids: Vec<String> = keys.iter().map(|k| k.hgid.to_hex()).collect();
                &AsRef::<str>::as_ref(&ids.join(" "))
            }
        )
        .in_scope(|| self.tree_store.prefetch(keys))
    }
}

/// The `Entry` is the data that is stored on disk. It should be seen as opaque to whether it
/// represents serialized or deserialized data. It is the object that performs the bytes to
/// business object transformations. It provides method for interacting with parsed data.
///
/// The ABNF specification for the current serialization is:
/// Entry         = 1*( Element LF )
/// Element       = PathComponent %x00 HgId [ Flag ]
/// Flag          = %s"x" / %s"l" / %s"t"
/// PathComponent = 1*( %x01-%x09 / %x0B-%xFF )
/// HgId          = 40HEXDIG
///
/// In this case an `Entry` is equivalent to the contents of a directory. The elements of the
/// directory are described by `Element`. `Entry` is a list of serialized `Element`s that are
/// separated by `\n`. An `Element` will always have a name (`PathComponent`) and a hash (`HgId`).
/// `Elements` may be different types of files or they can be directories. The type of element is
/// described by the flag or the absence of the flag. When the flag is missing we have a regular
/// file, the various flag options are: `x` for executable, `l` for symlink and `d` for directory.
/// It should be noted that Nodes are represented in their hex format rather than a straight up
/// binary format so they are 40 characters long rather than 20 bytes.
/// Check the documentation of the `PathComponent` struct for more details about it's
/// representation. For this serialization format it is important that they don't contain
/// `\0` or `\n`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Entry(pub Bytes);

pub struct EntryMut(BytesMut);

/// The `Element` is a parsed element of a directory. Directory elements are either files either
/// direcotries. The type of element is signaled by `Flag`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Element {
    pub component: PathComponentBuf,
    pub hgid: HgId,
    pub flag: Flag,
}

/// Used to signal the type of element in a directory: file or directory.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Flag {
    File(FileType),
    Directory,
}

impl Entry {
    /// Returns an iterator over the elements that the current `Entry` contains. This is the
    /// primary method of inspection for an `Entry`.
    pub fn elements<'a>(&'a self) -> Elements<'a> {
        Elements::from_byte_slice(&self.0)
    }

    /// The primary builder of an Entry, from a list of `Element`.
    pub fn from_elements<I: IntoIterator<Item = Result<Element>>>(elements: I) -> Result<Entry> {
        let mut underlying = BytesMut::new();
        for element_result in elements.into_iter() {
            underlying.extend(element_result?.to_byte_vec());
            underlying.extend(b"\n");
        }
        Ok(Entry(underlying.freeze()))
    }

    // used in tests, finalize and subtree_diff
    pub fn to_bytes(self) -> Bytes {
        self.0
    }
}

impl EntryMut {
    /// Constructs an empty `Entry`. It is not valid to save an empty `Entry`.
    pub fn new() -> Self {
        EntryMut(BytesMut::new())
    }

    /// Adds an element to the list of elements represented by this `Entry`.
    /// It is expected that elements are added sorted by paths.
    pub fn add_element(&mut self, element: Element) {
        self.0.extend(element.to_byte_vec());
        self.0.extend(b"\n");
    }

    pub fn freeze(self) -> Entry {
        Entry(self.0.freeze())
    }
}

impl AsRef<[u8]> for Entry {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub struct Elements<'a> {
    byte_slice: &'a [u8],
    position: usize,
    format: TreeFormat,
}

impl<'a> Elements<'a> {
    /// Constructs `Elements` from raw byte slice.
    pub fn from_byte_slice(byte_slice: &'a [u8]) -> Self {
        // hg: first 20 bytes are a hex SHA1, no spaces
        // git: mode + space. The space is the 6th or 7th byte.
        let format = if byte_slice.get(b"40000".len()) == Some(&b' ')
            || byte_slice.get(b"100644".len()) == Some(&b' ')
        {
            TreeFormat::Git
        } else {
            TreeFormat::Hg
        };
        Elements {
            byte_slice,
            position: 0,
            format,
        }
    }

    fn next_hg(&mut self) -> Option<Result<Element>> {
        if self.position >= self.byte_slice.len() {
            return None;
        }
        let end = match self.byte_slice[self.position..]
            .iter()
            .position(|&byte| byte == b'\n')
        {
            None => {
                return Some(Err(format_err!(
                    "failed to deserialize tree manifest entry, missing line feed\n{:?}",
                    String::from_utf8_lossy(self.byte_slice)
                )));
            }
            Some(delta) => self.position + delta,
        };
        let result = Element::from_byte_slice(&self.byte_slice[self.position..end]);
        self.position = end + 1;
        Some(result)
    }

    fn next_git(&mut self) -> Option<Result<Element>> {
        let slice = match self.byte_slice.get(self.position..) {
            None => return None,
            Some(s) if s.is_empty() => return None,
            Some(s) => s,
        };

        // MODE ' '       NAME          '\0'                     BIN_SHA1
        //      ^         ^             ^                        ^
        //      mode_len  mode_len + 1  mode_len + 1 + name_len  .. + 1
        let mode_len = match slice.iter().position(|&x| x == b' ') {
            Some(position) => position,
            None => return Some(Err(format_err!("did not find mode delimiter"))),
        };
        let name_len = match slice.iter().skip(mode_len + 1).position(|&x| x == b'\0') {
            Some(position) => position,
            None => return Some(Err(format_err!("did not find name delimiter"))),
        };

        let flag = match &slice[..mode_len] {
            b"40000" => Flag::Directory,
            b"100644" => Flag::File(FileType::Regular),
            b"100755" => Flag::File(FileType::Executable),
            b"120000" => Flag::File(FileType::Symlink),
            s => {
                return Some(Err(format_err!(
                    "unknown or unsupport mode in git tree ({})",
                    String::from_utf8_lossy(s)
                )));
            }
        };

        let mut offset = mode_len + 1;
        let name = &slice[offset..offset + name_len];
        let component = match PathComponent::from_utf8(name) {
            Ok(p) => p.to_owned(),
            Err(e) => return Some(Err(e.into())),
        };

        offset += name_len + 1;
        let hgid = if let Some(id_slice) = slice.get(offset..offset + HgId::len()) {
            HgId::from_slice(id_slice).expect("id_slice has the right length")
        } else {
            return Some(Err(format_err!("SHA1 is incomplete")));
        };

        offset += HgId::len();
        self.position += offset;

        let element = Element {
            component,
            hgid,
            flag,
        };
        Some(Ok(element))
    }
}

impl<'a> Iterator for Elements<'a> {
    type Item = Result<Element>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.format {
            TreeFormat::Hg => self.next_hg(),
            TreeFormat::Git => self.next_git(),
        }
    }
}

impl TryFrom<Entry> for manifest::List {
    type Error = anyhow::Error;

    fn try_from(v: Entry) -> Result<Self> {
        let mut entries = Vec::new();
        for entry in v.elements() {
            let entry = entry?;
            entries.push((
                entry.component,
                match entry.flag {
                    Flag::Directory => FsNodeMetadata::Directory(Some(entry.hgid)),
                    Flag::File(file_type) => {
                        FsNodeMetadata::File(FileMetadata::new(entry.hgid, file_type))
                    }
                },
            ))
        }
        Ok(manifest::List::Directory(entries))
    }
}

impl Element {
    pub fn new(component: PathComponentBuf, hgid: HgId, flag: Flag) -> Element {
        Element {
            component,
            hgid,
            flag,
        }
    }

    fn from_byte_slice(byte_slice: &[u8]) -> Result<Element> {
        let path_len = match byte_slice.iter().position(|&x| x == b'\0') {
            Some(position) => position,
            None => return Err(format_err!("did not find path delimiter")),
        };
        let component = PathComponent::from_utf8(&byte_slice[..path_len])?.to_owned();
        if path_len + HgId::hex_len() > byte_slice.len() {
            return Err(format_err!("hgid length is shorter than expected"));
        }
        if byte_slice.len() > path_len + HgId::hex_len() + 2 {
            return Err(format_err!("entry longer than expected"));
        }
        // TODO: We don't need this conversion to string
        let utf8_parsed = from_utf8(&byte_slice[path_len + 1..path_len + HgId::hex_len() + 1])?;
        let hgid = HgId::from_str(utf8_parsed)?;
        let flag = match byte_slice.get(path_len + HgId::hex_len() + 1) {
            None => Flag::File(FileType::Regular),
            Some(b'x') => Flag::File(FileType::Executable),
            Some(b'l') => Flag::File(FileType::Symlink),
            Some(b't') => Flag::Directory,
            Some(bad_flag) => return Err(format_err!("invalid flag {}", bad_flag)),
        };
        let element = Element {
            component,
            hgid,
            flag,
        };
        Ok(element)
    }

    fn to_byte_vec(&self) -> Vec<u8> {
        let component = self.component.as_byte_slice();
        // TODO: benchmark taking a buffer as a parameter
        // We may not use the last byte but it doesn't hurt to allocate
        let mut buffer = Vec::with_capacity(component.len() + HgId::hex_len() + 2);
        buffer.extend_from_slice(component);
        buffer.push(0);
        buffer.extend_from_slice(self.hgid.to_hex().as_ref());
        let flag = match self.flag {
            Flag::File(FileType::Regular) => None,
            Flag::File(FileType::Executable) => Some(b'x'),
            Flag::File(FileType::Symlink) => Some(b'l'),
            Flag::Directory => Some(b't'),
        };
        if let Some(byte) = flag {
            buffer.push(byte);
        }
        buffer
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;
    use types::testutil::*;

    use super::*;

    #[test]
    fn test_element_from_byte_slice() {
        let mut buffer = vec![];
        let path = PathComponent::from_str("foo").unwrap();
        let hgid = hgid("123");
        assert!(Element::from_byte_slice(&buffer).is_err());
        buffer.extend_from_slice(path.as_byte_slice());
        assert!(Element::from_byte_slice(&buffer).is_err());
        buffer.push(b'\0');
        assert!(Element::from_byte_slice(&buffer).is_err());
        buffer.extend_from_slice(hgid.to_hex().as_ref());
        assert_eq!(
            Element::from_byte_slice(&buffer).unwrap(),
            Element::new(path.to_owned(), hgid, Flag::File(FileType::Regular))
        );

        buffer.push(b'x');
        assert_eq!(
            Element::from_byte_slice(&buffer).unwrap(),
            Element::new(path.to_owned(), hgid, Flag::File(FileType::Executable))
        );

        *buffer.last_mut().unwrap() = b'l';
        assert_eq!(
            Element::from_byte_slice(&buffer).unwrap(),
            Element::new(path.to_owned(), hgid, Flag::File(FileType::Symlink))
        );

        *buffer.last_mut().unwrap() = b't';
        assert_eq!(
            Element::from_byte_slice(&buffer).unwrap(),
            Element::new(path.to_owned(), hgid, Flag::Directory)
        );

        *buffer.last_mut().unwrap() = b's';
        assert!(Element::from_byte_slice(&buffer).is_err());

        *buffer.last_mut().unwrap() = b'x';
        buffer.push(b'\0');
        assert!(Element::from_byte_slice(&buffer).is_err());
    }

    #[test]
    fn test_deserialize_git_tree() {
        // Tree data generated by:
        //
        // mkdir -p foo && cd foo
        // ( git init; touch normal; mkdir dir; touch dir/1; ln -s normal symlink; touch exe; chmod +x exe; git add .; git commit -m c; ) >/dev/null
        // TREE=$(git cat-file -p HEAD | grep tree | sed 's/.* //')
        // git cat-file tree $TREE
        let data = b"40000 dir\x00\x8d\xc8w\xa9\x98\xd8\xc6\x1f\x90\x0e\x8bN\xe9\xb5\x01\xfa\n\x03\x93X100755 exe\x00\xe6\x9d\xe2\x9b\xb2\xd1\xd6CK\x8b)\xaewZ\xd8\xc2\xe4\x8cS\x91100644 normal\x00\xe6\x9d\xe2\x9b\xb2\xd1\xd6CK\x8b)\xaewZ\xd8\xc2\xe4\x8cS\x91120000 symlink\x00Z\xe04cN\x8d8,\x86F\x06\x8b\x8bR8\x18\x15\xed\xab\xf0";
        let entry = Entry(Bytes::copy_from_slice(data));
        let elements = entry.elements();
        let elements_str = elements
            .map(|e| format!("{:?}", e.unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            elements_str,
            [
                "Element { component: PathComponentBuf(\"dir\"), hgid: HgId(\"8dc877a998d8c61f900e8b4ee9b501fa0a039358\"), flag: Directory }",
                "Element { component: PathComponentBuf(\"exe\"), hgid: HgId(\"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\"), flag: File(Executable) }",
                "Element { component: PathComponentBuf(\"normal\"), hgid: HgId(\"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\"), flag: File(Regular) }",
                "Element { component: PathComponentBuf(\"symlink\"), hgid: HgId(\"5ae034634e8d382c8646068b8b52381815edabf0\"), flag: File(Symlink) }",
            ]
        );
    }

    #[test]
    fn test_roundtrip_serialization_on_directory() {
        let component = PathComponentBuf::from_string(String::from("c")).unwrap();
        let hgid = HgId::from_str("2e31d52f551e445002a6e6690700ce2ac31f196e").unwrap();
        let flag = Flag::Directory;
        let byte_slice = b"c\02e31d52f551e445002a6e6690700ce2ac31f196et";
        let element = Element::new(component, hgid, flag);
        assert_eq!(Element::from_byte_slice(byte_slice).unwrap(), element);
        let buffer = element.to_byte_vec();
        assert_eq!(buffer.to_vec(), byte_slice.to_vec());
    }

    quickcheck! {
        fn test_rountrip_serialization(
            component: PathComponentBuf,
            hgid: HgId,
            flag_proxy: Option<FileType>
        ) -> bool {
            let flag = match flag_proxy {
                Some(file_type) => Flag::File(file_type),
                None => Flag::Directory,
            };
            let element = Element::new(component, hgid, flag);
            let buffer = element.to_byte_vec();
            Element::from_byte_slice(&buffer).unwrap() == element
        }
    }
}
