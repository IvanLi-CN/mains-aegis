# CST816D datasheet (Hynitron)

## Source

- Datasheet PDF: https://files.waveshare.com/wiki/common/CST816D_datasheet_En_V1.3.pdf

## In-repo copy

This folder stores an offline-renderable copy of the datasheet:

- `CST816D.pdf`
- `CST816D.md` (**generated; do not edit by hand**)
- `images/`

## Notes

- The V1.3 datasheet does not explicitly state whether the `IRQ` pin is open-drain or push-pull. If you intend to share the interrupt line with other devices (wired-OR), verify the `IRQ` electrical type on the real module or obtain an authoritative vendor statement first.
