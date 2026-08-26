# Assign summaries to their owning pages

Fleet Summary belongs only to Fleet, and the complete Device Overview belongs only to a device's Overview page. Other device pages begin with their own route title and capability-specific content; narrow layouts retain only compact Device Context so repeated summaries cannot consume the first viewport. The current Web App has no cross-device notification or global alert entry.

## Considered Options

- Keep both summaries in a shared layout: rejected because device pages lose their first viewport to context that is not specific to the task.
- Hide shared summaries only with narrow-screen CSS: rejected because aggregate and route-specific information remain semantically duplicated and page headings still depend on the shared layout.
