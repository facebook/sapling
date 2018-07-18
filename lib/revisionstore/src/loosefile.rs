// Copyright Facebook, Inc. 2018
// LooseFile class can parse loose file format written by
// hg/hgext/remotefilelog/remotefilelog.py:_createfileblob()
// into the size of text, text and ancestors information.

use std::fs::File;
use std::io::prelude::*;
use std::rc::Rc;

use error::{KeyError, Result};
use historystore::{Ancestors, NodeInfo};
use key::Key;
use node::Node;

#[derive(Debug, Fail)]
#[fail(display = "LooseFile Error: {:?}", _0)]
struct LooseFileError(String);

impl From<LooseFileError> for KeyError {
    fn from(err: LooseFileError) -> Self {
        KeyError::new(err.into())
    }
}

pub struct LooseFile {
    pub size: usize,
    pub text: Rc<Box<[u8]>>,
    pub ancestors: Ancestors,
}

fn read_file(spath: &str) -> Result<Vec<u8>> {
    let mut file = File::open(&spath)?;
    let mut file_content = Vec::new();
    file.read_to_end(&mut file_content)?;
    Ok(file_content)
}

fn atoi(content: &[u8]) -> Result<(usize, usize)> {
    let mut ret = 0;
    for i in 0..content.len() {
        let si = content[i] as char;
        if !si.is_digit(10) {
            if content[i] != 0 {
                break;
            }
            return Ok((ret as usize, i));
        }
        ret = ret * 10 + (content[i] as usize - '0' as usize);
    }
    Err(LooseFileError(format!("atoi fail to parse value")).into())
}

impl LooseFile {
    pub fn new(size: usize, text: Rc<Box<[u8]>>, ancestors: Ancestors) -> Self {
        LooseFile {
            size,
            text,
            ancestors,
        }
    }

    pub fn from_content(content: &Vec<u8>) -> Result<Self> {
        let (textsize, size) = atoi(&content)?;
        let mut start = size + 1;
        let text = content[start..start + textsize].to_vec();
        start += textsize;
        let mut ancestors: Ancestors = Ancestors::new();
        while start + 80 < content.len() {
            let node: Node = Node::from_slice(&content[start..start + 20])?;
            let key: Key = Key::new(Box::new([5u8; 3]), node);
            let p0: Node = Node::from_slice(&content[start + 20..start + 40])?;
            let k0: Key = Key::new(Box::new([0u8; 3]), p0);
            let p1: Node = Node::from_slice(&content[start + 40..start + 60])?;
            let k1: Key = Key::new(Box::new([1u8; 3]), p1);
            let parents: [Key; 2] = [k0, k1];
            let linknode: Node = Node::from_slice(&content[start + 60..start + 80])?;
            let nodeinfo = NodeInfo { parents, linknode };
            ancestors.insert(key, nodeinfo);
            start += 80;
            while start < content.len() && content[start] != 0 {
                start += 1;
            }
            start += 1;
        }
        Ok(LooseFile::new(
            textsize,
            Rc::new(text.into_boxed_slice()),
            ancestors,
        ))
    }

    pub fn from_file(spath: &str) -> Result<Self> {
        let content = read_file(spath)?;
        LooseFile::from_content(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::chacha::ChaChaRng;

    quickcheck! {
        fn test_roundtrip_atoi(value: u32) -> bool {
            let value_str = format!("{}\0", value);
            let (textsize, size) = match atoi(&value_str.as_bytes()) {
                Ok(res) => res,
                Err(_e) => panic!("atoi test with valid input should not give error"),
            };
            // +1 due to the extra \0
            (textsize, size + 1) == (value as usize, value_str.len())
        }
    }

    #[test]
    fn test_atoi_invalid() {
        let mut content: Vec<u8> = Vec::new();
        content.push(49); // 49='1'
        content.push(50); // 50='2'
        content.push(30); // not digit but not 0 also
        match atoi(&content) {
            Ok(_res) => assert!(false),
            Err(_e) => return,
        };
    }

    #[test]
    fn test_from_content() {
        let mut content: Vec<u8> = Vec::new();
        content.push(49); // 49='1'
        content.push(50); // 50='2'
        content.push(0);
        for i in 97..109 {
            // text='abcdefghijkl'
            content.push(i);
        }

        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let anode = Node::random(&mut rng);
        let bnode = Node::random(&mut rng);
        let ap0 = Node::random(&mut rng);
        let bp0 = Node::random(&mut rng);
        let ap1 = Node::random(&mut rng);
        let bp1 = Node::random(&mut rng);
        let alinknode = Node::random(&mut rng);
        let blinknode = Node::random(&mut rng);
        content.extend_from_slice(anode.as_ref());
        content.extend_from_slice(ap0.as_ref());
        content.extend_from_slice(ap1.as_ref());
        content.extend_from_slice(alinknode.as_ref());
        for i in 65..70 {
            // ABCDE
            content.push(i);
        }
        content.push(0);
        content.extend_from_slice(bnode.as_ref());
        content.extend_from_slice(bp0.as_ref());
        content.extend_from_slice(bp1.as_ref());
        content.extend_from_slice(blinknode.as_ref());
        for i in 65..71 {
            // ABCDEF
            content.push(i);
        }
        content.push(0);
        let loose_file = match LooseFile::from_content(&content) {
            Ok(res) => res,
            Err(_e) => panic!("whatever"),
        };
        assert_eq!(loose_file.size, 12, "text size supposed to be 12");
        assert_eq!(loose_file.text.len(), 12, "Actual text size should be 12");
        for i in 97..109 {
            assert_eq!(
                loose_file.text[i - 97],
                i as u8,
                "text content does not match"
            );
        }
        for (key, nodeinfo) in &loose_file.ancestors {
            if key.node() == &anode {
                assert_eq!(nodeinfo.parents[0].node(), &ap0);
                assert_eq!(nodeinfo.parents[1].node(), &ap1);
                assert_eq!(nodeinfo.linknode, alinknode);
            } else {
                assert_eq!(key.node(), &bnode);
                assert_eq!(nodeinfo.parents[0].node(), &bp0);
                assert_eq!(nodeinfo.parents[1].node(), &bp1);
                assert_eq!(nodeinfo.linknode, blinknode);
            }
        }
    }
}
