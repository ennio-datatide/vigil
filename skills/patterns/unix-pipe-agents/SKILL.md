---
name: unix-pipe-agents
description: Build AI workflows using Unix philosophy -- prompts as files, LLM calls as commands, composition via pipes. Use when you want lightweight, composable, shell-native AI workflows without writing application code.
---

# Unix Pipe Agents

> The pattern works in any language but is most natural in Bash/shell. Python helper shown for the LLM call.

## When to Use

- You want to compose AI operations in the shell like any other Unix tool
- Prompts should be version-controlled Markdown files, not embedded in code
- You need quick, ad-hoc AI pipelines without writing application code
- Your workflow is linear: input -> transform -> transform -> output
- **NOT** for complex branching or loops -- use dag-workflows instead
- **NOT** for multi-turn conversations -- use agent-loop instead
- **NOT** for tasks requiring tool calling -- use tool-calling instead

## The Pattern

```python
#!/usr/bin/env python3
"""llm-pipe: A Unix-friendly LLM command. Reads stdin, applies a pattern, writes stdout."""

import anthropic
import sys
import os

client = anthropic.Anthropic()

PATTERNS_DIR = os.path.expanduser("~/.patterns")


def load_pattern(name: str) -> str:
    """Load a prompt pattern from the patterns directory."""
    pattern_file = os.path.join(PATTERNS_DIR, name, "system.md")
    if not os.path.exists(pattern_file):
        print(f"Error: Pattern '{name}' not found at {pattern_file}", file=sys.stderr)
        sys.exit(1)
    with open(pattern_file) as f:
        return f.read()


def run(pattern_name: str, input_text: str) -> str:
    """Run input through an LLM with the given pattern as system prompt."""
    system_prompt = load_pattern(pattern_name)
    response = client.messages.create(
        model="claude-sonnet-4-20250514",
        max_tokens=4096,
        system=system_prompt,
        messages=[{"role": "user", "content": input_text}],
    )
    return response.content[0].text


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: echo 'input' | llm-pipe <pattern-name>", file=sys.stderr)
        sys.exit(1)

    pattern_name = sys.argv[1]
    input_text = sys.stdin.read()
    output = run(pattern_name, input_text)
    print(output)
```

**Pattern files** -- each pattern is a directory with a `system.md`:

```
~/.patterns/
├── summarize/
│   └── system.md      # "You are a summarizer. Output a concise summary..."
├── extract_actions/
│   └── system.md      # "Extract action items from the text. Output as a bullet list..."
├── write_tests/
│   └── system.md      # "Write unit tests for the code. Use pytest..."
├── review_code/
│   └── system.md      # "Review this code for bugs, security, performance..."
├── translate_spanish/
│   └── system.md      # "Translate to Spanish. Preserve formatting..."
└── explain_simply/
    └── system.md      # "Explain this to a 10-year-old..."
```

**Example `~/.patterns/summarize/system.md`:**

```markdown
You are a precise summarizer. Given any text, produce a concise summary that captures:
- The main point or thesis
- Key supporting arguments or facts
- Any action items or conclusions

Output only the summary. No preamble. No commentary. Keep it under 200 words.
```

**Shell usage -- compose with pipes:**

```bash
# Summarize a document
cat report.pdf | pdf-to-text | llm-pipe summarize

# Extract action items from meeting notes
cat meeting.md | llm-pipe extract_actions

# Chain: summarize, then translate
cat paper.md | llm-pipe summarize | llm-pipe translate_spanish

# Code review pipeline
cat app.py | llm-pipe review_code | llm-pipe extract_actions > review.md

# Process multiple files
for f in docs/*.md; do
    echo "=== $f ===" >> summaries.md
    cat "$f" | llm-pipe summarize >> summaries.md
done

# Combine with standard Unix tools
git diff HEAD~5 | llm-pipe summarize | mail -s "Weekly changes" team@company.com

# YouTube transcript -> summary -> action items
yt-dlp --write-auto-sub --sub-lang en --skip-download -o transcript "VIDEO_URL" \
    && cat transcript.en.vtt | llm-pipe summarize | llm-pipe extract_actions
```

## Example

A complete content pipeline using only shell commands:

```bash
#!/bin/bash
# content-pipeline.sh: Research -> Draft -> Review -> Publish

TOPIC="$1"

# Step 1: Research (using a web search tool + LLM)
echo "$TOPIC" | llm-pipe research_topic > /tmp/research.md

# Step 2: Draft a blog post from research
cat /tmp/research.md | llm-pipe write_blog_post > /tmp/draft.md

# Step 3: Review the draft
cat /tmp/draft.md | llm-pipe review_code > /tmp/review.md

# Step 4: Improve based on review
cat /tmp/draft.md /tmp/review.md | llm-pipe improve_with_feedback > /tmp/final.md

# Step 5: Generate metadata
cat /tmp/final.md | llm-pipe extract_metadata > /tmp/meta.json

echo "Published: /tmp/final.md"
echo "Metadata: /tmp/meta.json"
```

**Creating new patterns is trivial:**

```bash
# Create a new pattern in seconds
mkdir -p ~/.patterns/fix_typos
cat > ~/.patterns/fix_typos/system.md << 'EOF'
Fix all typos and grammatical errors in the text. Output the corrected text only.
Do not change meaning, tone, or style. Do not add or remove content.
EOF

# Use it immediately
cat essay.md | llm-pipe fix_typos > essay_fixed.md
```

## Common Pitfalls

1. **No error handling in pipes** -- If one step in a pipe fails silently, garbage flows downstream. Use `set -o pipefail` in bash scripts and check exit codes.
2. **Context loss between pipes** -- Each `llm-pipe` call is independent. If step 3 needs context from step 1, you must explicitly pass it (e.g., `cat step1.md step2.md | llm-pipe combine`).
3. **Large inputs** -- Piping a 10MB file into an LLM will exceed context windows. Add a truncation or chunking step before the LLM call.
4. **Pattern drift** -- As you accumulate 50+ patterns, they become hard to manage. Use a naming convention and keep patterns focused on one task each.
5. **No streaming** -- The basic pattern waits for the full response. For interactive use, add streaming support to `llm-pipe` so output appears progressively.

## Key Insight

The Unix pipe is the original workflow orchestrator: by making LLM calls behave like standard Unix commands (read stdin, write stdout, configured by files), you get composability, version control, and the entire Unix toolchain for free -- no framework required.
