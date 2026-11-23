# ADR-016: FerroHAL's first supported user (2026-11-23)

ADR-016 - Status: promoted

Promoted to: [scope.md](../design/scope.md) (design).

Supersedes [ADR-005](archive/2026/2026-10-31-ferrowasp-integration.md).

## Context

ADR-005 recorded how one firmware, FerroWasp, would move onto FerroHAL: the order of
its swap, its boards, what it uses each peripheral for, and how it pins its dependency.
Those are that firmware's implementation details, not FerroHAL design. One decision in
it binds FerroHAL.

## Decision

1. FerroWasp is FerroHAL's first-class supported user: its needs drive what FerroHAL
   builds and in what order.
2. The toaster rule still holds. A need lands as a generic capability, named, specified
   and tested in FerroHAL's own terms, never as logic for one firmware. FerroHAL never
   depends on a user's firmware and holds no transition code for one.

## Rejected

- Keeping a firmware's migration plan in a FerroHAL decision record: the plan is the
  firmware's.

## Consequences

- `docs/design/scope.md` already states both rules; no code changes.
- ADR-005 is archived as superseded.
