# RFC 0000: (Title)

- **Status:** Draft | Under Review | Accepted | Rejected | Withdrawn
- **Author:** (your name / GitHub handle)
- **Created:** YYYY-MM-DD
- **Last updated:** YYYY-MM-DD
- **Tracking issue:** #(number, once created)

## Summary

One paragraph explaining the proposal.

## Motivation

Why are we doing this? What problem does this solve? What use cases does it enable?

Include concrete examples of code that is currently painful or impossible to write.

## Detailed Design

### Syntax

```rad
// Show the proposed syntax with realistic examples
```

### Semantics

Describe the runtime behavior. Cover:

- What happens at compile time vs. runtime
- How this interacts with the type checker
- How this interacts with ECS / pipelines / events

### Examples

```rad
// A complete, runnable example showing the feature in context
```

## Drawbacks

- Why might we *not* want to do this?
- What complexity does this add to the language?
- Does this make RAD harder to learn?

## Alternatives

What other designs were considered? Why were they rejected?

## Impact

### On existing code

- Does this break any existing programs?
- If so, what is the migration path?

### On the Three Laws

- Does this affect ECS, pipelines, or events?
- Does this preserve the purity guarantees of pipelines?
- Does this maintain deterministic system execution?

### On tooling

- Does the LSP need updates?
- Does the formatter need updates?
- Does the playground need updates?

## Unresolved Questions

- What parts of the design are still open?
- What do we need to learn from implementation?

## Implementation Plan

1. (lexer/parser changes)
2. (checker changes)
3. (core/vm changes)
4. (conformance tests)
5. (documentation)

---

## How to Submit an RFC

1. Copy this template into a new proposal page or pull-request description as `NNNN-short-name` (use the next available number)
2. Fill in the sections above
3. Open a pull request titled `RFC NNNN: Short Title`
4. The PR description should link to any related issues
5. RFCs are discussed in PR comments — expect 1–2 weeks of review
6. A maintainer will merge (accepted) or close (rejected) with rationale
