---
name: react-agent
description: Build a ReAct (Reason + Act + Observe) agent that explicitly thinks before acting. Use when you need transparent reasoning traces, debugging visibility, or when the task requires planning before each action.
---

# ReAct Agent

> The pattern works in any language. Python shown for clarity.

## When to Use

- You need the agent to show its reasoning before taking actions
- Debugging and transparency matter (you want to see *why* the agent did something)
- The task requires planning and reflection between steps
- You want to constrain the agent to think-then-act discipline
- **NOT** for simple tool calling without reasoning -- use tool-calling instead
- **NOT** for fixed-step workflows -- use prompt-chaining instead

## The Pattern

```python
import anthropic
import json
import subprocess

client = anthropic.Anthropic()

SYSTEM_PROMPT = """You are a ReAct agent. For each step, you MUST follow this exact format:

Thought: <your reasoning about what to do next>
Action: <tool_name>(<json_args>)

After you see the observation (tool result), reason again and decide the next action.
When the task is complete, respond with:

Thought: <final reasoning>
Answer: <your final answer to the user>

Available tools:
- search({"query": "search terms"}) -- Search the web for information
- python({"code": "python code"}) -- Execute Python code and return stdout
- bash({"command": "shell command"}) -- Execute a shell command and return stdout"""


def execute_action(tool_name: str, args: dict) -> str:
    """Execute a tool and return the observation."""
    if tool_name == "search":
        # Stub -- replace with real search API
        return f"Search results for '{args['query']}': [result 1, result 2, ...]"
    elif tool_name == "python":
        try:
            result = subprocess.run(
                ["python3", "-c", args["code"]],
                capture_output=True, text=True, timeout=30,
            )
            return result.stdout or result.stderr or "(no output)"
        except subprocess.TimeoutExpired:
            return "Error: execution timed out after 30s"
    elif tool_name == "bash":
        try:
            result = subprocess.run(
                args["command"], shell=True,
                capture_output=True, text=True, timeout=30,
            )
            return result.stdout or result.stderr or "(no output)"
        except subprocess.TimeoutExpired:
            return "Error: execution timed out after 30s"
    return f"Unknown tool: {tool_name}"


def parse_action(text: str) -> tuple[str, dict] | None:
    """Extract tool name and args from 'Action: tool_name({...})' format."""
    for line in text.strip().split("\n"):
        line = line.strip()
        if line.startswith("Action:"):
            action_str = line[len("Action:"):].strip()
            paren_idx = action_str.index("(")
            tool_name = action_str[:paren_idx].strip()
            args_str = action_str[paren_idx + 1 : -1]  # Strip outer parens
            return tool_name, json.loads(args_str)
    return None


def react_loop(task: str, max_steps: int = 10) -> str:
    """Run the ReAct loop: Thought -> Action -> Observation -> repeat."""
    transcript = f"Task: {task}\n\n"

    for step in range(max_steps):
        # Ask the model to think and act
        response = client.messages.create(
            model="claude-sonnet-4-20250514",
            max_tokens=2048,
            system=SYSTEM_PROMPT,
            messages=[{"role": "user", "content": transcript}],
        )
        reply = response.content[0].text
        transcript += reply + "\n"

        # Check for final answer
        if "Answer:" in reply:
            for line in reply.split("\n"):
                if line.strip().startswith("Answer:"):
                    return line.strip()[len("Answer:"):].strip()

        # Parse and execute action
        action = parse_action(reply)
        if action is None:
            transcript += "\nObservation: No valid action found. Please use the format: Action: tool_name({...})\n\n"
            continue

        tool_name, args = action
        observation = execute_action(tool_name, args)
        transcript += f"Observation: {observation}\n\n"
        print(f"[Step {step + 1}] {tool_name}({args}) -> {observation[:100]}...")

    return "Max steps reached without final answer"


# Run it
answer = react_loop("What is the population of Tokyo divided by the area of France in km2?")
print(f"Final answer: {answer}")
```

## Example

A debugging agent that reasons about code errors:

```
Task: Fix the failing test in test_auth.py

Thought: I need to first see what the test error is.
Action: bash({"command": "python -m pytest test_auth.py -x 2>&1 | tail -20"})
Observation: FAILED test_auth.py::test_login - AssertionError: expected 200, got 401

Thought: The login is returning 401. Let me check the auth handler.
Action: bash({"command": "cat auth/handler.py"})
Observation: [file contents showing token validation...]

Thought: I see the issue -- the token expiry check uses UTC but the test creates tokens with local time.
Action: python({"code": "..."})  # generates fix
Observation: [fix applied]

Thought: Let me verify the fix.
Action: bash({"command": "python -m pytest test_auth.py -x"})
Observation: 1 passed

Answer: Fixed the timezone mismatch in auth/handler.py -- token expiry now consistently uses UTC.
```

## Common Pitfalls

1. **Model breaks format** -- LLMs sometimes skip "Thought:" or use freeform text. Include format instructions in the system prompt AND re-prompt if parsing fails.
2. **Reasoning loops** -- The agent may repeat the same thought/action cycle. Track previous actions and prompt "You already tried X, try something different."
3. **Thought-action mismatch** -- The agent reasons about one approach but takes a different action. The structured format helps catch this.
4. **Over-thinking** -- Some tasks do not need explicit reasoning. Use the simpler agent-loop pattern for straightforward tool-calling tasks.
5. **Context overflow** -- The full transcript grows with every step. For long tasks, summarize earlier steps instead of keeping the raw transcript.

## Key Insight

ReAct forces the LLM to articulate its reasoning before acting, making the agent's decision process transparent, debuggable, and more reliable -- the explicit "Thought" step reduces impulsive tool calls.
