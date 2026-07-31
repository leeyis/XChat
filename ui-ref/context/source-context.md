# Source Project Context

This design-system workspace was created from an existing Open Design project. Treat the copied project files as the primary source evidence for the generated design system.

## Source project

- Source project id: 2e0eb181-e830-4623-8775-3dc474f49cf9
- Source project name: Web Prototype
- New design-system project id: ed504cc6-b960-4233-80be-1daf664b9eb1
- New design-system id: user:web-prototype-design-system
- Source skill id: (none)
- Source design system id: (none)

## Source metadata

```json
{
  "kind": "prototype",
  "nameSource": "prompt"
}
```

## Copied files

- brand-spec.md
- xchat-desktop-prototype.html
- image-1.png
- image.png

## Skipped files

- (none)

## Generation contract

- Read this file before editing design-system outputs.
- Read the copied files directly from the project workspace; they are source evidence, not generated design-system output.
- Preserve high-signal assets, source examples, UI surfaces, copy, tokens, typography, and interaction patterns from the copied project.
- Generate a reusable Open Design design-system package in this same project: DESIGN.md, README.md, SKILL.md, colors_and_type.css, context/provenance, focused preview cards, preserved assets/build/fonts when available, and ui_kits/app/.
- Before final response, run `"$OD_NODE_BIN" "$OD_BIN" tools connectors design-system-package-audit --path . --fail-on-warnings` and fix every actionable issue.
