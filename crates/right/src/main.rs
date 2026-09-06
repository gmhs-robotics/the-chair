use vexide::prelude::*;
#[vexide::main]
async fn main(peripherals: Peripherals) {
    chair_shared::link::drivetrain::run_node(peripherals).await;
}
