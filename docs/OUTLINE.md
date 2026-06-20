# The Chair Hardware Description

This document describes the electrical and control hardware for `The Chair`, a
VEX V5 based go-kart chair. The project uses three VEX V5 Brains.

The implemented code currently wires the controller Brain's steering motor and
HUD. The left and right drive-node programs exist as binaries, but they are
placeholders. Items marked as planned are part of the hardware design but are
not yet enforced by the current Rust code.

![Three-brain system topology](diagrams/system-topology.svg)

## System Topology

The electronics are split into one controller Brain and two drive Brains:

| Node              | Program            | V5 slot | Role                                                    |
| ----------------- | ------------------ | ------: | ------------------------------------------------------- |
| Controller Brain  | `chair-controller` |       1 | Master node, steering input, HUD, safety, node commands |
| Left Drive Brain  | `chair-left`       |       2 | Node for the four left-side drive motors                |
| Right Drive Brain | `chair-right`      |       3 | Node for the four right-side drive motors               |

The controller Brain is the only node that should make driving decisions. The
left and right drive Brains should act as output nodes: they receive a requested
side voltage or brake command, apply it to their four motors, and report status
back to the controller Brain.

The draft design reserves controller smart port 5 for the left drive node link
and controller smart port 10 for the right drive node link. The current code
does not instantiate either link yet, and the physical inter-Brain communication
method must be verified before wiring. V5 smart ports are normally used for V5
smart devices, so do not assume generic Brain-to-Brain networking without a
defined protocol and cable plan.

## Controller Brain

![Controller Brain ports](diagrams/controller-brain-ports.svg)

The controller Brain is mounted where the driver can reach the steering wheel
and see the V5 screen. It owns the driver-facing controls and computes the
commands that will eventually be sent to the two drive nodes.

| Interface            | Port          | Status      | Description                                                                |
| -------------------- | ------------- | ----------- | -------------------------------------------------------------------------- |
| Steering wheel motor | Smart port 1  | Implemented | V5 motor used as both steering shaft encoder and resistive return actuator |
| HUD                  | V5 display    | Implemented | Shows the normalized steering value as `Steer: <value>`                    |
| Ignition buttons     | ADI A, ADI B  | Planned     | Two steering-wheel buttons pressed together to arm or start the cart       |
| Emergency stop input | ADI C         | Planned     | Driver E-stop input; the safety state should propagate to every node       |
| Speed limit knob     | ADI D         | Planned     | Potentiometer for requested speed limit or throttle scaling                |
| Drift/handbrake knob | ADI E         | Planned     | Potentiometer for drift/handbrake behavior                                 |
| Left node link       | Smart port 5  | Planned     | Reserved for controller-to-left-node communication                         |
| Right node link      | Smart port 10 | Planned     | Reserved for controller-to-right-node communication                        |

### Steering Hardware

The steering shaft is coupled to a V5 motor on smart port 1 with the blue
gearset and forward direction. The motor position is read as the steering wheel
angle. When the wheel is inside the center deadzone, the controller zeroes the
motor position and brakes the motor. When the wheel leaves the deadzone, the
controller applies return voltage toward center.

Current steering geometry constants:

| Parameter                            |   Value |
| ------------------------------------ | ------: |
| Wheelbase                            |   24 in |
| Track width                          |   24 in |
| Steering wheel to road wheel ratio   |   3.2:1 |
| Full-lock turn radius                |   42 in |
| Road-wheel center deadzone           |   2 deg |
| Approximate steering-wheel deadzone  | 6.4 deg |
| Approximate steering-wheel full lock | 100 deg |

The steering output is normalized to `-1.0..=1.0`. The active curve uses a small
deadzone, cubic expo of `0.2`, and steering output slew rate of `7.0`. Return
force is limited to `32%` of V5 maximum motor voltage and slews at `60.0` units
per second. The `damping` argument in the feedback constructor is currently
validated but not applied by the implementation.

### Driver Inputs And Safety

The ignition, E-stop, speed, and drift/handbrake controls are planned ADI
inputs. The intended safety rule is simple: any local E-stop or low-battery
condition on any Brain should force all drive nodes into a braking or disabled
state. The current code does not yet read these inputs or distribute safety
state.

The touchscreen lock/ignition flow is also planned. It should be software-only:
the display can require a password or arming gesture before the controller sends
drive commands, but hardware E-stop behavior must not depend on a touchscreen
flow.

## Left And Right Drive Brains

![Drive node motor layout](diagrams/drive-nodes.svg)

Each drive Brain is intended to control four motors on one side of the chair.
The left Brain controls the left-side motors; the right Brain controls the
right-side motors. The two sides are symmetric except for their physical
mounting and motor direction.

Planned drive-node contract:

| Command         | Meaning                                                           |
| --------------- | ----------------------------------------------------------------- |
| Voltage command | Apply an exact requested voltage to all four motors on that side  |
| Brake command   | Put all four motors on that side into the requested V5 brake mode |
| Disable command | Stop applying drive voltage and enter the safety state            |
| Status report   | Return battery/safety/link status to the controller Brain         |

The current `chair-left` and `chair-right` binaries only print their node names.
They do not yet bind motors, ports, communication, braking, telemetry, or
battery-fault behavior.

## Motion Control Responsibility

The controller Brain should compute left and right drive requests from the
steering output, requested speed limit, and drift/handbrake setting. The drive
Brains should not independently decide vehicle behavior; they should only apply
commands and enforce local safety.

Planned motion calculations include:

- Left/right voltage calculation from steering geometry and speed request.
- Brake-mode commands for normal braking and emergency stop.
- Drift/handbrake behavior based on steering angle, speed limit, and the planned
  drift input.
- "Gear shifting" as a software speed-range change, not a physical gearbox. In
  this design it means increasing the minimum available maximum speed while
  still allowing wheel voltage to exceed the current speed when a maneuver
  requires it.

## Build And Docs

The docs are an mdBook. From this repository, serve them with:

```console
mdbook serve docs
```

The book output is configured to build under `target/docs-book`, which is
already ignored by Git.
