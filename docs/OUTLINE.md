# The Chair: hardware and control notes

The Chair is a differential-drive ride-on robot based on three VEX V5 Brains. This page documents what the current Rust code expects. See the repository README for build steps, media, and the current project checklist.

![Three-Brain system topology](diagrams/system-topology.svg)

## System topology

The controller Brain owns driving decisions. It reads the steering wheel and two-button throttle, computes left and right motor voltage, supervises both drive nodes, and renders the HUD. Each drive Brain applies voltage to four motors, brakes on command loss, and reports motor speed and health.

| Node | Program | Slot | Link | Motors |
| --- | --- | ---: | --- | --- |
| Controller | `chair-controller` | 1 | Smart 2 to left; smart 3 to right | Steering motor on smart 1 |
| Left drive | `chair-left` | 2 | Smart 2 to controller | Smart 3–6, forward |
| Right drive | `chair-right` | 3 | Smart 2 to controller | Smart 3–6, reversed |

![Controller Brain ports](diagrams/controller-brain-ports.svg)

## Driver controls

Controller ADI A and B are digital inputs. Both must be high to request motion. The left drive Brain hosts shared controls: ADI C is an active-high E-stop, ADI D selects reverse/park/drive by thirds of a legacy potentiometer range, and ADI E sets maximum motor voltage.

The steering motor uses a blue cartridge. Its encoder supplies steering angle while limited motor voltage pushes the wheel toward center. Current geometry constants are:

| Parameter | Value |
| --- | ---: |
| Wheel diameter | 4 in |
| Wheelbase | 24 in |
| Track width | 24 in |
| Steering wheel to road wheel ratio | 3.2:1 |
| Full-lock turn radius | 42 in |
| Road-wheel center deadzone | 2 degrees |

Throttle is mixed as `left = throttle × (1 + steer)` and `right = throttle × (1 - steer)`, then both outputs are proportionally desaturated to the V5 voltage limit. Steering cannot move the chair when throttle is zero.

## Node protocol

Links run at 115,200 baud. Serde/Postcard packets are protected by CRC-32, COBS framed, and terminated with a zero byte. The controller retries handshakes every 250 ms, requests health once per second, and treats a health reply delayed by three seconds as a fault.

Drive nodes report average motor RPM every 50 ms. The controller converts each side to MPH and displays their average. A drive node brakes all motors when it has not received a voltage command for 250 ms.

## Safety behavior

Emergency stop is deliberately latched until every Brain reboots. It can be triggered by:

- local active-high E-stop input;
- motor port or telemetry failure;
- battery capacity below 20%, temperature above 45 °C, or current above 20 A;
- failed startup handshake or health timeout;
- serial, steering, or throttle I/O error.

Any trigger brakes the local drive motors and reports the stop to the controller. The controller stops steering feedback, sends emergency-stop packets to both drive nodes, and replaces the HUD with a reboot warning.

This software behavior does not replace a correctly wired physical emergency stop. Test all fault paths with the drive wheels raised.

## Build and upload

```sh
nix develop
cargo test --workspace
./upload.sh controller left right
```

The mdBook can be previewed with `mdbook serve docs`. Output goes to `target/docs-book`.
