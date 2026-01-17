use color_eyre::Result;
use ec_demo::app::App;

fn main() -> Result<()> {
    color_eyre::install()?;

    let terminal = ratatui::init();

    #[cfg(feature = "mock")]
    let source = ec_demo::mock::Mock::default();

    #[cfg(feature = "acpi")]
    let source = ec_demo::acpi::Acpi::default();

    #[cfg(feature = "serial")]
    let source = {
        // Revisit: Quick and easy for grabbing command line args, but when
        // debug tab PR is merged this can switch to clap for arg parsing
        let mut args = std::env::args().skip(1);
        let path = args.next().expect("Serial port path must be provided");
        let flow_control = args.next().expect("Flow control mode must be provided");
        let flow_control = match flow_control.as_str() {
            "hw" => true,
            "none" => false,
            _ => panic!("Flow control mode must be either `hw` or `none`"),
        };

        let baud = args
            .next()
            .unwrap_or("115200".to_string())
            .parse::<u32>()
            .expect("Serial baud rate must be a u32");

        ec_demo::serial::Serial::new(path.as_str(), baud, flow_control)
    };

    App::new(source).run(terminal)
}
