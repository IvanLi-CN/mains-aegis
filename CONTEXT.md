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

**Install Recommendation**:
A non-disruptive, browser-local invitation to install Mains Aegis Web. It offers a native install action only when the browser makes one available, or an iOS add-to-home-screen guide where that action is unavailable.
_Avoid_: Automatic install, install prompt

**Install Eligibility**:
The current browser state in which Mains Aegis Web is not already installed and can either offer a native PWA installation action or an iOS installation guide.
_Avoid_: PWA support, browser support

**Demo Mode**:
A mock-only browser view used to preview Mains Aegis management workflows without live hardware. It is not an installable product state because an installed app starts the normal management console.
_Avoid_: Installed demo, production mode
