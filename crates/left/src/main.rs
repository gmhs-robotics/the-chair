use chair_shared::link::{
    Node,
    drivetrain::{Drivetrain, DrivetrainControls, DrivetrainNode},
    master::MasterNode,
};
use vexide::prelude::*;

#[vexide::main]
async fn main(peripherals: Peripherals) {
    let node_master: Node<DrivetrainNode, MasterNode> = Node::open(peripherals.port_2).await;

    Drivetrain::new(
        [
            Motor::new(peripherals.port_3, Gearset::Green, Direction::Forward),
            Motor::new(peripherals.port_4, Gearset::Green, Direction::Forward),
            Motor::new(peripherals.port_5, Gearset::Green, Direction::Forward),
            Motor::new(peripherals.port_6, Gearset::Green, Direction::Forward),
        ],
        node_master,
    )
    .with_controls(DrivetrainControls::new(
        peripherals.adi_c,
        peripherals.adi_d,
        peripherals.adi_e,
    ))
    .run()
    .await;
}
