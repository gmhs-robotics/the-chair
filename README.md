# The Chair

Three VEX V5 Brains, eight drive motors, a resistive steering wheel and an optional V5 controller. Rust firmware for a school robotics demonstrator.

![Chair wiring overview](docs/diagrams/system-topology.svg)

**Software checks pass; occupied operation is not validated.** Begin with [commissioning](docs/COMMISSIONING.md). Wheel direction, gearing, steering travel, stopping distance and the reported shared battery supply still require physical verification.

## Documentation

| Need | Guide |
| --- | --- |
| Connect the Brains, motors and controls | [Wiring](docs/WIRING.md) |
| Start, arm, drive and stop | [Operator guide](docs/OPERATING.md) |
| Read the screen or troubleshoot arming | [Dashboard](docs/HUD.md) |
| Understand limits, leases and stop behavior | [Software design](docs/SAFETY.md) |
| Validate the finished chair | [Commissioning](docs/COMMISSIONING.md) |
| Build and upload in Nix | [Development](docs/DEVELOPMENT.md) |

## Controls at a glance

| Action | WHEEL | CONTROLLER |
| --- | --- | --- |
| Speed / direction | P17 positive forward, negative reverse; hold ADI A | Left stick Y forward/reverse |
| Steering | Turn wheel | Right stick X |
| Enable | Left wheel button, ADI A | Press A once to arm; no hold |
| Coast stop | Release left button; stays armed | Center both sticks; stays armed |
| Latched E-stop | Right wheel button, ADI B | Rider right button or controller B |
| Arm / park / mode | Display ARM / PARK / MODE | A / L1 / X, or display |

A connected controller's B and L1 also work in WHEEL. Releasing WHEEL drive-enable or returning controller sticks to zero coasts while remaining armed. PARK, L1, the rider stop in CONTROLLER, or a mode change coasts and disarms. Latched faults still request electrical Brake. CONTROLLER uses signed arcade mixing: left Y drives forward/reverse and right X turns, including turning in place at zero throttle. P17 is ignored in CONTROLLER. No automatic radio fallback.

Master P19 connects LEFT P21; master P20 connects RIGHT P21. Each child drives bottom motors on P4/P5 and top motors on P6/P7. Top and bottom pairs use opposite motor polarity because they are gear-coupled. Master P21 is steering; P18 is radio. The master and rider controls must remain attached in both modes.

## Build

```sh
nix develop
./check.sh
./upload.sh controller left right
```

The check includes host tests, Clippy, RustSec, all three V5 binaries, shell checks and the documentation book. Upload builds first, uses one USB Brain at a time, and does **not** start programs. Slots: master 1, left 2, right 3.

The pinned environment uses vexide [`403c4f9`](https://github.com/vexide/vexide/commit/403c4f92a94b88d878767ee0d778c0e689af6361), Rust `nightly-2026-09-04` and cargo-v5 0.12.1. These are the revisions verified on 2026-09-05, not a promise that future upstream HEAD stays unchanged. See [target compatibility](docs/DEVELOPMENT.md#current-rust-target-compatibility).

## Dashboard preview

![Firmware-generated HUD fixture](docs/diagrams/hud-driving.svg)

![Controller-mode HUD fixture](docs/diagrams/hud-controller.svg)

Current WHEEL and CONTROLLER UI, rendered by firmware drawing code with synthetic telemetry. WHEEL throttle comes from the P17 Rotation Sensor. [All six display states and controls](docs/HUD.md).

## Repository

- `crates/controller/`: inputs, driving state machine, steering feedback and HUD.
- `crates/left/`, `crates/right/`: entry points for the shared drive runtime.
- `crates/shared/`: communication, telemetry, limits and local stopping.
- `docs/`: field guide, diagrams, commissioning and development.
- `hardware/`: existing CAD, scans and gearing calculations.
- `tools/diagrams.py`: reproducible documentation figures.

[Earlier single-Brain driving video](media/video/driving-demo.mp4) · [Project devlogs](https://stardance.hackclub.com/projects/4170). Historical media does not validate this firmware.

MIT. See [LICENSE](LICENSE).
