import type { sessions } from '../db/schema.js';

export function withParsedGitMetadata(row: typeof sessions.$inferSelect) {
  let gitMetadata = null;
  if (row.gitMetadata) {
    try {
      gitMetadata = JSON.parse(row.gitMetadata);
    } catch {
      gitMetadata = null;
    }
  }
  return { ...row, gitMetadata };
}
