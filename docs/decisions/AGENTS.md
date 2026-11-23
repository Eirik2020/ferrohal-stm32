# Decision records - agent rules

These rules bind every decision recorded in this repository; only the owner
outranks them. They apply wherever a decision is written, not only in this folder.

1. A decision is recorded here, as an ADR, and nowhere else. Working documents
   (`docs/design/`, `docs/policies/`, `hil/`, `requirements/`) carry no decision
   labels: no `[Decided]`, `[Proposal]`, `[Superseded]`, and no date or "owner"
   attribution. They state the rule as it holds now.
2. One date per ADR, in its title. A second date is a breach.
3. An ADR is at most 3000 bytes and is never split. Design detail belongs in the
   design document the ADR is promoted to.
4. Sections: Context, Decision, Rejected, Consequences. No others.
5. The owner signs off the exact wording. Before an ADR is committed, paste its
   full text in your reply for review. A summary is not a review.
