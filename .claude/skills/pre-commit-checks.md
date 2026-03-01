---
name: pre-commit-checks
description: MANDATORY before ANY git commit or push. Run biome check, build, and tests. Never skip. Never commit without passing all checks.
---

# Pre-Commit CI Checks

## HARD RULE

You MUST run these checks BEFORE every `git commit`. No exceptions. No "I'll fix it after." No "it's just a formatting change." Run them EVERY time.

## The Checklist

Run these in order. If any step fails, fix it before committing.

### 1. Biome (lint + format)

```bash
npx biome check --write .
```

Then verify it's clean:

```bash
npx biome check .
```

If biome reports errors after `--write`, there are issues that need manual fixing.

### 2. Build

```bash
npm run build
```

All 4 workspaces must succeed. Zero TypeScript errors.

### 3. Tests

```bash
npm test
```

All tests must pass. The `dist/hooks/hooks/auth.test.ts` file failure is a known pre-existing issue (stale compiled test file) — ignore it. All source tests must pass.

## Only THEN Commit

After all three checks pass:

```bash
git add <files>
git commit -m "..."
git push  # if requested
```

## Common Mistakes

- **Biome formatting**: Long function signatures, trailing commas, line length. Always run `npx biome check --write .` first.
- **TypeScript errors**: New interfaces or changed function signatures that break callers.
- **Test failures**: Changed behavior without updating tests.

## When You Forget

If you already committed and CI fails:
1. Fix the issues
2. Create a NEW commit (don't amend)
3. Push again
