---
name: hiroute-ui-quality
description: Verify HiRoute Desktop layout, text wrapping, visual hierarchy and form states using production styles and observable geometry. Apply to UI implementation or layout repairs; not backend-only work or a request to discuss a design without implementing it.
---

# HiRoute UI quality

Implement the confirmed interaction as a readable, consistent, usable real page.
Wrapping, hierarchy and state layout are delivery quality; compilation or control
presence is insufficient. Routine layout choices within the user's design need
no extra approval.

## Find the production owner

Trace the live entry/component/style imports first. Desktop `src/main.tsx` loads
`src/occami/styles.css` and its `src/occami/styles/` imports; do not place fixes in
obsolete `src/style.css` based on its name. Test pages load production styles,
not a parallel style set hiding defects. Reuse actual Dialog/form/button components
and semantic color/spacing/radius/text variables. Check common layout assumptions,
e.g. horizontal `.field-label`; override vertically only in the affected form.
Fix dimensions only when the product requires them; controls/columns must shrink.

## Select coverage by the change

Read the [Desktop test map](../../../apps/desktop/tests/README.md) and use
[HiRoute validation](../hiroute-validation/SKILL.md) to select and execute checks.
Record selected combinations:

- Layout/forms: default and smallest supported window from current `tauri.conf.json`
  (currently 1200×813 and 755×600). Distinguish content viewport from native window.
- Text/control widths: Chinese and English, relevant long labels/values/errors.
  Desktop currently fixes text scale at 100%; do not simulate unsupported settings.
  Preference changes check that old 150%/200% saves normalize to 100%. Add scale
  levels only when the product supports them again.
- Interaction: affected expanded/collapsed, loading, empty, error and saving states.
  Reliable unchanged-state evidence need not be mechanically repeated.
- Themes: affected light/dark. Existing semantic variables plus layout-only change
  normally need geometry in one theme and contrast sampling in the other.

## Inspect real pages and protect regressions

Run production browser scenarios first. Native save/restart/authorization/real
requests require a capable Codex host and [HiRoute CU](../hiroute-cu/SKILL.md).
Claude does not load that Codex-only skill; report native acceptance pending.
Mocked IPC is not a business result. Save and actually inspect selected screenshots:

- Labels do not become vertical letters under control pressure; long text wraps
  naturally, primary/helper hierarchy is clear and field/card/button gaps agree.
- Controls align, long values stay within cards/dialogs, and hidden overflow does
  not conceal truncated content.
- Small windows scroll while main actions stay visible/clickable; footer cannot
  cover the last field/error. Expand/cancel/retry preserves context.
- Visible keyboard focus moves appropriately into/out of editing; disabled/saving
  states do not expose duplicate submission.

For a reproduced layout defect, add the smallest observable assertion under
production styles: label/control bounds, overflow or footer viewport. Show old
code fails and the repair passes. CSS-string matching/test counts do not prove
coverage. Geometry does not replace visual screenshot inspection.

Record revision, entry, viewport, language, text scale and state in the Issue/PR,
with selected screenshots/results and browser/native limits. Keep raw evidence
local unless explicitly suitable for publication. Documentation-only follow-ups
do not rerun unaffected product checks. Personal commands/credentials/host config
are not user examples. This skill grants no delegation, writes, merge or publishing
beyond the current task and repository rules.
