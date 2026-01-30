/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import hasBinaryContent from './hasBinaryContent';

describe('hasBinaryContent', () => {
  describe('null and empty content handling', () => {
    test('returns false for null content', () => {
      expect(hasBinaryContent(null)).toBe(false);
    });

    test('returns false for undefined content', () => {
      expect(hasBinaryContent(undefined)).toBe(false);
    });

    test('returns false for empty string', () => {
      expect(hasBinaryContent('')).toBe(false);
    });
  });

  describe('text file detection', () => {
    test('returns false for plain text content', () => {
      expect(hasBinaryContent('Hello, world!')).toBe(false);
    });

    test('returns false for JavaScript code', () => {
      const jsCode = `
function hello() {
  console.log('Hello, world!');
}
export default hello;
`;
      expect(hasBinaryContent(jsCode)).toBe(false);
    });

    test('returns false for JSON content', () => {
      const json = JSON.stringify({name: 'test', value: 123});
      expect(hasBinaryContent(json)).toBe(false);
    });

    test('returns false for HTML content', () => {
      const html = '<!DOCTYPE html><html><head><title>Test</title></head><body></body></html>';
      expect(hasBinaryContent(html)).toBe(false);
    });

    test('returns false for content with newlines and special characters', () => {
      const content = 'Line 1\nLine 2\r\nLine 3\tTabbed';
      expect(hasBinaryContent(content)).toBe(false);
    });
  });

  describe('null byte detection', () => {
    test('returns true for content with null byte at the beginning', () => {
      expect(hasBinaryContent('\x00hello')).toBe(true);
    });

    test('returns true for content with null byte in the middle', () => {
      expect(hasBinaryContent('hello\x00world')).toBe(true);
    });

    test('returns true for content with multiple null bytes', () => {
      expect(hasBinaryContent('he\x00ll\x00o')).toBe(true);
    });

    test('returns true for content with null byte within first 8192 bytes', () => {
      const content = 'a'.repeat(8000) + '\x00' + 'b'.repeat(100);
      expect(hasBinaryContent(content)).toBe(true);
    });

    test('returns false for content with null byte after first 8192 bytes', () => {
      const content = 'a'.repeat(8200) + '\x00' + 'b'.repeat(100);
      expect(hasBinaryContent(content)).toBe(false);
    });
  });

  describe('image format signatures', () => {
    test('returns true for PNG signature', () => {
      expect(hasBinaryContent('\x89PNG\r\n\x1a\n')).toBe(true);
    });

    test('returns true for JPEG signature', () => {
      expect(hasBinaryContent('\xff\xd8\xff')).toBe(true);
    });

    test('returns true for GIF87a signature', () => {
      expect(hasBinaryContent('GIF87a')).toBe(true);
    });

    test('returns true for GIF89a signature', () => {
      expect(hasBinaryContent('GIF89a')).toBe(true);
    });

    test('returns true for BMP signature', () => {
      expect(hasBinaryContent('BM')).toBe(true);
    });

    test('returns true for ICO signature', () => {
      expect(hasBinaryContent('\x00\x00\x01\x00')).toBe(true);
    });
  });

  describe('document format signatures', () => {
    test('returns true for PDF signature', () => {
      expect(hasBinaryContent('%PDF')).toBe(true);
    });

    test('returns true for ZIP signature (also DOCX, XLSX, etc.)', () => {
      expect(hasBinaryContent('PK\x03\x04')).toBe(true);
    });

    test('returns true for empty ZIP archive signature', () => {
      expect(hasBinaryContent('PK\x05\x06')).toBe(true);
    });

    test('returns true for GZIP signature', () => {
      expect(hasBinaryContent('\x1f\x8b')).toBe(true);
    });
  });

  describe('executable format signatures', () => {
    test('returns true for ELF signature (Linux executable)', () => {
      expect(hasBinaryContent('\x7fELF')).toBe(true);
    });

    test('returns true for MZ signature (DOS/Windows executable)', () => {
      expect(hasBinaryContent('MZ')).toBe(true);
    });

    test('returns true for Mach-O universal binary signature', () => {
      expect(hasBinaryContent('\xca\xfe\xba\xbe')).toBe(true);
    });

    test('returns true for Mach-O 32-bit signature', () => {
      expect(hasBinaryContent('\xfe\xed\xfa\xce')).toBe(true);
    });

    test('returns true for Mach-O 64-bit signature', () => {
      expect(hasBinaryContent('\xfe\xed\xfa\xcf')).toBe(true);
    });

    test('returns true for Mach-O 32-bit reverse signature', () => {
      expect(hasBinaryContent('\xce\xfa\xed\xfe')).toBe(true);
    });

    test('returns true for Mach-O 64-bit reverse signature', () => {
      expect(hasBinaryContent('\xcf\xfa\xed\xfe')).toBe(true);
    });
  });

  describe('audio format signatures', () => {
    test('returns true for RIFF signature (WAV, AVI, WebP)', () => {
      expect(hasBinaryContent('RIFF')).toBe(true);
    });

    test('returns true for OGG signature', () => {
      expect(hasBinaryContent('OggS')).toBe(true);
    });

    test('returns true for FLAC signature', () => {
      expect(hasBinaryContent('fLaC')).toBe(true);
    });

    test('returns true for MP3 with ID3 tag signature', () => {
      expect(hasBinaryContent('ID3')).toBe(true);
    });

    test('returns true for MP3 frame sync signature (0xfffb)', () => {
      expect(hasBinaryContent('\xff\xfb')).toBe(true);
    });

    test('returns true for MP3 frame sync signature (0xfffa)', () => {
      expect(hasBinaryContent('\xff\xfa')).toBe(true);
    });

    test('returns true for MP3 frame sync signature (0xfff3)', () => {
      expect(hasBinaryContent('\xff\xf3')).toBe(true);
    });

    test('returns true for MP3 frame sync signature (0xfff2)', () => {
      expect(hasBinaryContent('\xff\xf2')).toBe(true);
    });
  });

  describe('font format signatures', () => {
    test('returns true for WOFF signature', () => {
      expect(hasBinaryContent('wOFF')).toBe(true);
    });

    test('returns true for WOFF2 signature', () => {
      expect(hasBinaryContent('wOF2')).toBe(true);
    });
  });

  describe('binary signature with additional content', () => {
    test('returns true for PNG with additional data', () => {
      expect(hasBinaryContent('\x89PNG\r\n\x1a\nrest of png data here')).toBe(true);
    });

    test('returns true for PDF with version', () => {
      expect(hasBinaryContent('%PDF-1.7')).toBe(true);
    });

    test('returns false for text that happens to contain signature-like substrings', () => {
      // Text starting with "BM" would be detected as BMP
      // but "IBM" would not match because it doesn't START with BM
      expect(hasBinaryContent('IBM is a company')).toBe(false);
    });
  });
});
