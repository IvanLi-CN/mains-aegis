# Front panel UI docs

This directory consolidates the current confirmed front panel UI design view from specs.

## Scope

- Firmware screen UI (implemented now):
  - Design language (SoT): [design-language.md](design-language.md)
  - Component contracts: [component-contracts.md](component-contracts.md)
  - Visual regression checklist: [visual-regression-checklist.md](visual-regression-checklist.md)
  - Dashboard module design: [dashboard-design.md](dashboard-design.md)
  - Self-check module design: [self-check-design.md](self-check-design.md)
- Host-side UI (future implementation): reserved, not frozen in this directory yet
- Runtime behavior baseline: [../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md](../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md)
- Visual freeze baseline: [../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md](../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md)

## Assets

- Frozen renders (8): `assets/dashboard-b-*.png`, `assets/self-check-c-*.png`
- Module maps (2):
  - `assets/dashboard-b-module-map.png`
  - `assets/self-check-c-module-map.png`
  - Used in module-level docs (`dashboard-design.md`, `self-check-design.md`)
- Design-language previews (2):
  - `../../docs/specs/hg3dw-front-panel-visual-language/assets/color-preview.svg`
  - `../../docs/specs/hg3dw-front-panel-visual-language/assets/typography-preview.svg`
- All assets are `320x172` and offline-readable.

## Preview (representative final renders)

![Dashboard Variant B - STANDBY](assets/dashboard-b-standby-mode.png)
![Self-check Variant C - STANDBY idle](assets/self-check-c-standby-idle.png)

## Read order

1. [design-language.md](design-language.md)
2. [component-contracts.md](component-contracts.md)
3. [dashboard-design.md](dashboard-design.md)
4. [self-check-design.md](self-check-design.md)
5. [visual-regression-checklist.md](visual-regression-checklist.md)
6. Source specs for traceability:
   - [../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md](../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md)
   - [../../docs/specs/hg3dw-front-panel-visual-language/SPEC.md](../../docs/specs/hg3dw-front-panel-visual-language/SPEC.md)
   - [../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md](../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md)

## Notes

- `firmware/ui` is the stable entry for current confirmed firmware UI design.
- `docs/specs` remains the source of record for historical scope, milestones, and acceptance details.
- Visual style and token-level constraints are normalized in `design-language.md`; page docs reference it instead of redefining style terms.
- Asset synchronization rule: `firmware/ui/assets/` is the display source for current reviews; when visual baseline changes, update `firmware/ui/assets` and reference specs in the same PR.
- Historical reference images `dashboard-b-ac-mode.png` and `dashboard-b-batt-mode.png` stay only under `docs/specs/.../assets/`.
