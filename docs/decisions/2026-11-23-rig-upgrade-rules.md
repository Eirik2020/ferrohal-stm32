# ADR-020: The rules a rig upgrade must fit (2026-11-23)

ADR-020 - Status: accepted

## Context

The rig is what records evidence, and its gaps are listed with the upgrades that would
close them. An upgrade is the owner's to pick. The suggestions need stated limits.

## Decision

1. One permanent, hands-free set-up. A procedure that needs a jumper moved, a cable
   swapped or a button pressed by hand is not an answer; a switch the rig drives itself
   is.
2. No soldering for now, headers included. It is allowed later; a suggestion that needs
   it says so.
3. Nothing is approved until the owner picks it: an upgrade enters the queue only then.

## Rejected

No alternative was recorded.

## Consequences

- `hil/RIG-IMPROVEMENTS.md` states the rules; `hil/CAPABILITIES.md` holds the queue.
- No code changes, and no bench run is owed.
