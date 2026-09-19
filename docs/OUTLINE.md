# The Chair field guide

Three VEX V5 Brains coordinate a differential-drive chair demonstrator. WHEEL mode uses signed P17 forward/reverse. CONTROLLER uses signed arcade sticks with in-place turning. The steering-wheel master reads rider controls or a V5 controller; the two identical drive runtimes supervise four motors each.

**Status:** firmware builds and host checks are available. Occupied operation and the shared battery arrangement have not been validated.

![System overview](diagrams/system-topology.svg)

## Choose your task

| Task | Read |
| --- | --- |
| Connect the finished chair | [Wiring and hardware assumptions](WIRING.md) |
| Start, drive, brake or change mode | [Operator guide](OPERATING.md) |
| Understand the dashboard or WAIT READY | [Dashboard guide](HUD.md) |
| Investigate a latched stop | [Fault messages and recovery](FAULTS.md) |
| Review limits and fault behavior | [Software design and stops](SAFETY.md) |
| Prepare for first physical operation | [Commissioning and test record](COMMISSIONING.md) |
| Build, upload or change firmware | [Development and verification](DEVELOPMENT.md) |

## The two driving modes

**WHEEL:** center the wheel and P17 before startup; when ON, hold the left rider button, rotate the throttle forward, and turn the wheel. The wheel applies limited resistive feedback.

**CONTROLLER:** use left stick Y for forward/reverse, and right stick X for steering or an in-place turn. No button hold is required. P17 is ignored. The physical wheel follows steering while ON. The rider's left button coasts while held and right button requests electrical Brake while held.

The right rider button and connected controller B request electrical Brake while held in both modes without latching. Release, stop and hold controls neutral for 500 ms to resume. Neither replaces an independently reviewed means of stopping the final hardware.

## Configuration scope

The master is retained in both modes. The firmware does not support removing it and driving with two Brains. Programs must be started manually, master first. Wiring diagrams identify ports and responsibilities; they are not mechanical dimensions or electrical approval.

The guide separates measured facts from current calibration assumptions. Follow the unloaded commissioning sequence before using a rider or relying on estimated MPH.
