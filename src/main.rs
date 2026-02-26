use roboplc::controller::prelude::*;
use roboplc_middleware::{Message, Variables};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    roboplc::setup_panic();
    roboplc::configure_logger(roboplc::LevelFilter::Info);

    roboplc::set_simulated();

    let mut controller: Controller<Message, Variables> = Controller::new();

    controller.register_signals(std::time::Duration::from_secs(5))?;

    controller.block();
    Ok(())
}
