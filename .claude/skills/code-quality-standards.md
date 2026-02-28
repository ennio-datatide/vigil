---
name: code-quality-standards
description: Use when reviewing code, planning implementations, or evaluating architectural decisions — reference for SOLID, clean code, TDD, and architecture patterns used in praefectus
---

# Code Quality Standards

## Overview

Reference document for code quality principles enforced during reviews. Use this to evaluate implementations against established engineering standards.

## SOLID Principles

### S — Single Responsibility

Each module/class/function does ONE thing. If you need "and" to describe it, split it.

```typescript
// BAD: SessionManager handles sessions AND sends notifications
// GOOD: SessionManager handles sessions, TelegramNotifier handles notifications
```

**Review check:** Can you describe the module's purpose in one sentence without "and"?

### O — Open/Closed

Open for extension, closed for modification. Add behavior by adding code, not changing existing code.

```typescript
// GOOD: New agent types added via AgentType enum + new spawner strategy
// BAD: if/else chain in agent-spawner that grows with every new type
```

**Review check:** Does adding a feature require modifying existing, tested code?

### L — Liskov Substitution

Subtypes must be substitutable for their base types without breaking behavior.

**Review check:** If you swap an implementation, do callers still work correctly?

### I — Interface Segregation

Don't force clients to depend on methods they don't use. Keep interfaces focused.

```typescript
// BAD: One massive AppContext with everything
// GOOD: Services injected individually (app.settingsService, app.notifier)
```

**Review check:** Does the consumer use all the methods/properties of what it receives?

### D — Dependency Inversion

Depend on abstractions, not concretions. High-level modules shouldn't depend on low-level details.

```typescript
// GOOD: TelegramNotifier accepts fetchFn parameter (can be mocked)
// BAD: TelegramNotifier hardcodes global fetch with no way to test
```

**Review check:** Can you test this in isolation? If not, dependencies are too concrete.

## Clean Code

### Naming

- Functions: verb + noun (`buildApp`, `resolveConfig`, `installHooks`)
- Booleans: `is`/`has`/`should` prefix (`isDefault`, `hasWorktree`)
- Avoid generic names: `data`, `info`, `item`, `result`, `handler`
- Name length proportional to scope (short in tight loops, descriptive at module level)

### Functions

- Do one thing
- Max 3 parameters (use an options object beyond that)
- No side effects hidden in the name (a `get` function shouldn't write to DB)
- Early returns over deep nesting
- Max ~30 lines (guideline, not a hard rule)

### Comments

- Don't comment WHAT (the code shows that)
- Comment WHY when the reason isn't obvious
- Delete commented-out code — git has history
- TODO comments must include context, not just "TODO: fix this"

### Error Handling

- Fail fast, fail loud
- Don't swallow errors silently (`catch {}`)
- Return meaningful error messages
- Validate at system boundaries (API inputs, external data)
- Trust internal code — don't over-validate between your own modules

## TDD — Test-Driven Development

### The Cycle

```
RED → GREEN → REFACTOR
```

1. **RED:** Write a test that fails for the right reason
2. **GREEN:** Write the minimum code to make it pass
3. **REFACTOR:** Improve structure without changing behavior

### Test Quality

- Test behavior, not implementation details
- One assertion per logical concept
- Tests should be fast, isolated, and deterministic
- Name tests as specifications: `should reject invalid input`, `should retry on timeout`
- Use `buildApp()` with temp directories for isolated test environments (praefectus pattern)
- Use `app.inject()` for HTTP tests — no real server needed

### What to Test

- Happy paths (does it work?)
- Edge cases (empty inputs, null values, boundary conditions)
- Error paths (what happens when things fail?)
- Integration points (do modules work together?)

### What NOT to Test

- Implementation details (private methods, internal state)
- Third-party library internals
- Obvious constructors or getters
- Things already tested by the framework

## Architecture Patterns (Praefectus)

### Plugin Architecture (Fastify)

Routes and services registered as Fastify plugins. Each route file is self-contained.

```typescript
const settingsRoute: FastifyPluginAsync = async (app) => {
  app.get('/api/settings/telegram', async () => { ... });
  app.put('/api/settings/telegram', async (request, reply) => { ... });
};
```

**Review check:** Is the new route registered as a plugin? Does it follow existing patterns?

### Service Layer

Business logic lives in `services/`, not in route handlers. Routes parse input, call services, return output.

```
Route (parse + validate) → Service (business logic) → DB/External
```

**Review check:** Is the route handler thin? Is business logic testable independently?

### Shared Schemas (Zod)

API contracts defined once in `packages/shared`, used by server and CLI. Runtime validation at API boundaries.

**Review check:** Is the new schema in shared? Is it validated at the API boundary?

### Event Bus

Loose coupling between components via EventEmitter. Services emit events, other services listen.

**Review check:** Are components communicating through the event bus rather than direct references?

## Code Review Checklist

Use this during reviews:

### Correctness
- [ ] Does it do what it's supposed to?
- [ ] Are edge cases handled?
- [ ] Are errors handled appropriately?

### Design
- [ ] Does it follow SOLID principles?
- [ ] Is it consistent with existing patterns?
- [ ] Is there unnecessary complexity?

### Testing
- [ ] Are there tests? Do they test behavior, not implementation?
- [ ] Do tests cover happy path AND error paths?
- [ ] Bug fixes: is there a regression test? (see bug-driven-testing skill)

### Security
- [ ] Input validated at boundaries?
- [ ] No secrets in code or logs?
- [ ] No injection vulnerabilities (SQL, command, XSS)?

### Performance
- [ ] No obvious N+1 queries or unnecessary loops?
- [ ] No blocking operations in hot paths?

### Maintainability
- [ ] Clear naming?
- [ ] No dead code?
- [ ] Could a new contributor understand this?
