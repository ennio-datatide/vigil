---
name: parallelization
description: Run multiple LLM calls simultaneously and aggregate results. Use when you have independent subtasks that can execute concurrently, like analyzing multiple documents, getting multiple perspectives, or processing items in batch.
---

# Parallelization

> The pattern works in any language. Python shown for clarity.

## When to Use

- You have multiple independent LLM calls that do not depend on each other
- You want to reduce total latency by running calls concurrently
- You need multiple perspectives on the same input (voting/ensemble)
- You are processing a batch of items with the same prompt
- **NOT** for sequential steps where each depends on the prior -- use prompt-chaining instead
- **NOT** for dynamic task decomposition -- use orchestrator-workers instead

## The Pattern

```python
import anthropic
import asyncio
import json

client = anthropic.AsyncAnthropic()


async def llm(prompt: str, system: str = "") -> str:
    """Async LLM call."""
    response = await client.messages.create(
        model="claude-sonnet-4-20250514",
        max_tokens=4096,
        system=system if system else anthropic.NOT_GIVEN,
        messages=[{"role": "user", "content": prompt}],
    )
    return response.content[0].text


# --- Pattern 1: Fan-out / Fan-in (sectioning) ---

async def fan_out_fan_in(items: list[str], prompt_template: str) -> list[str]:
    """Process multiple items concurrently with the same prompt."""
    tasks = [
        llm(prompt_template.format(item=item))
        for item in items
    ]
    return await asyncio.gather(*tasks)


# --- Pattern 2: Voting (multiple perspectives) ---

async def vote(prompt: str, n_voters: int = 3) -> dict:
    """Get multiple independent responses and find consensus."""
    tasks = [
        llm(
            prompt + "\n\nRespond with a single JSON object: {\"answer\": \"...\", \"confidence\": 0.0-1.0}",
            system=f"You are evaluator #{i+1}. Give your independent assessment.",
        )
        for i in range(n_voters)
    ]
    responses = await asyncio.gather(*tasks)

    # Parse and tally votes
    votes = []
    for r in responses:
        try:
            parsed = json.loads(r)
            votes.append(parsed)
        except json.JSONDecodeError:
            continue

    # Find majority answer
    from collections import Counter
    answer_counts = Counter(v["answer"] for v in votes)
    winner = answer_counts.most_common(1)[0]

    return {
        "answer": winner[0],
        "vote_count": winner[1],
        "total_voters": len(votes),
        "all_votes": votes,
    }


# --- Pattern 3: Parallel analysis with aggregation ---

async def parallel_analyze(document: str, analyses: list[dict]) -> str:
    """Run multiple analyses on the same document, then aggregate."""
    # Fan-out: run all analyses concurrently
    tasks = [
        llm(
            f"Analyze this document for {a['focus']}:\n\n{document}",
            system=a["system"],
        )
        for a in analyses
    ]
    results = await asyncio.gather(*tasks)

    # Fan-in: aggregate all analyses
    combined = "\n\n".join(
        f"## {a['focus']}\n{result}"
        for a, result in zip(analyses, results)
    )
    return await llm(
        f"Synthesize these analyses into a single comprehensive report:\n\n{combined}",
        system="You are a senior analyst. Combine multiple perspectives into a coherent report.",
    )


# --- Run examples ---

async def main():
    # Fan-out: summarize 4 articles concurrently
    articles = ["Article 1 text...", "Article 2 text...", "Article 3 text...", "Article 4 text..."]
    summaries = await fan_out_fan_in(
        articles,
        "Summarize this article in 2 sentences:\n\n{item}",
    )
    for i, s in enumerate(summaries):
        print(f"Summary {i+1}: {s}\n")

    # Voting: get consensus on a classification
    result = await vote("Is this email spam or legitimate? 'You won a free iPhone! Click here now!'")
    print(f"Verdict: {result['answer']} ({result['vote_count']}/{result['total_voters']} votes)")

    # Parallel analysis: analyze a document from multiple angles
    report = await parallel_analyze(
        document="[quarterly earnings report text]",
        analyses=[
            {"focus": "financial performance", "system": "You are a financial analyst."},
            {"focus": "market risks", "system": "You are a risk analyst."},
            {"focus": "competitive position", "system": "You are a strategy consultant."},
        ],
    )
    print(f"Combined report:\n{report}")


asyncio.run(main())
```

## Example

Parallel code review -- check the same code for different concerns simultaneously:

```python
results = await asyncio.gather(
    llm(code, system="Review this code for security vulnerabilities."),
    llm(code, system="Review this code for performance issues."),
    llm(code, system="Review this code for maintainability and readability."),
    llm(code, system="Review this code for test coverage gaps."),
)
# 4 reviews complete in the time of 1 sequential call
```

## Common Pitfalls

1. **Hidden dependencies** -- If task B needs task A's output, they cannot run in parallel. Identify dependencies before parallelizing.
2. **Rate limits** -- Launching 50 concurrent API calls will hit rate limits. Use `asyncio.Semaphore` to cap concurrency (e.g., 10 at a time).
3. **Error handling** -- One failed call in `asyncio.gather` raises an exception by default. Use `return_exceptions=True` to get all results including errors.
4. **Context window waste** -- Sending the same large document in 5 parallel calls multiplies token costs. Consider extracting relevant sections first.
5. **Inconsistent outputs** -- Parallel calls to the same model can give contradictory results. Use voting to find consensus, or specify that consistency matters in the prompt.

## Key Insight

Parallelization is the simplest performance optimization for LLM applications: if two calls do not depend on each other, run them at the same time with asyncio.gather and cut your latency by the number of parallel tasks.
