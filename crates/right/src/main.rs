use chair_shared::link::{
    Node,
    drivetrain::{Drivetrain, DrivetrainNode},
    master::MasterNode,
};
use vexide::prelude::*;

#[vexide::main]
async fn main(peripherals: Peripherals) {
    let node_master: Node<DrivetrainNode, MasterNode> = Node::open(peripherals.port_2).await;

    Drivetrain::new(
        [
            Motor::new(peripherals.port_3, Gearset::Green, Direction::Reverse),
            Motor::new(peripherals.port_4, Gearset::Green, Direction::Reverse),
            Motor::new(peripherals.port_5, Gearset::Green, Direction::Reverse),
            Motor::new(peripherals.port_6, Gearset::Green, Direction::Reverse),
        ],
        node_master,
    )
    .run()
    .await;
}
