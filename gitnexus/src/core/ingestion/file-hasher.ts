/**
 * Incremental Indexing — File Hash Utilities
 *
 * Computes SHA-256 (first 16 hex chars) fingerprints for source files so the
 * analysis pipeline can skip a full rebuild when nothing has changed.
 *
 * Design goals:
 *   - Fast: reads each file once; hashes are cheap relative to tree-sitter parsing.
 *   - Deterministic: content-based, not mtime-based, so results are stable across
 *     `git clone`, CI caches, and Windows ↔ Unix cross-mounts.
 *   - Minimal: returns a plain Record<string, string> that fits in meta.json.
 */

import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';

const HASH_BYTES = 16; // 16 hex chars = 64 bits — sufficient for change detection

/** Compute a short SHA-256 fingerprint of raw file content. */
export function hashFileContent(content: string): string {
  return createHash('sha256').update(content, 'utf8').digest('hex').slice(0, HASH_BYTES);
}

/** Compute a short SHA-256 fingerprint from a raw buffer (binary-safe). */
export function hashFileBuffer(buf: Buffer): string {
  return createHash('sha256').update(buf).digest('hex').slice(0, HASH_BYTES);
}

/**
 * Compute fingerprints for a list of repo-relative file paths.
 *
 * Files that cannot be read (permissions, deleted race) are silently skipped.
 *
 * @param repoPath   Absolute path to the repository root.
 * @param filePaths  Repo-relative paths (as returned by walkRepositoryPaths).
 * @returns          Map of repo-relative path → short hash string.
 */
export async function computeFileHashes(
  repoPath: string,
  filePaths: readonly string[],
): Promise<Record<string, string>> {
  const result: Record<string, string> = Object.create(null);

  const CONCURRENCY = 32;
  for (let i = 0; i < filePaths.length; i += CONCURRENCY) {
    const batch = filePaths.slice(i, i + CONCURRENCY);
    await Promise.allSettled(
      batch.map(async (relPath) => {
        try {
          const buf = await fs.readFile(path.join(repoPath, relPath));
          result[relPath] = hashFileBuffer(buf);
        } catch {
          // Skip unreadable files — they will be absent from the hash map and
          // treated as "changed" on the next comparison.
        }
      }),
    );
  }

  return result;
}

/**
 * Compare two file-hash maps and return the set of paths that were added,
 * removed, or modified.
 *
 * @param previous  Hashes from the last successful analysis (may be empty).
 * @param current   Freshly-computed hashes for the same file set.
 * @returns         Repo-relative paths that differ between the two maps,
 *                  or `null` if no changes were detected (fast path).
 */
export function diffFileHashes(
  previous: Record<string, string>,
  current: Record<string, string>,
): Set<string> | null {
  const changed = new Set<string>();

  for (const [p, hash] of Object.entries(current)) {
    if (previous[p] !== hash) changed.add(p);
  }

  // Files present in previous but absent in current are deletions — treat as
  // changed so the caller can decide how to handle them.
  for (const p of Object.keys(previous)) {
    if (!(p in current)) changed.add(p);
  }

  return changed.size === 0 ? null : changed;
}
