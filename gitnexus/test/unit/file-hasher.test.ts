import { describe, it, expect } from 'vitest';
import { hashFileContent, hashFileBuffer, diffFileHashes } from '../../src/core/ingestion/file-hasher.js';

describe('file-hasher', () => {
  describe('hashFileContent', () => {
    it('returns a 16-character hex string', () => {
      const hash = hashFileContent('hello world');
      expect(hash).toHaveLength(16);
      expect(hash).toMatch(/^[0-9a-f]+$/);
    });

    it('returns the same hash for identical content', () => {
      expect(hashFileContent('fn main() {}')).toBe(hashFileContent('fn main() {}'));
    });

    it('returns different hashes for different content', () => {
      expect(hashFileContent('fn foo() {}')).not.toBe(hashFileContent('fn bar() {}'));
    });

    it('handles empty string', () => {
      const hash = hashFileContent('');
      expect(hash).toHaveLength(16);
    });

    it('handles unicode content', () => {
      const hash = hashFileContent('pub fn grüßen() {}');
      expect(hash).toHaveLength(16);
    });
  });

  describe('hashFileBuffer', () => {
    it('returns a 16-character hex string', () => {
      const hash = hashFileBuffer(Buffer.from('hello'));
      expect(hash).toHaveLength(16);
    });

    it('matches hashFileContent for the same UTF-8 text', () => {
      const text = 'struct Foo { bar: u32 }';
      expect(hashFileBuffer(Buffer.from(text, 'utf8'))).toBe(hashFileContent(text));
    });
  });

  describe('diffFileHashes', () => {
    it('returns null when maps are identical', () => {
      const hashes = { 'src/main.rs': 'abc123', 'src/lib.rs': 'def456' };
      expect(diffFileHashes(hashes, { ...hashes })).toBeNull();
    });

    it('returns null for two empty maps', () => {
      expect(diffFileHashes({}, {})).toBeNull();
    });

    it('detects a modified file', () => {
      const prev = { 'src/main.rs': 'aaa', 'src/lib.rs': 'bbb' };
      const curr = { 'src/main.rs': 'aaa', 'src/lib.rs': 'ccc' };
      const changed = diffFileHashes(prev, curr);
      expect(changed).not.toBeNull();
      expect(changed!.has('src/lib.rs')).toBe(true);
      expect(changed!.has('src/main.rs')).toBe(false);
    });

    it('detects a new file', () => {
      const prev = { 'src/main.rs': 'aaa' };
      const curr = { 'src/main.rs': 'aaa', 'src/new.rs': 'bbb' };
      const changed = diffFileHashes(prev, curr);
      expect(changed).not.toBeNull();
      expect(changed!.has('src/new.rs')).toBe(true);
    });

    it('detects a deleted file', () => {
      const prev = { 'src/main.rs': 'aaa', 'src/old.rs': 'bbb' };
      const curr = { 'src/main.rs': 'aaa' };
      const changed = diffFileHashes(prev, curr);
      expect(changed).not.toBeNull();
      expect(changed!.has('src/old.rs')).toBe(true);
    });

    it('reports only the changed files, not unchanged ones', () => {
      const prev = { 'a.rs': '111', 'b.rs': '222', 'c.rs': '333' };
      const curr = { 'a.rs': '111', 'b.rs': 'CHANGED', 'c.rs': '333' };
      const changed = diffFileHashes(prev, curr);
      expect(changed).not.toBeNull();
      expect(changed!.size).toBe(1);
      expect(changed!.has('b.rs')).toBe(true);
    });
  });
});
