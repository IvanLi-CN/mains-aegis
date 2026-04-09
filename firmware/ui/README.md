# Front panel UI docs

This directory is the current source of truth for the firmware front panel screen system.

## Scope

- Firmware screen UI (implemented now):
  - Design language (SoT): [design-language.md](design-language.md)
  - Component contracts: [component-contracts.md](component-contracts.md)
  - Visual regression checklist: [visual-regression-checklist.md](visual-regression-checklist.md)
  - Dashboard home design: [dashboard-design.md](dashboard-design.md)
  - Dashboard detail + manual charge design: [dashboard-detail-design.md](dashboard-detail-design.md)
  - Self-check + BQ40 overlay design: [self-check-design.md](self-check-design.md)
- Public handbook entry:
  - [../../docs-site/docs/design/front-panel-screen-pages.md](../../docs-site/docs/design/front-panel-screen-pages.md)
  - [../../docs-site/docs/design/front-panel-ui-design.md](../../docs-site/docs/design/front-panel-ui-design.md)
- Runtime behavior baseline:
  - [../../docs/specs/g2kte-dashboard-live-after-self-check/SPEC.md](../../docs/specs/g2kte-dashboard-live-after-self-check/SPEC.md)
- Visual freeze baseline:
  - [../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md](../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md)
- BQ40 result dialog baseline:
  - [../../docs/specs/5cvrj-bq40-self-check-result-dialogs/SPEC.md](../../docs/specs/5cvrj-bq40-self-check-result-dialogs/SPEC.md)

## Current page system

- Boot / recovery: `SELF CHECK` (Variant C) + `BQ40Z50` recovery overlays/results
- Steady state: Dashboard home (Variant B)
- Drill-down: `Output / Thermal / Cells / Charger / Battery Flow`
- Charger control: `MANUAL CHARGE` under `Charger Detail`
- Runtime truth: `SELF CHECK` is a startup / recovery page, not the steady-state landing page; when self-check clears and the first runtime snapshot is ready, the screen transitions into Dashboard

## Assets

- Frozen renders under `assets/`:
  - `dashboard-b-*.png`
  - `dashboard-b-detail-*.png`
  - `dashboard-detail-icons.png`
  - `self-check-c-*.png`
  - `manual-charge-*.png`
- Module maps:
  - `assets/dashboard-b-module-map.png`
  - `assets/self-check-c-module-map.png`
- Design-language previews:
  - `../../docs/specs/hg3dw-front-panel-visual-language/assets/color-preview.svg`
  - `../../docs/specs/hg3dw-front-panel-visual-language/assets/typography-preview.svg`
- All promoted screen assets are `320x172` and offline-readable.

## Preview (representative final renders)

![Dashboard Variant B - STANDBY](assets/dashboard-b-standby-mode.png)
![Self-check Variant C - STANDBY idle](assets/self-check-c-standby-idle.png)
![Manual Charge - Default](assets/manual-charge-default.png)

## Read order

1. [design-language.md](design-language.md)
2. [component-contracts.md](component-contracts.md)
3. [dashboard-design.md](dashboard-design.md)
4. [dashboard-detail-design.md](dashboard-detail-design.md)
5. [self-check-design.md](self-check-design.md)
6. [visual-regression-checklist.md](visual-regression-checklist.md)
7. Source specs for traceability:
   - [../../docs/specs/g2kte-dashboard-live-after-self-check/SPEC.md](../../docs/specs/g2kte-dashboard-live-after-self-check/SPEC.md)
   - [../../docs/specs/5cvrj-bq40-self-check-result-dialogs/SPEC.md](../../docs/specs/5cvrj-bq40-self-check-result-dialogs/SPEC.md)
   - [../../docs/specs/hg3dw-front-panel-visual-language/SPEC.md](../../docs/specs/hg3dw-front-panel-visual-language/SPEC.md)
   - [../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md](../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md)

## Notes

- `firmware/ui` is the stable entry for the current confirmed screen runtime docs.
- Public handbook pages should deep-link here or to committed specs, not to legacy gallery-style doc pages.
- Visual style and token-level constraints are normalized in `design-language.md`; page docs reference it instead of redefining style terms.
- Asset synchronization rule: `firmware/ui/assets/` is the current promoted asset source for screen docs; when a visual baseline changes, update promoted assets and the referencing docs in the same PR.
- Historical screenshots may remain in old specs for traceability, but active docs should not depend on those legacy spec paths.
