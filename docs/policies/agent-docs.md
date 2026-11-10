# Agent-doc style

Agent docs = `AGENTS.md` + `docs/{policies,decisions,notes,housekeeping,design}`. Optimise
tokens; human prose is not the goal. **Plans (`docs/plans/`) and human docs are excluded** -
see `documentation.md`.

## Write

- Imperative fragments. Drop articles, politeness, restated context, "this document".
- Tables > paragraphs. `key: value` lines. Arrows (`->`, `|`) > sentences.
- Reference path + section (`docs/design/api-surface.md`); never restate the rule.
- One fact per line. No diary prose; the dated filename carries the date.
- Abbreviations: RM=reference manual, DS=datasheet, ES=errata, §=section. Define new ones once.
- Keep numbers exact (bytes, IDs, line refs) - they are the evidence.

## Budget

- Estimator: 4 bytes/token. Caps in `documentation.md`; enforced by `tools/check_docs.py`.
- Over cap -> rewrite terser, or split into a second document. Never drop a fact.

## Human docs stay human

`requirements/**`, `src/**/requirements.yaml`, `DEBT.md`,
`RESOLVED.md`, `README.md`, `THIRD-PARTY.md`, `hil/WIRING.md`, `hil/*/README.md`: normal
prose, citations, readable by people. Do not compress them for tokens.
