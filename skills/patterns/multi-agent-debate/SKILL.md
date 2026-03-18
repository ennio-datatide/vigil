---
name: multi-agent-debate
description: Build a multi-agent debate system where multiple LLM agents argue, share reasoning, and converge on a better answer. Use when you need higher accuracy through diverse perspectives, like fact-checking, complex reasoning, or decision-making where a single model might be biased or wrong.
---

# Multi-Agent Debate

> The pattern works in any language. Python shown for clarity.

## When to Use

- A single LLM response may be incorrect or biased
- The task benefits from multiple perspectives (reasoning, fact-checking, analysis)
- You want to improve accuracy without fine-tuning or complex prompting
- You need a consensus mechanism for high-stakes decisions
- **NOT** for tasks with one obvious correct approach -- a single LLM call suffices
- **NOT** for tasks requiring specialized tools -- use agent-loop or orchestrator-workers instead
- **NOT** for conversation routing -- use multi-agent-handoffs instead

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


def debate(
    question: str,
    n_agents: int = 3,
    n_rounds: int = 3,
    use_judge: bool = True,
) -> dict:
    """
    Run a multi-round debate between n_agents on a question.

    Each round:
    1. Every agent sees all other agents' previous responses
    2. Every agent produces a new response considering others' arguments
    3. After final round, a judge (or majority vote) picks the best answer
    """
    agent_personas = [
        "You are a careful, methodical thinker. You prioritize accuracy over speed.",
        "You are a creative, lateral thinker. You consider unconventional angles.",
        "You are a skeptical, critical thinker. You challenge assumptions and look for flaws.",
        "You are a practical, results-oriented thinker. You focus on what works.",
        "You are a thorough researcher. You value evidence and citations.",
    ]

    # Initialize: each agent gives an independent answer
    responses = {}  # {round: {agent_id: response}}

    for round_num in range(n_rounds):
        responses[round_num] = {}

        for agent_id in range(n_agents):
            if round_num == 0:
                # First round: independent responses
                prompt = f"Question: {question}\n\nProvide your answer with detailed reasoning."
            else:
                # Subsequent rounds: see other agents' responses
                other_responses = "\n\n".join(
                    f"Agent {other_id + 1}'s response:\n{responses[round_num - 1][other_id]}"
                    for other_id in range(n_agents)
                    if other_id != agent_id
                )
                prompt = (
                    f"Question: {question}\n\n"
                    f"Other agents' responses from the previous round:\n{other_responses}\n\n"
                    f"Your previous response:\n{responses[round_num - 1][agent_id]}\n\n"
                    f"Consider the other agents' arguments. You may update your answer if "
                    f"you find their reasoning convincing, or defend your position if you "
                    f"disagree. Provide your revised answer with reasoning."
                )

            response = llm(
                prompt=prompt,
                system=agent_personas[agent_id % len(agent_personas)],
            )
            responses[round_num][agent_id] = response
            print(f"Round {round_num + 1}, Agent {agent_id + 1}: {response[:100]}...")

    # Final round responses
    final_responses = responses[n_rounds - 1]

    if use_judge:
        # Judge picks the best answer
        all_final = "\n\n".join(
            f"Agent {aid + 1}:\n{resp}"
            for aid, resp in final_responses.items()
        )
        verdict = llm(
            prompt=f"""Question: {question}

After {n_rounds} rounds of debate, here are the final positions:

{all_final}

As the judge, determine:
1. Which agent(s) have the most correct/compelling answer
2. What the final consensus answer should be
3. Where agents still disagree and why

Respond with JSON:
{{
  "winner": <agent number>,
  "consensus_answer": "...",
  "confidence": 0.0-1.0,
  "reasoning": "...",
  "unresolved": ["..."]
}}""",
            system="You are an impartial judge. Evaluate arguments on their merits, evidence, and logical soundness.",
        )
        return {
            "verdict": json.loads(verdict),
            "rounds": n_rounds,
            "agents": n_agents,
            "debate_history": responses,
        }
    else:
        # No judge -- return all final responses
        return {
            "final_responses": final_responses,
            "rounds": n_rounds,
            "agents": n_agents,
        }


# Run it
result = debate(
    question="Should a startup use a microservices architecture from day one, or start with a monolith?",
    n_agents=3,
    n_rounds=2,
    use_judge=True,
)
print(f"\nVerdict: {json.dumps(result['verdict'], indent=2)}")
```

## Example

Fact-checking with debate -- multiple agents cross-examine a claim:

```python
result = debate(
    question="Is it true that the Great Wall of China is visible from space with the naked eye?",
    n_agents=3,
    n_rounds=2,
    use_judge=True,
)
# Round 1: Agent 1 says "yes, commonly known fact"
#           Agent 2 says "no, astronauts have debunked this"
#           Agent 3 says "depends on definition of 'space'"
# Round 2: Agent 1 revises: "After considering Agent 2's point about astronaut testimony..."
#           Agents converge on: "No, it's a myth. Not visible from low Earth orbit."
# Judge: confidence 0.95, consensus: the claim is false
```

## Common Pitfalls

1. **Groupthink** -- Agents converge too quickly on the first confident-sounding answer. Give agents distinct personas (skeptic, optimist, contrarian) to maintain diversity.
2. **Too many rounds** -- 2-3 rounds is usually sufficient. Beyond that, agents just rephrase their positions without new insights.
3. **No convergence** -- On genuinely ambiguous questions, agents may never agree. Set a round limit and use a judge to make the final call.
4. **Token cost explosion** -- With 3 agents and 3 rounds, you make 9 LLM calls plus 1 judge call. For cost-sensitive applications, use 2 agents and 2 rounds.
5. **Identical responses** -- If all agents use the same persona, they tend to give the same answer. Differentiate with distinct system prompts.

## Key Insight

Multi-agent debate improves LLM accuracy by forcing models to confront and respond to alternative viewpoints -- the same mechanism that makes peer review and adversarial collaboration effective for humans.
