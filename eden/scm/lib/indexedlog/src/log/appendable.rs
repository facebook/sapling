/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use minibytes::Bytes;

use crate::log::ExtendWrite;

/// Abstraction for data that can be appended to a `Log`.
pub trait Appendable {
    fn write_to(&self, buf: &mut dyn ExtendWrite) -> crate::Result<()>;

    fn data_len(&self) -> Option<usize> {
        None
    }
}

macro_rules! impl_appendable {
    ($(
        [$($generics:tt)*] $ty:ty => |$this:ident| $bytes:expr, $len:expr
    );+ $(;)?) => {
        $(
            impl<$($generics)*> Appendable for $ty {
                fn write_to(&self, buf: &mut dyn ExtendWrite) -> crate::Result<()> {
                    #[allow(unused)]
                    let $this = self;
                    buf.extend_from_slice($bytes);
                    Ok(())
                }

                fn data_len(&self) -> Option<usize> {
                    #[allow(unused)]
                    let $this = self;
                    Some($len)
                }
            }
        )+
    };
}

impl_appendable! {
    [] Vec<u8> => |this| this, this.len();
    [] &Vec<u8> => |this| this, this.len();
    [] Bytes => |this| this, this.len();
    [] &Bytes => |this| this, this.len();
    [] [u8] => |this| this, this.len();
    [] &[u8] => |this| this, this.len();
    [] &&[u8] => |this| this, this.len();
    [] str => |this| this.as_bytes(), this.len();
    [] &str => |this| this.as_bytes(), this.len();
    [] &&str => |this| this.as_bytes(), this.len();
    [const N: usize] [u8; N] => |this| this.as_slice(), N;
    [const N: usize] &[u8; N] => |this| this.as_slice(), N;
}

impl<F, E> Appendable for F
where
    F: Fn(&mut dyn ExtendWrite) -> Result<(), E>,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    fn write_to(&self, buf: &mut dyn ExtendWrite) -> crate::Result<()> {
        (self)(buf).map_err(|e| {
            crate::Error::blank()
                .message("append callback error")
                .source_dyn(e.into())
        })
    }
}
