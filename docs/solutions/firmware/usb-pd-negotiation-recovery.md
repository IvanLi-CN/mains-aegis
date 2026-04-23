# USB PD / PPS negotiation and hotplug recovery

## Scope

This note captures the implementation rules and debugging lessons for `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/usb_pd/` when the main USB-C sink must recover to PPS quickly after reset or cable replug.

It applies to the `FUSB302B`-based sink path together with `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/main.rs` and the charger integration in `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/output/`.

## The symptom pattern to recognize

If the board shows any of these behaviors, the PD recovery path is not healthy yet:

- replugging USB sometimes reaches PPS quickly, but other times stalls for many seconds
- the panel flips between `N/A`, `NO CC`, and `CAP?`
- the charger stays around `5V` even though the same source can provide PPS
- logs show repeated `attach -> no source caps -> hard reset/rearm` loops before PPS finally appears

These symptoms mean the board is usually attached physically, but the sink policy has not rebuilt a stable active contract.

## Root causes that mattered in practice

### 1. Partial RX frames were treated as bad frames

Do not read or flush a PD message just because RX FIFO is non-empty.

The recovery path became unstable when `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/usb_pd/mod.rs` consumed RX while `rx_non_empty=true` but `rx_ready=false`. That discards partial `Source_Capabilities` frames and turns normal negotiation into random retries.

**Rule:** only read RX when `rx_message_ready()` is true.

### 2. Protocol reset and physical detach must not be mixed

A peer `Hard Reset` during `contract=None` is not the same thing as a cable detach.

Treating protocol reset as physical detach creates repeated `detach -> attach -> recover` storms and large timing variance.

**Rule:** when attached but no contract exists yet, handle peer hard reset with `PD_RESET + wait for Source Caps`, not by tearing down the whole session immediately.

### 3. Auto recovery inside FUSB302 can fight firmware recovery

If hardware automatic protocol resets and firmware recovery logic both try to recover at the same time, the result is often repeated `contract=None` loops.

**Rule:** keep the recovery ownership in firmware. Hardware retry is fine, but protocol reset sequencing must be driven by the sink manager.

### 4. Fresh attach must not continue consuming stale IRQ state

After a full PHY reinit, the previous interrupt snapshot is already stale. Continuing to process it can immediately break the new session.

**Rule:** on fresh attach, reinitialize the PHY, poll/clear status as needed, then return to the next tick instead of consuming pre-reinit IRQ state.

### 5. Main-loop scheduling can be the real bottleneck

Even correct PD logic looks broken if `usb_pd.tick()` is serviced too slowly.

In this project the biggest timing improvement came from giving the no-contract negotiation window explicit priority in `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/main.rs`.

Without that, a configured `400ms` wait can effectively stretch to about one second because BMS, charger, fan, UI, and other periodic work delay the next PD tick.

## Implementation rules that proved reliable

### Attached + no-contract is a high-priority window

When all of the following are true:

- `attached == true`
- `contract == None`
- the source is still physically present

then the firmware should temporarily bias the main loop toward PD progress.

For this project the working pattern is:

- faster PHY poll interval during negotiation (`50ms` instead of the normal `250ms`)
- a short focused service window in `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/main.rs`
- continue feeding `usb_pd.tick()` and fresh IRQ deltas before running the heavier `power.tick()` path

That change is what turned the recovery from visibly random into stable seconds-level PPS reacquisition.

### Use a graded recovery ladder

The sink should escalate recovery in stages instead of jumping straight to repeated full reinit.

A practical ladder is:

1. wait briefly for `Source_Capabilities`
2. if still missing, start protocol-level recovery
3. only escalate to full PHY rearm when the prior stage times out

This avoids both extremes:

- waiting too long before trying recovery
- thrashing the PHY so hard that the source never gets a stable window to answer

### Defer recovery while RX is still incomplete

If partial RX activity has just been seen, do not immediately fire recovery actions such as hard reset or rearm.

**Rule:** give partial RX a short grace window before deciding the message is really absent.

### Ignore stale retry-fail state when it is not a new event

Sticky status bits are not the same as a fresh interrupt event.

**Rule:** use real retry-fail interrupts as the trigger, not stale status bits that can survive longer than the event that caused them.

## Recommended state-machine separation

Keep these concepts separate even if they are implemented in the same module:

- **Physical state**: detached vs attached
- **Negotiation state**: waiting for caps, requesting, waiting for PS_RDY, ready
- **Recovery state**: protocol reset in progress, hard-reset wait, full rearm fallback

The most important invariant is:

> Physical detach should only come from reliable detach evidence. Protocol-level failure should stay inside negotiation/recovery states.

## Observability that actually helps

The following signals were the most useful for debugging this path:

- `attach`
- `contract_pps`
- explicit `hard reset` / retry events
- `no source caps` timeout messages
- EEPROM breadcrumbs with ordered `seq` and `tick_100ms`

For timing analysis, the only number that matters is:

> first real attach after the event -> first `contract_pps`

Do not confuse that with the final retry step. The last retry may be fast even when the total user-visible recovery time was long.

## Validation recipe

When changing this area, validate in this order:

1. **Reset baseline**
   - Board already connected to the PPS source
   - Confirm `attach -> contract_pps` timing from logs
2. **Real cable replug**
   - Verify user-visible recovery to PPS
   - Then read breadcrumbs or logs to confirm the same path
3. **Failure-mode review**
   - Ensure there is no long-lived `CAP? + 5V` dead path
   - Ensure repeated replug does not drift back into wide timing variance

## Project-specific files to inspect first

- `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/usb_pd/mod.rs`
- `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/usb_pd/fusb302.rs`
- `/Users/ivan/Projects/Ivan/mains-aegis/firmware/src/main.rs`
- `/Users/ivan/Projects/Ivan/mains-aegis/docs/specs/hn29u-usb-c-pd-sink-pps/SPEC.md`

## What to avoid in future edits

- do not read RX frames before they are ready
- do not let stale interrupt snapshots survive a fresh reinit
- do not use protocol reset as a proxy for physical detach
- do not assume timeout constants mean anything if PD ticks are starved by the main loop
- do not claim timing success from the final retry step unless the first attach timestamp is also captured
