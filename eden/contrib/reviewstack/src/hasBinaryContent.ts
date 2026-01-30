/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Checks if the content appears to be binary by looking for common binary file
 * signatures (magic bytes) or null bytes in the content.
 *
 * This is used as an additional safety check beyond the `isBinary` flag from
 * the GitHub API to ensure we don't attempt to render binary content.
 */
export default function hasBinaryContent(content: string | null | undefined): boolean {
  if (content == null || content.length === 0) {
    return false;
  }

  // Check for null bytes in the first portion of the file
  // This is a common heuristic for detecting binary files
  const checkLength = Math.min(8192, content.length);
  for (let i = 0; i < checkLength; i++) {
    if (content.charCodeAt(i) === 0) {
      return true;
    }
  }

  // Check for common binary file signatures (magic bytes)
  // These are the starting bytes of common binary file formats
  const binarySignatures = [
    '\x89PNG\r\n\x1a\n', // PNG
    '\xff\xd8\xff', // JPEG
    'GIF87a', // GIF87
    'GIF89a', // GIF89
    '%PDF', // PDF
    'PK\x03\x04', // ZIP/DOCX/XLSX/etc.
    'PK\x05\x06', // ZIP empty archive
    '\x1f\x8b', // GZIP
    '\x7fELF', // ELF executable
    '\xca\xfe\xba\xbe', // Mach-O binary (universal)
    '\xfe\xed\xfa\xce', // Mach-O binary (32-bit)
    '\xfe\xed\xfa\xcf', // Mach-O binary (64-bit)
    '\xce\xfa\xed\xfe', // Mach-O binary (32-bit, reverse)
    '\xcf\xfa\xed\xfe', // Mach-O binary (64-bit, reverse)
    'MZ', // DOS/Windows executable
    '\x00\x00\x00', // Various binary formats starting with null bytes
    'RIFF', // WAV, AVI, WebP
    'OggS', // OGG audio
    'fLaC', // FLAC audio
    'ID3', // MP3 with ID3 tag
    '\xff\xfb', // MP3 frame sync
    '\xff\xfa', // MP3 frame sync
    '\xff\xf3', // MP3 frame sync
    '\xff\xf2', // MP3 frame sync
    'BM', // BMP image
    '\x00\x00\x01\x00', // ICO
    'wOFF', // WOFF font
    'wOF2', // WOFF2 font
  ];

  for (const sig of binarySignatures) {
    if (content.startsWith(sig)) {
      return true;
    }
  }

  return false;
}
