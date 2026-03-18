---
name: evaluator-optimizer
description: Build a generate-evaluate-improve loop where one LLM produces output and another evaluates and provides feedback for iterative refinement. Use for quality-critical tasks like code generation, writing, or any output that benefits from iterative improvement.
---

# Evaluator-Optimizer

> The pattern works in any language. Python shown for clarity.

## When to Use

- The task has measurable quality criteria (correctness, style, completeness)
- First-draft output is unlikely to be good enough
- You want automatic quality improvement without human review
- You can define what "good" looks like in a rubric
- **NOT** for simple one-shot tasks -- a single LLM call suffices
- **NOT** for tasks without clear quality criteria -- use prompt-chaining for fixed workflows
- **NOT** for collaborative reasoning -- use multi-agent-debate instead

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


def generate(task: str, feedback: str = "") -> str:
    """Generate or refine output based on the task and optional feedback."""
    prompt = f"Task: {task}"
    if feedback:
        prompt += f"\n\nPrevious attempt received this feedback:\n{feedback}\n\nImprove your output based on the feedback."
    return llm(
        prompt=prompt,
        system="You are an expert. Produce high-quality output. If feedback is provided, address every point.",
    )


def evaluate(task: str, output: str, criteria: list[str]) -> dict:
    """Evaluate output against criteria. Returns score and feedback."""
    criteria_text = "\n".join(f"- {c}" for c in criteria)
    result = llm(
        prompt=f"""Evaluate this output against the criteria below.

Task: {task}

Output to evaluate:
{output}

Criteria:
{criteria_text}

Respond with JSON:
{{
  "score": <1-10>,
  "passed": [<list of criteria that are met>],
  "failed": [<list of criteria that are NOT met>],
  "feedback": "<specific, actionable feedback for improvement>"
}}""",
        system="You are a strict evaluator. Be honest and specific. Only give 8+ if the output truly meets all criteria.",
    )
    return json.loads(result)


def evaluator_optimizer(
    task: str,
    criteria: list[str],
    threshold: int = 7,
    max_iterations: int = 3,
) -> dict:
    """Generate, evaluate, and iteratively improve until quality threshold is met."""
    feedback = ""

    for iteration in range(max_iterations):
        # Generate (or refine)
        output = generate(task, feedback)
        print(f"\n--- Iteration {iteration + 1} ---")
        print(f"Output preview: {output[:200]}...")

        # Evaluate
        evaluation = evaluate(task, output, criteria)
        score = evaluation["score"]
        print(f"Score: {score}/10")
        print(f"Failed: {evaluation['failed']}")

        # Check if we met the threshold
        if score >= threshold:
            print(f"Passed at iteration {iteration + 1}")
            return {
                "output": output,
                "score": score,
                "iterations": iteration + 1,
                "evaluation": evaluation,
            }

        # Use feedback for next iteration
        feedback = evaluation["feedback"]

    print(f"Max iterations reached. Best score: {score}/10")
    return {
        "output": output,
        "score": score,
        "iterations": max_iterations,
        "evaluation": evaluation,
    }


# Run it
result = evaluator_optimizer(
    task="Write a Python function that implements binary search on a sorted list. "
         "Include type hints, docstring, and handle edge cases.",
    criteria=[
        "Correct binary search algorithm (O(log n) complexity)",
        "Type hints on all parameters and return value",
        "Comprehensive docstring with examples",
        "Handles edge cases: empty list, single element, element not found",
        "Clean, readable code following PEP 8",
    ],
    threshold=8,
    max_iterations=3,
)
print(f"\nFinal output (score {result['score']}, {result['iterations']} iterations):")
print(result["output"])
```

## Example

Iterative API design refinement:

```python
result = evaluator_optimizer(
    task="Design a REST API for a todo app with CRUD operations",
    criteria=[
        "RESTful URL structure (nouns, not verbs)",
        "Proper HTTP methods (GET, POST, PUT, DELETE)",
        "Consistent error response format",
        "Pagination support for list endpoints",
        "Input validation described for each endpoint",
    ],
    threshold=8,
    max_iterations=3,
)
# Iteration 1: Score 5 -- missing pagination and error format
# Iteration 2: Score 7 -- added pagination but error format inconsistent
# Iteration 3: Score 9 -- all criteria met
```

## Common Pitfalls

1. **Vague criteria** -- "Make it good" is not evaluable. Write specific, binary criteria: "All functions have type hints" not "Code quality is high."
2. **Lenient evaluator** -- The evaluator tends to give 7+ by default. Use the system prompt to enforce strict scoring: "Only give 8+ if ALL criteria are met."
3. **No iteration limit** -- Without `max_iterations`, a perfectionist loop runs forever. 3 iterations is usually enough; diminishing returns after that.
4. **Feedback not actionable** -- "This could be better" does not help. The evaluator must say exactly what failed and how to fix it.
5. **Same model for both** -- Using the same model to generate and evaluate can create blind spots. Consider using a stronger model for evaluation, or providing the evaluator with different context.

## Key Insight

The evaluator-optimizer pattern is automated code review: generate output, evaluate it against explicit criteria, feed the evaluation back as improvement instructions, and repeat until quality is sufficient or iterations run out.
