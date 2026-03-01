# Front panel UI docs

This directory consolidates the current confirmed front panel UI design view from specs.

## Scope

- Firmware screen UI (implemented now):
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
- All assets are `320x172` and offline-readable.

## Preview

![Dashboard Variant B Module Map](assets/dashboard-b-module-map.png)
![Self-check Variant C Module Map](assets/self-check-c-module-map.png)

## Read order

1. [dashboard-design.md](dashboard-design.md)
2. [self-check-design.md](self-check-design.md)
3. Source specs for traceability:
   - [../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md](../../docs/specs/7n4qd-mcu-self-check-live-panel/SPEC.md)
   - [../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md](../../docs/specs/6qrjs-front-panel-industrial-ui-preview/SPEC.md)

## Notes

- `firmware/ui` is the stable entry for current confirmed firmware UI design.
- `docs/specs` remains the source of record for historical scope, milestones, and acceptance details.
- Asset synchronization rule: `firmware/ui/assets/` is the display source for current reviews; when visual baseline changes, update `firmware/ui/assets` and reference specs in the same PR.
- Historical reference images `dashboard-b-ac-mode.png` and `dashboard-b-batt-mode.png` stay only under `docs/specs/.../assets/`.
