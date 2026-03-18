---
name: prompt-chaining
description: Chain sequential LLM calls where each step uses the output of the previous step. Use when you have a fixed multi-step workflow like extract-then-summarize, translate-then-format, or draft-then-review.
---

# Prompt Chaining

> The pattern works in any language. Python shown for clarity.

## When to Use

- You have a fixed sequence of LLM steps (e.g., extract -> transform -> summarize)
- Each step's output feeds into the next step's prompt
- You want validation gates between steps (reject bad output early)
- The workflow is deterministic -- you know the steps in advance
- **NOT** for dynamic tool selection -- use agent-loop instead
- **NOT** for parallel independent tasks -- use parallelization instead

## The Pattern

```python
import anthropic
import json

client = anthropic.Anthropic()


def llm(prompt: str, system: str = "") -> str:
    """Single LLM call helper."""
    messages = [{"role": "user", "content": prompt}]
    response = client.messages.create(
        model="claude-sonnet-4-20250514",
        max_tokens=4096,
        system=system if system else anthropic.NOT_GIVEN,
        messages=messages,
    )
    return response.content[0].text


def chain(steps: list[dict], initial_input: str) -> list[str]:
    """
    Run a chain of LLM calls. Each step is:
      {"prompt": "...", "system": "...", "validate": callable}

    Use {input} for the current input and {output[N]} for the Nth step's output.
    """
    outputs = []
    current_input = initial_input

    for i, step in enumerate(steps):
        # Interpolate prior outputs into the prompt
        prompt = step["prompt"].replace("{input}", current_input)
        for j, prev_output in enumerate(outputs):
            prompt = prompt.replace(f"{{output[{j}]}}", prev_output)

        # Call the LLM
        result = llm(prompt, system=step.get("system", ""))

        # Optional validation gate
        validate = step.get("validate")
        if validate and not validate(result):
            raise ValueError(f"Step {i} failed validation: {result[:200]}")

        outputs.append(result)
        current_input = result  # Next step receives this step's output

    return outputs


# Define a 3-step chain: extract -> translate -> format
steps = [
    {
        "system": "You are a data extraction expert.",
        "prompt": "Extract all company names and their revenue figures from this text. "
                  "Output as JSON array: [{\"company\": \"...\", \"revenue\": \"...\"}]\n\n{input}",
        "validate": lambda r: r.strip().startswith("["),  # Must be JSON array
    },
    {
        "system": "You are a translator.",
        "prompt": "Translate these company descriptions to Spanish, keeping the JSON structure:\n\n{input}",
        "validate": lambda r: r.strip().startswith("["),
    },
    {
        "system": "You are a report writer.",
        "prompt": "Create a formatted markdown report from this data. "
                  "Use the original English data:\n{output[0]}\n\n"
                  "And the Spanish translation:\n{output[1]}",
    },
]

report = chain(steps, initial_input="Acme Corp reported $5M revenue. Globex had $3.2M...")
final_report = report[-1]
print(final_report)
```

## Example

A code review chain: analyze -> identify issues -> suggest fixes -> generate report:

```python
steps = [
    {
        "system": "You are a code analyzer.",
        "prompt": "Analyze this code for structure and patterns:\n```\n{input}\n```\nOutput a brief analysis.",
    },
    {
        "system": "You are a security reviewer.",
        "prompt": "Given this code analysis:\n{input}\n\nIdentify security issues and bugs. Output as JSON: "
                  '[{"issue": "...", "severity": "high|medium|low", "line": N}]',
        "validate": lambda r: "issue" in r,
    },
    {
        "system": "You are a senior developer.",
        "prompt": "For each issue found:\n{input}\n\nSuggest specific code fixes. Be concrete.",
    },
    {
        "system": "You are a technical writer.",
        "prompt": "Generate a code review report combining:\n"
                  "Analysis: {output[0]}\nIssues: {output[1]}\nFixes: {output[2]}",
    },
]

results = chain(steps, initial_input=open("app.py").read())
```

## Common Pitfalls

1. **No validation gates** -- Without checking intermediate outputs, garbage propagates through the entire chain. Add a `validate` function to catch bad outputs early.
2. **Losing context** -- Later steps may need outputs from earlier steps, not just the immediately preceding one. Use `{output[N]}` syntax to reference any prior step.
3. **Overly long chains** -- Each step adds latency and cost. If you have 8+ steps, consider whether some can be combined into a single prompt.
4. **Rigid error handling** -- If step 3 of 5 fails validation, you have options: retry just that step, restart from step 2, or abort. Choose based on the cost of retrying vs. failing.
5. **Not using system prompts** -- Each step benefits from a focused system prompt that defines the role. Without it, the model tries to be a generalist at every step.

## Key Insight

Prompt chaining is just string interpolation in a for loop -- each LLM call produces text that gets inserted into the next prompt, with optional validation gates between steps to catch errors early.
