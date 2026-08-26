# Mains Aegis Management UI

The management UI lets an owner scan a UPS fleet and work on one device at a time. Its information hierarchy separates fleet-level state, device-level state, and route-specific work.

## Language

**Fleet Summary**:
The aggregate operational view of all managed devices, including device counts and cross-device alert counts. It belongs to the Fleet page.
_Avoid_: Global TopBar, device summary

**Device Overview**:
The complete operational snapshot for one UPS, including its running state, output, battery, hardware, and data state. It belongs to that device's Overview page.
_Avoid_: Device Context, shared status band

**Device Context**:
The minimal identity and current-state cue that lets an owner stay oriented while working on a device page. It is not an aggregate Fleet Summary or a complete Device Overview.
_Avoid_: Device Overview, fleet metrics
The current Web App has no cross-device notification or global alert entry.

**Device Page**:
A focused page for one device capability, such as Battery, Power, Alerts, Thermal, Settings, or API. It owns its route title and its capability-specific content.
_Avoid_: Generic device detail
