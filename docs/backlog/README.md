# Backlog

This is work queued for after the current set of tasks. Read it in this order:

1. **[01-feature-brief.md](01-feature-brief.md):**
   - the maintainer's feature list, each item checked against the code, with an approach and a size;
   - a suggested build order at the end.
2. **[02-release-readiness.md](02-release-readiness.md):** what to fix before the first public test build. It's ranked
   by what a first tester would hit in their first hour.
3. **[03-gaps-found-in-the-tour.md](03-gaps-found-in-the-tour.md):** problems found while scripting the demo films.
   - Some are fixed on branch `claude/demo-player` (demo player; multi-select Properties edits).
   - Others are fixed on branch `claude/starter-house-polish`.
   - Review and merge those two branches first.

**Suggested start:** items 1–3 of the brief's order, all small:
- a true top view;
- distinct roof-shape icons;
- Excel take-off export.

Then the release-readiness items marked "must fix".

**The guiding rule is to do it better, not bigger:**
- one general mechanism over many special tools;
- direct manipulation in 3D and 2D;
- every control recognisable at a glance.
