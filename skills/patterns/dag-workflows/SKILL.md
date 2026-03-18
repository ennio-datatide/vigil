---
name: dag-workflows
description: Build directed acyclic graph (DAG) workflows where nodes have prep/exec/post lifecycle methods and communicate through shared state. Use when your workflow has complex dependencies between steps that go beyond simple linear chaining, like build pipelines, data processing, or multi-stage analysis.
---

# DAG Workflows

> The pattern works in any language. Python shown for clarity.

## When to Use

- Your workflow has steps with complex dependencies (not just linear A -> B -> C)
- Some steps can run in parallel while others must wait for prerequisites
- Steps need to share state and communicate results
- You want reusable, composable workflow nodes
- **NOT** for simple linear sequences -- use prompt-chaining instead
- **NOT** for dynamic task decomposition -- use orchestrator-workers instead
- **NOT** for independent parallel tasks without dependencies -- use parallelization instead

## The Pattern

```python
import anthropic
import json
from collections import defaultdict

client = anthropic.Anthropic()


def llm(prompt: str, system: str = "") -> str:
    response = client.messages.create(
        model="claude-sonnet-4-20250514",
        max_tokens=4096,
        system=system if system else anthropic.NOT_GIVEN,
        messages=[{"role": "user", "content": prompt}],
    )
    return response.content[0].text


class SharedStore:
    """Shared state store that all nodes can read from and write to."""

    def __init__(self):
        self._data = {}

    def get(self, key: str, default=None):
        return self._data.get(key, default)

    def set(self, key: str, value):
        self._data[key] = value

    def __repr__(self):
        return f"SharedStore({list(self._data.keys())})"


class Node:
    """A workflow node with prep/exec/post lifecycle."""

    def __init__(self, name: str):
        self.name = name
        self.successors: list[tuple[str, "Node"]] = []  # (edge_label, node)

    def add_successor(self, node: "Node", label: str = "default"):
        self.successors.append((label, node))
        return node

    def prep(self, store: SharedStore) -> dict:
        """Prepare inputs from shared store. Returns context for exec."""
        return {}

    def exec(self, context: dict) -> str:
        """Execute the node's work. Returns a result string."""
        raise NotImplementedError

    def post(self, store: SharedStore, result: str) -> str:
        """Save results to shared store. Returns edge label for routing."""
        return "default"


class LLMNode(Node):
    """A node that calls an LLM with a prompt template."""

    def __init__(self, name: str, prompt_template: str, system: str = "",
                 input_keys: list[str] = None, output_key: str = None):
        super().__init__(name)
        self.prompt_template = prompt_template
        self.system = system
        self.input_keys = input_keys or []
        self.output_key = output_key or name

    def prep(self, store: SharedStore) -> dict:
        return {key: store.get(key, "") for key in self.input_keys}

    def exec(self, context: dict) -> str:
        prompt = self.prompt_template.format(**context)
        return llm(prompt, system=self.system)

    def post(self, store: SharedStore, result: str) -> str:
        store.set(self.output_key, result)
        return "default"


class ConditionalNode(Node):
    """A node that routes to different successors based on a condition."""

    def __init__(self, name: str, condition_key: str, threshold: float = 0.7):
        super().__init__(name)
        self.condition_key = condition_key
        self.threshold = threshold

    def prep(self, store: SharedStore) -> dict:
        return {"value": store.get(self.condition_key)}

    def exec(self, context: dict) -> str:
        return str(context["value"])

    def post(self, store: SharedStore, result: str) -> str:
        try:
            score = float(result)
            return "pass" if score >= self.threshold else "fail"
        except ValueError:
            return "fail"


class DAGRunner:
    """Execute a DAG of nodes following edges from a start node."""

    def __init__(self, start: Node):
        self.start = start

    def run(self, store: SharedStore = None, max_steps: int = 20) -> SharedStore:
        store = store or SharedStore()
        current = self.start
        steps = 0

        while current and steps < max_steps:
            print(f"[Step {steps + 1}] Running: {current.name}")

            # Lifecycle: prep -> exec -> post
            context = current.prep(store)
            result = current.exec(context)
            edge_label = current.post(store, result)

            # Find next node via edge label
            next_node = None
            for label, successor in current.successors:
                if label == edge_label:
                    next_node = successor
                    break

            current = next_node
            steps += 1

        return store


# --- Build a DAG: Research -> Draft -> Evaluate -> (pass: Publish, fail: Revise -> Evaluate) ---

research = LLMNode(
    name="research",
    prompt_template="Research this topic and provide key facts:\n\n{topic}",
    system="You are a researcher. Provide accurate, well-sourced information.",
    input_keys=["topic"],
    output_key="research_results",
)

draft = LLMNode(
    name="draft",
    prompt_template="Write a blog post based on this research:\n\n{research_results}",
    system="You are a technical writer. Write engaging, clear content.",
    input_keys=["research_results"],
    output_key="draft",
)

evaluate = LLMNode(
    name="evaluate",
    prompt_template="Rate this blog post from 0.0 to 1.0 (only output the number):\n\n{draft}",
    system="You are an editor. Be strict. Only rate above 0.7 if the post is publication-ready.",
    input_keys=["draft"],
    output_key="quality_score",
)

gate = ConditionalNode(name="quality_gate", condition_key="quality_score", threshold=0.7)

revise = LLMNode(
    name="revise",
    prompt_template="Improve this blog post:\n\n{draft}\n\nIt scored {quality_score}. Make it better.",
    system="You are a senior editor. Improve quality significantly.",
    input_keys=["draft", "quality_score"],
    output_key="draft",  # Overwrites the draft
)

publish = LLMNode(
    name="publish",
    prompt_template="Format this post for publication with title and meta description:\n\n{draft}",
    system="You are a publishing editor.",
    input_keys=["draft"],
    output_key="published",
)

# Wire the DAG edges
research.add_successor(draft)
draft.add_successor(evaluate)
evaluate.add_successor(gate)
gate.add_successor(publish, label="pass")
gate.add_successor(revise, label="fail")
revise.add_successor(evaluate)  # Loop back for re-evaluation

# Run it
store = SharedStore()
store.set("topic", "Building reliable distributed systems with consensus algorithms")
runner = DAGRunner(research)
result = runner.run(store)
print(f"\nPublished:\n{result.get('published')}")
```

## Example

A data pipeline DAG with parallel branches:

```
[Load CSV] -> [Clean Data] -> [Validate Schema]
                                    |
                          pass: [Analyze] -> [Generate Report]
                          fail: [Log Errors] -> (stop)
```

Each node reads inputs from the shared store and writes outputs back, creating a clean separation between workflow structure and node implementation.

## Common Pitfalls

1. **Cycles without limits** -- A revise -> evaluate -> revise cycle can run forever. Always add a max_steps limit or an iteration counter in the shared store.
2. **Missing store keys** -- If node B reads a key that node A was supposed to write but failed, you get a silent None. Validate required keys in `prep()`.
3. **Over-engineering** -- If your workflow is A -> B -> C with no branches, use prompt-chaining. DAGs add value when you have conditional routing or convergent paths.
4. **State mutation bugs** -- Multiple nodes writing to the same key (like "draft" in the revise loop) is powerful but can cause confusion. Document which nodes write which keys.
5. **No observability** -- Without logging, a 10-node DAG is a black box. Log every node entry/exit and the edge taken.

## Key Insight

All LLM workflow patterns -- chains, loops, branches, parallel fan-out -- reduce to a directed graph of nodes with shared state: define nodes with prep/exec/post lifecycle methods, connect them with labeled edges, and let a runner traverse the graph.
