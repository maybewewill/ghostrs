## Plans → always Antigravity

- `superpowers:writing-plans` and any implementation plan → always delegate with `/antigravity:delegate`
- Model: only `gemini-3.7-flash-high`
- Claude:
  - writes the brief (goal, constraints, path to spec/design)
  - runs `/antigravity:delegate --model gemini-3.7-flash-high --dir . "..."`
  - reviews the result and fixes only critical issues
- Do not write the plan on Claude
- Execution:
  - plans, scaffolding, implementation, tests, migrations, bulk edits, first-pass coding. → `/antigravity:delegate --model gemini-3.7-flash-high`
  - requirements, architecture decisions, task briefs, verification, final review, merge judgement. → Claude