---
name: orchestrator-workers
description: Build a central orchestrator LLM that decomposes tasks and delegates subtasks to specialized worker LLMs. Use when a task is too complex for a single agent and needs to be broken into independent subtasks with different expertise.
---

# Orchestrator-Workers

> The pattern works in any language. Python shown for clarity.

## When to Use

- A task naturally decomposes into independent subtasks (e.g., "build a website" -> design, backend, frontend, tests)
- Different subtasks need different expertise or system prompts
- You want a central coordinator that synthesizes worker outputs
- The decomposition is dynamic -- the orchestrator decides at runtime what workers to spawn
- **NOT** for a fixed sequence of steps -- use prompt-chaining instead
- **NOT** for tasks that need back-and-forth between agents -- use multi-agent-handoffs instead
- **NOT** for independent parallel tasks without coordination -- use parallelization instead

## The Pattern

```python
import anthropic
import json

client = anthropic.Anthropic()


def llm(prompt: str, system: str = "") -> str:
    response = client.messages.create(
        model="claude-sonnet-4-20250514",
        max_tokens=4096,
        system=system if system else anthropic.NOT_GIVEN,
        messages=[{"role": "user", "content": prompt}],
    )
    return response.content[0].text


# 1. Orchestrator: decomposes the task into subtasks
def orchestrate(task: str) -> dict:
    plan = llm(
        prompt=f"""Break this task into 2-5 independent subtasks that can be done by separate workers.

Task: {task}

Output JSON:
{{
  "subtasks": [
    {{"id": 1, "description": "...", "worker_type": "researcher|coder|writer|analyst"}},
    ...
  ]
}}""",
        system="You are a project manager. Decompose tasks into clear, independent subtasks. Output only valid JSON.",
    )
    return json.loads(plan)


# 2. Worker: executes a single subtask with a specialized persona
WORKER_PROMPTS = {
    "researcher": "You are a thorough researcher. Find relevant information and cite sources.",
    "coder": "You are an expert programmer. Write clean, tested, production-ready code.",
    "writer": "You are a skilled technical writer. Write clear, concise documentation.",
    "analyst": "You are a data analyst. Analyze data and provide actionable insights.",
}


def worker(subtask: dict) -> dict:
    system = WORKER_PROMPTS.get(subtask["worker_type"], "You are a helpful assistant.")
    result = llm(
        prompt=f"Complete this task:\n\n{subtask['description']}",
        system=system,
    )
    return {"id": subtask["id"], "description": subtask["description"], "result": result}


# 3. Synthesizer: combines all worker outputs into a final result
def synthesize(task: str, results: list[dict]) -> str:
    results_text = "\n\n".join(
        f"### Subtask {r['id']}: {r['description']}\n{r['result']}" for r in results
    )
    return llm(
        prompt=f"""Original task: {task}

Worker results:
{results_text}

Synthesize these results into a single, coherent response that fully addresses the original task.""",
        system="You are a senior editor. Combine multiple contributions into a polished final result.",
    )


# 4. Run the full pattern
def orchestrator_workers(task: str) -> str:
    # Decompose
    plan = orchestrate(task)
    print(f"Plan: {json.dumps(plan, indent=2)}")

    # Execute workers (sequentially here; see parallelization pattern for concurrent)
    results = []
    for subtask in plan["subtasks"]:
        print(f"Worker {subtask['id']}: {subtask['description'][:80]}...")
        result = worker(subtask)
        results.append(result)

    # Synthesize
    final = synthesize(task, results)
    return final


answer = orchestrator_workers(
    "Create a technical blog post about building REST APIs with FastAPI, "
    "including code examples, performance benchmarks, and deployment instructions"
)
print(answer)
```

## Example

Building a full-stack feature:

```python
plan = orchestrate("Add user authentication to our Flask app")
# Orchestrator produces:
# 1. researcher: "Research best practices for Flask auth (JWT vs session)"
# 2. coder: "Implement JWT auth middleware for Flask with login/register endpoints"
# 3. coder: "Write pytest tests for the auth endpoints"
# 4. writer: "Write API documentation for the auth endpoints"

# Each worker executes independently with its specialized system prompt
# Synthesizer combines: architecture decision + code + tests + docs
```

## Common Pitfalls

1. **Dependent subtasks** -- If subtask B needs output from subtask A, they are not independent. Either chain them or have the orchestrator sequence them explicitly.
2. **Vague decomposition** -- "Do the backend stuff" is too vague for a worker. The orchestrator must produce specific, actionable descriptions.
3. **Missing synthesis** -- Without a synthesis step, you get N disconnected outputs. The synthesizer ensures consistency and fills gaps.
4. **Too many workers** -- More than 5 workers usually means the task was over-decomposed. Combine related subtasks.
5. **Wrong worker type** -- If the orchestrator picks "researcher" for a coding task, the output quality drops. Validate worker_type against the subtask description, or let workers self-select their approach.

## Key Insight

The orchestrator-workers pattern turns one hard problem into several easier problems: a planner LLM decomposes the task, specialized worker LLMs execute each piece independently, and a synthesizer LLM combines the results into a coherent whole.
