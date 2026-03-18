---
name: multi-agent-handoffs
description: Build a system where specialized agents hand off conversations to each other based on context. Use when different parts of a conversation need different expertise, like a customer service system routing between billing, technical support, and sales agents.
---

# Multi-Agent Handoffs

> The pattern works in any language. Python shown for clarity.

## When to Use

- A conversation needs different expertise at different points (e.g., triage -> specialist)
- You want specialized agents with focused system prompts and tools
- The routing decision should be made by the agents themselves, not hardcoded
- Users interact with what feels like one system but multiple agents handle different parts
- **NOT** for independent parallel tasks -- use parallelization instead
- **NOT** for a central coordinator delegating work -- use orchestrator-workers instead
- **NOT** for adversarial/collaborative reasoning -- use multi-agent-debate instead

## The Pattern

```python
import openai
import json

client = openai.OpenAI()


class Agent:
    """An agent with a persona, instructions, and tools (including handoff tools)."""

    def __init__(self, name: str, instructions: str, tools: list[dict] = None, tool_fns: dict = None):
        self.name = name
        self.instructions = instructions
        self.tools = tools or []
        self.tool_fns = tool_fns or {}


def handoff_to(agent: Agent):
    """Create a handoff function that switches to another agent."""
    def transfer(**kwargs):
        return agent
    return transfer


# 1. Define specialized agents
triage_agent = Agent(
    name="Triage",
    instructions="You are a triage agent. Determine what the user needs and hand off to the right specialist. "
                 "Use transfer_to_billing for payment/invoice issues. "
                 "Use transfer_to_support for technical problems. "
                 "Use transfer_to_sales for new purchases or upgrades.",
    tools=[
        {"type": "function", "function": {"name": "transfer_to_billing", "description": "Hand off to billing agent for payment and invoice issues", "parameters": {"type": "object", "properties": {}}}},
        {"type": "function", "function": {"name": "transfer_to_support", "description": "Hand off to technical support for product issues", "parameters": {"type": "object", "properties": {}}}},
        {"type": "function", "function": {"name": "transfer_to_sales", "description": "Hand off to sales for purchases and upgrades", "parameters": {"type": "object", "properties": {}}}},
    ],
)

billing_agent = Agent(
    name="Billing",
    instructions="You are a billing specialist. Help with invoices, payments, and refunds. "
                 "You can look up invoices and process refunds. "
                 "If the issue is not billing-related, transfer back to triage.",
    tools=[
        {"type": "function", "function": {"name": "lookup_invoice", "description": "Look up an invoice by ID", "parameters": {"type": "object", "properties": {"invoice_id": {"type": "string"}}, "required": ["invoice_id"]}}},
        {"type": "function", "function": {"name": "process_refund", "description": "Process a refund for an invoice", "parameters": {"type": "object", "properties": {"invoice_id": {"type": "string"}, "amount": {"type": "number"}}, "required": ["invoice_id", "amount"]}}},
        {"type": "function", "function": {"name": "transfer_to_triage", "description": "Hand off back to triage for re-routing", "parameters": {"type": "object", "properties": {}}}},
    ],
)

support_agent = Agent(
    name="Support",
    instructions="You are technical support. Diagnose and resolve product issues. "
                 "If the issue is not technical, transfer back to triage.",
    tools=[
        {"type": "function", "function": {"name": "check_system_status", "description": "Check if a system/service is operational", "parameters": {"type": "object", "properties": {"service": {"type": "string"}}, "required": ["service"]}}},
        {"type": "function", "function": {"name": "create_ticket", "description": "Create a support ticket", "parameters": {"type": "object", "properties": {"summary": {"type": "string"}, "priority": {"type": "string", "enum": ["low", "medium", "high"]}}, "required": ["summary"]}}},
        {"type": "function", "function": {"name": "transfer_to_triage", "description": "Hand off back to triage for re-routing", "parameters": {"type": "object", "properties": {}}}},
    ],
)

sales_agent = Agent(
    name="Sales",
    instructions="You are a sales agent. Help with purchases, upgrades, and pricing. "
                 "If the issue is not sales-related, transfer back to triage.",
    tools=[
        {"type": "function", "function": {"name": "get_pricing", "description": "Get pricing for a product/plan", "parameters": {"type": "object", "properties": {"product": {"type": "string"}}, "required": ["product"]}}},
        {"type": "function", "function": {"name": "transfer_to_triage", "description": "Hand off back to triage for re-routing", "parameters": {"type": "object", "properties": {}}}},
    ],
)

# 2. Wire up handoff functions
triage_agent.tool_fns = {
    "transfer_to_billing": handoff_to(billing_agent),
    "transfer_to_support": handoff_to(support_agent),
    "transfer_to_sales": handoff_to(sales_agent),
}
billing_agent.tool_fns = {
    "transfer_to_triage": handoff_to(triage_agent),
    "lookup_invoice": lambda invoice_id: json.dumps({"id": invoice_id, "amount": 99.99, "status": "paid"}),
    "process_refund": lambda invoice_id, amount: json.dumps({"status": "refunded", "amount": amount}),
}
support_agent.tool_fns = {
    "transfer_to_triage": handoff_to(triage_agent),
    "check_system_status": lambda service: json.dumps({"service": service, "status": "operational"}),
    "create_ticket": lambda summary, priority="medium": json.dumps({"ticket_id": "TKT-001", "summary": summary}),
}
sales_agent.tool_fns = {
    "transfer_to_triage": handoff_to(triage_agent),
    "get_pricing": lambda product: json.dumps({"product": product, "price": "$49/mo", "enterprise": "$199/mo"}),
}


# 3. The handoff loop
def run_conversation(starting_agent: Agent, user_messages: list[str]):
    agent = starting_agent
    messages = []

    for user_msg in user_messages:
        messages.append({"role": "user", "content": user_msg})
        print(f"\nUser: {user_msg}")

        # Inner loop: process tool calls until agent produces a text response
        while True:
            response = client.chat.completions.create(
                model="gpt-4o",
                messages=[{"role": "system", "content": agent.instructions}] + messages,
                tools=agent.tools if agent.tools else openai.NOT_GIVEN,
            )
            msg = response.choices[0].message

            if not msg.tool_calls:
                # Agent produced a text response
                messages.append({"role": "assistant", "content": msg.content})
                print(f"[{agent.name}]: {msg.content}")
                break

            # Process tool calls
            messages.append(msg)
            for tc in msg.tool_calls:
                fn_name = tc.function.name
                fn_args = json.loads(tc.function.arguments) if tc.function.arguments else {}
                result = agent.tool_fns[fn_name](**fn_args)

                # Check if the result is a handoff (an Agent object)
                if isinstance(result, Agent):
                    print(f"[Handoff: {agent.name} -> {result.name}]")
                    agent = result
                    messages.append({"role": "tool", "tool_call_id": tc.id, "content": f"Transferred to {agent.name}"})
                else:
                    messages.append({"role": "tool", "tool_call_id": tc.id, "content": result})


# Run it
run_conversation(
    triage_agent,
    [
        "I was charged twice for my last invoice",
        "The invoice ID is INV-2024-001",
        "Can you refund the duplicate charge of $99.99?",
    ],
)
# Triage -> detects billing issue -> hands off to Billing
# Billing -> looks up invoice -> processes refund -> responds
```

## Example

A conversation that crosses multiple agents:

```
User: "I was charged twice for my last invoice"
[Triage]: Detects billing issue
[Handoff: Triage -> Billing]
[Billing]: "I'll look into that. What's your invoice ID?"

User: "INV-2024-001"
[Billing]: Calls lookup_invoice("INV-2024-001") -> finds $99.99 paid
[Billing]: "I found invoice INV-2024-001 for $99.99. I'll process the refund."
[Billing]: Calls process_refund("INV-2024-001", 99.99) -> refunded
[Billing]: "Done! Your refund of $99.99 has been processed."
```

## Common Pitfalls

1. **Losing context on handoff** -- The full message history must transfer with the handoff. If you reset messages, the new agent has no idea what the user already said.
2. **Handoff loops** -- Agent A hands to B, B immediately hands back to A. Add a cooldown or track recent handoffs to prevent ping-pong.
3. **Too many agents** -- Start with 2-3 specialists. Each new agent adds routing complexity. Merge agents with overlapping responsibilities.
4. **No escape hatch** -- Always include a "transfer back to triage" tool so conversations can be re-routed if the wrong specialist was chosen.
5. **Invisible handoffs** -- Users should know when they are being transferred. Include a message like "Let me connect you with our billing team" before the handoff.

## Key Insight

Multi-agent handoffs reduce to a single mechanism: a tool call that returns a new agent instead of a string result -- the orchestration loop swaps the active agent, and the conversation continues with the new agent's instructions and tools.
