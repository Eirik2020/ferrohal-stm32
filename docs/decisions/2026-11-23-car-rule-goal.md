# ADR-015: The car rule is a goal, and a G5 exception is a decision record (2026-11-23)

ADR-015 - Status: promoted

Promoted to: [goals.md](../../requirements/goals.md) (requirements).

## Context

G4 held the verification pillar in one sentence and named reqtool's mechanism, file
hashing. The car rule, which says how little a change re-owes, lived only in a design
document, below the goals in authority. Agents followed a script over it and ran benches
that no change called for. G5 asked for "the owner's dated approval" of an exception, a
form nothing defined.

## Decision

- The car rule is part of G4: a part is verified once against its own requirements, an IP
  version proven on one chip is proven for every chip that carries it, and a change to a
  part that still fulfils the requirements set on it re-owes only that part.
- G4 does not say how a change is tracked. That is reqtool's design.
- A G5 exception needs a decision record and a default-off feature gate.

## Rejected

- Leaving the car rule in the design document alone: every requirement's verification
  depends on it, so it belongs where requirements derive from.
- Keeping hashing in G4: the goal would have to change whenever reqtool's method does.

## Consequences

- `docs/design/requirements-system.md` keeps the detail (layers, tiers, the boards) and
  derives from G4.
- No bench run is owed: no code changes.
