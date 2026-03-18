---
name: tool-calling
description: Implement LLM tool calling (function calling) with JSON schemas, function execution, and result passing. Use when you need an LLM to interact with external systems, APIs, or databases through structured function calls.
---

# Tool Calling

> The pattern works in any language. Python shown for clarity.

## When to Use

- You need an LLM to call external APIs, query databases, or interact with systems
- You want structured, validated inputs from the LLM (not free-form text parsing)
- You need the LLM to choose which function to call from a set of options
- **NOT** for autonomous multi-step tasks -- use agent-loop instead (which builds on this)
- **NOT** for predefined sequences -- use prompt-chaining instead

## The Pattern

```python
import openai
import json

client = openai.OpenAI()

# 1. Define tools as JSON schemas (OpenAI format)
tools = [
    {
        "type": "function",
        "function": {
            "name": "search_products",
            "description": "Search the product catalog by query. Returns matching products with prices.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query, e.g. 'red running shoes'",
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return",
                        "default": 5,
                    },
                    "min_price": {
                        "type": "number",
                        "description": "Minimum price filter in USD",
                    },
                },
                "required": ["query"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "add_to_cart",
            "description": "Add a product to the user's shopping cart by product ID.",
            "parameters": {
                "type": "object",
                "properties": {
                    "product_id": {"type": "string"},
                    "quantity": {"type": "integer", "default": 1},
                },
                "required": ["product_id"],
            },
        },
    },
]

# 2. Map tool names to implementations
def search_products(query: str, max_results: int = 5, min_price: float = 0) -> str:
    # In production, this hits your actual database/API
    results = [
        {"id": "SKU-001", "name": f"Result for '{query}'", "price": 49.99},
        {"id": "SKU-002", "name": f"Another match for '{query}'", "price": 79.99},
    ]
    return json.dumps(results[:max_results])

def add_to_cart(product_id: str, quantity: int = 1) -> str:
    return json.dumps({"status": "added", "product_id": product_id, "quantity": quantity})

TOOL_DISPATCH = {
    "search_products": search_products,
    "add_to_cart": add_to_cart,
}

# 3. Single-turn tool call: send message, execute tools, return final answer
def call_with_tools(user_message: str) -> str:
    messages = [{"role": "user", "content": user_message}]

    response = client.chat.completions.create(
        model="gpt-4o",
        messages=messages,
        tools=tools,
    )

    message = response.choices[0].message

    # If no tool calls, return the text response directly
    if not message.tool_calls:
        return message.content

    # Execute each tool call
    messages.append(message)
    for tool_call in message.tool_calls:
        fn_name = tool_call.function.name
        fn_args = json.loads(tool_call.function.arguments)
        result = TOOL_DISPATCH[fn_name](**fn_args)
        messages.append(
            {
                "role": "tool",
                "tool_call_id": tool_call.id,
                "content": result,
            }
        )

    # Get final response with tool results in context
    final = client.chat.completions.create(
        model="gpt-4o",
        messages=messages,
        tools=tools,
    )
    return final.choices[0].message.content


answer = call_with_tools("Find me running shoes under $60 and add the cheapest to my cart")
print(answer)
```

## Example

A database query tool that lets the LLM write and execute SQL:

```python
tools = [{
    "type": "function",
    "function": {
        "name": "run_sql",
        "description": "Execute a read-only SQL query against the analytics database. Returns rows as JSON.",
        "parameters": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "SELECT query to execute"},
            },
            "required": ["query"],
        },
    },
}]

# The LLM generates: run_sql(query="SELECT COUNT(*) FROM orders WHERE date > '2025-01-01'")
# You execute it, return the result, and the LLM interprets it for the user
```

## Common Pitfalls

1. **Vague descriptions** -- The model selects tools based on the `description` field. "Do stuff" will not work. Be specific: "Search the product catalog by keyword. Returns up to N matching products with name, price, and ID."
2. **Missing argument validation** -- The LLM can hallucinate argument values. Validate inputs before executing (check IDs exist, sanitize SQL, enforce ranges).
3. **Not returning errors gracefully** -- If a tool fails, return a structured error message as the tool result instead of crashing. The LLM can often recover or try a different approach.
4. **Too many tools** -- Models degrade with 20+ tools. Group related operations into fewer tools with a `action` parameter, or use routing to select a subset.
5. **Forgetting parallel tool calls** -- Both OpenAI and Anthropic can return multiple tool calls in one response. Always iterate over all of them, not just the first.

## Key Insight

Tool calling is the fundamental building block of all agentic patterns: define a JSON schema so the LLM knows what it can call, execute the function locally, and pass the result back as context for the next response.
