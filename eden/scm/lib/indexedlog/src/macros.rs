/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Implement traits for typed offset structs.
macro_rules! impl_offset {
    ($type:ident, $type_int:expr, $name:expr) => {
        impl TypedOffsetMethods for $type {
            #[inline]
            fn type_int() -> u8 {
                $type_int
            }

            #[inline]
            fn from_offset_unchecked(offset: Offset) -> Self {
                $type(offset)
            }

            #[inline]
            fn to_offset(&self) -> Offset {
                self.0
            }
        }

        impl Deref for $type {
            type Target = Offset;

            #[inline]
            fn deref(&self) -> &Offset {
                &self.0
            }
        }

        impl Debug for $type {
            fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
                if self.is_null() {
                    write!(f, "None")
                } else {
                    if self.is_dirty() {
                        write!(f, "{}[{}]", $name, self.dirty_index())
                    } else {
                        // `Offset` will print "Disk[{}]".
                        self.0.fmt(f)
                    }
                }
            }
        }

        impl From<$type> for Offset {
            #[inline]
            fn from(x: $type) -> Offset {
                x.0
            }
        }

        impl From<$type> for u64 {
            #[inline]
            fn from(x: $type) -> u64 {
                (x.0).0
            }
        }

        impl From<$type> for usize {
            #[inline]
            fn from(x: $type) -> usize {
                (x.0).0 as usize
            }
        }
    };
}
