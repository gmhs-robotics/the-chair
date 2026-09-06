# Development and verification

## Pinned environment

Use the repository Nix shell, not an unrelated global `cargo`:

```sh
nix develop
rustc --version
cargo v5 --version
./check.sh
```

The shell includes Rust source, rustfmt, Clippy, rust-analyzer, cargo-v5, cargo-audit, Python, Git, USB tools, ShellCheck and mdBook. Cargo.lock and flake.lock are checked in. Cargo dependencies are updated within upstream-compatible requirements; transitive major versions required by upstream crates are not forcibly overridden. Postcard's unused default `heapless-cas` feature is disabled, removing its unmaintained `atomic-polyfill` dependency.

Verified vexide HEAD on 2026-09-05: `403c4f92a94b88d878767ee0d778c0e689af6361`, 2026-07-30, tracking version 0.9.0. [Pinned source](https://github.com/vexide/vexide/tree/403c4f92a94b88d878767ee0d778c0e689af6361). Its SDK and host mock are 0.29.0-rc.1 and 0.2.0-rc.1. The firmware uses nightly-2026-09-04 (rustc 1.100.0-nightly), rather than the old 2025 nightly workaround.

## Current Rust target compatibility

Two upstream changes require explicit handling:

1. The current compiler target is `thumbv7a-vex-v5`; the old built-in `armv7a-vex-v5` is absent. Current cargo-v5 still defaults to the old target, so builds must pass an explicit target.
2. The current built-in target omits `singlethread=true`, while VEX std selects `no_threads` synchronization. Current std therefore refuses to compile with `Using no_threads implementation on a target with threads`.

`.cargo/thumbv7a-vex-v5-chair.json` is generated from the **pinned compiler's own target specification**, changing only `singlethread` to true. VEX std does not implement Rust threads. No CPU, ABI, memory map or linker script is rewritten. The [official Rust target definition](https://github.com/rust-lang/rust/blob/master/compiler/rustc_target/src/spec/mod.rs) explains that this flag controls `target_has_threads` and atomic lowering. All application concurrency here uses the single-thread vexide async runtime; do not introduce native threads or interrupt-shared Rust synchronization under this target. Master Display/touch and both drivetrain Displays own their peripherals inside retained `task::spawn` futures. They exchange copied views and pending actions through short `Rc<RefCell<_>>` borrows; no borrow survives an `.await`. Safety, control, UART servicing and motor commands remain in each node's ordered 10 ms async control loop. This preserves fail-closed ordering while removing display cadence from that loop. Cooperative tasks are not preemptive: a long synchronous SDK call can still delay every task, so loop-gap and command-lease stops remain required.

To reproduce the checked-in target after a toolchain update:

```sh
python3 tools/update-target.py
git diff -- .cargo/thumbv7a-vex-v5-chair.json
./check.sh
```

Do not regenerate from a different compiler and assume equivalence. Remove the workaround when upstream fixes the stock target and verify actual firmware linking again.

`build-std` is no longer global in Cargo configuration. Current nightly host tests otherwise mix rebuilt and prebuilt `core` and fail with duplicate lang items. The `cargo firmware` alias and `build.sh` explicitly supply `-Zjson-target-spec`, `-Zbuild-std=std,panic_abort` and `-Zbuild-std-features=compiler-builtins-mem` only for the V5 target.

## Build products

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo audit
cargo firmware --locked           # links all three ELF images
./build.sh                        # also produces uploadable BIN images through cargo-v5
```

Products are in `target/thumbv7a-vex-v5-chair/release/`:

- `chair-controller` and `chair-controller.bin`
- `chair-left` and `chair-left.bin`
- `chair-right` and `chair-right.bin`

Host tests use vexide's incomplete mocked SDK and deterministic pure control/protocol tests. They cover neutral/rearm interlocks, both E-stop paths, signed WHEEL throttle, signed CONTROLLER arcade input and pivoting, direction-change stopping, passenger stopping, controller loss, voltage limits, steering feedback, command ordering, corrupt frames, timeout latching, thermal thresholds and cumulative duty. They do not simulate real motor torque, radio latency, connector faults or stopping distance. Firmware linking catches target/ABI/SDK problems that a host-only check cannot catch.

## Upload

```sh
./upload.sh controller left right
```

This builds every requested program first, prompts for one USB Brain at a time, uses slots 1/2/3, and passes `--after none`. The script uploads the already linked ELF rather than letting cargo-v5 silently choose its obsolete default target. No firmware has to be flashed to run host checks. Start the master program manually before starting both children.

## HUD and documentation

The book is organized by task: `OUTLINE.md` is the entry point, `WIRING.md` owns the cable schedule, `OPERATING.md` owns the operator sequence, `HUD.md` explains display fixtures, and `SAFETY.md` owns software limits and protocol behavior. Keep physical test evidence in `COMMISSIONING.md` or a dated copy of its record.

The UI previews call the same `draw` function used on the Brain. Six fixtures cover startup, parked, WHEEL driving, CONTROLLER driving, motor overtemperature and stale link telemetry. Values are synthetic; host fonts and strokes approximate VEX rendering. Preview generation changes no runtime display behavior.

During hardware link diagnosis, the Master node cards show transmitted, received and malformed-frame byte counts. Tapping a node card restarts that handshake. The Master UI task samples touch at the physical Display refresh interval and keeps each action pending until the control loop drains it. Each drivetrain Brain's UI task renders its P21 link state and the same counters at 250 ms cadence. Displays are diagnostic and grant no motion authority.

Seven schematic SVGs cover topology, master ports, driving sequence, child lifecycle, stop paths, control signals and link timing. `tools/diagrams.py` owns their source. The diagrams identify software responsibilities and cable endpoints, not measured geometry or approval of the shared battery supply.

Regenerate and validate in the pinned shell:

```sh
python3 tools/diagrams.py
./render-hud.sh
mdbook build docs
python3 tools/check-docs.py
git diff --check
mkdir -p target/docs-previews
for figure in system-topology controller-brain-ports driving-flow drive-nodes stop-paths control-signals link-timing; do
  resvg --use-fonts-dir "$(dirname "$CHAIR_HUD_FONT")" \
    "docs/diagrams/$figure.svg" "target/docs-previews/$figure.png"
done
```

Inspect all seven schematic PNGs in `target/docs-previews/` and six UI PNGs in `target/hud-previews/` for clipped text, crossings and ambiguous endpoints. Check the physical Brain for final font fit. Every SVG includes an accessible title and description. Edit the generator or fixture, not generated SVG paths.

The book builds into `target/docs-book/`; `mdbook serve docs` opens a local preview server. For documentation edits, regeneration, the book build, the documentation checker and whitespace checks are sufficient. Run relevant host tests when changing fixture code; firmware behavior changes also require firmware checks.

The checker validates inline local links, rendered heading anchors, SVG XML/accessibility metadata and coverage of every current fault label. It does not fetch external URLs or validate physical behavior.

### Diagram source map

| Figure | Implementation to check when updating |
| --- | --- |
| System topology / master ports | `controller/src/main.rs`, `shared/src/lib.rs`, `shared/src/link/drivetrain.rs` |
| Driving sequence / control signals | `controller/src/control.rs`, `throttle.rs`, `steering.rs`, `main.rs` |
| Child lifecycle / link timing | `shared/src/link.rs`, `link/master.rs`, `link/child.rs`, `link/drivetrain.rs` |
| Stop paths | `shared/src/safety.rs`, `shared/src/lib.rs`, `controller/src/main.rs` |
| All HUD fixtures | `controller/src/hud.rs`; values supplied by `controller/src/main.rs` |

Source paths above are relative to `crates/`; short filenames continue the preceding directory. Physical assumptions belong in [wiring](WIRING.md), and measurements in [commissioning](COMMISSIONING.md).

## Source references

- [vexide motor API guide](https://vexide.dev/docs/motor/) and [pinned motor implementation](https://github.com/vexide/vexide/blob/403c4f92a94b88d878767ee0d778c0e689af6361/packages/vexide-devices/src/smart/motor.rs): Brake uses zero-velocity control; voltage control selects coast.
- [Pinned serial implementation](https://github.com/vexide/vexide/blob/403c4f92a94b88d878767ee0d778c0e689af6361/packages/vexide-devices/src/smart/serial.rs): buffer-capacity and nonblocking read/write behavior.
- [Pinned controller implementation](https://github.com/vexide/vexide/blob/403c4f92a94b88d878767ee0d778c0e689af6361/packages/vexide-devices/src/controller.rs): input state and connection checks, nonblocking screen calls.
- [VEX bumper wiring](https://kb.vex.com/hc/en-us/articles/360038026831-Using-the-V5-3-Wire-Bumper-Switch-v2-Limit-Switch): normally open, pressed LOW.
- [VEX motor timeouts](https://api.vex.com/v5/home/cpp/Motors_and_MotorControllers/motor_and_motor_group.html): position-movement timeouts are not a generic voltage-command watchdog; this firmware implements its own command lease.
