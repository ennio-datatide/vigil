---
name: agent-loop
description: Build an autonomous AI agent that calls tools in a loop until the task is complete. Use when you need an LLM to take multiple actions, use tools, and decide when to stop on its own.
---

# Agent Loop

> The pattern works in any language. Python shown for clarity.

## When to Use

- You need an LLM to autonomously complete a multi-step task
- The number of steps is not known in advance
- The agent must decide which tools to call and when to stop
- **NOT** for fixed sequences of LLM calls -- use prompt-chaining instead
- **NOT** for multiple agents collaborating -- use orchestrator-workers or multi-agent-handoffs instead

## The Pattern

```python
import anthropic
import json

client = anthropic.Anthropic()

# 1. Define tools with JSON schemas
tools = [
    {
        "name": "get_weather",
        "description": "Get current weather for a city",
        "input_schema": {
            "type": "object",
            "properties": {
                "city": {"type": "string", "description": "City name"}
            },
            "required": ["city"],
        },
    },
    {
        "name": "send_email",
        "description": "Send an email to a recipient",
        "input_schema": {
            "type": "object",
            "properties": {
                "to": {"type": "string"},
                "subject": {"type": "string"},
                "body": {"type": "string"},
            },
            "required": ["to", "subject", "body"],
        },
    },
]

# 2. Implement tool execution
def execute_tool(name: str, input: dict) -> str:
    if name == "get_weather":
        return json.dumps({"temp": 72, "condition": "sunny", "city": input["city"]})
    if name == "send_email":
        return f"Email sent to {input['to']}"
    return f"Unknown tool: {name}"

# 3. The agent loop
def agent_loop(task: str, max_iterations: int = 10) -> str:
    messages = [{"role": "user", "content": task}]

    for i in range(max_iterations):
        response = client.messages.create(
            model="claude-sonnet-4-20250514",
            max_tokens=4096,
            tools=tools,
            messages=messages,
        )

        # Check if the model wants to use tools
        if response.stop_reason == "end_turn":
            # Agent decided it's done -- extract final text
            return "".join(
                block.text for block in response.content if block.type == "text"
            )

        # Process tool calls
        messages.append({"role": "assistant", "content": response.content})
        tool_results = []
        for block in response.content:
            if block.type == "tool_use":
                result = execute_tool(block.name, block.input)
                tool_results.append(
                    {
                        "type": "tool_result",
                        "tool_use_id": block.id,
                        "content": result,
                    }
                )
        messages.append({"role": "user", "content": tool_results})

    return "Max iterations reached"


# Run it
answer = agent_loop("Check the weather in Tokyo and email me a summary at user@example.com")
print(answer)
```

## Example

A file-processing agent that reads a directory, finds CSVs, and summarizes each:

```python
tools = [
    {"name": "list_files", "description": "List files in a directory", ...},
    {"name": "read_file", "description": "Read a file's contents", ...},
    {"name": "write_summary", "description": "Write a summary to a file", ...},
]

answer = agent_loop("Find all CSV files in /data, summarize each, and write summaries to /output")
# The agent will: list_files -> read_file (for each CSV) -> write_summary -> stop
```

## Common Pitfalls

1. **No iteration limit** -- Always set `max_iterations`. Without it, a confused agent loops forever and burns tokens.
2. **Growing context** -- Every tool call adds to the message history. For long-running agents, summarize or truncate older messages.
3. **Missing error handling** -- If a tool throws an exception, catch it and return the error as a tool result so the agent can recover.
4. **Vague tool descriptions** -- The agent picks tools based on their `description` field. Be specific about what each tool does and when to use it.
5. **No stop condition guidance** -- Tell the agent in the system prompt when it should stop (e.g., "Reply with your final answer when the task is complete").

## Key Insight

An autonomous agent is just an LLM in a while loop: call the model, execute any tool requests, feed results back, repeat until the model says it is done.
