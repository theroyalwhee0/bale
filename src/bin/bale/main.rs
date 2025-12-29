use clap::Parser;

#[derive(Parser)]
#[command(
    version = concat!(
        env!("CARGO_PKG_VERSION"),
        "\n",
        "  Built:   ",
        env!("BUILD_DATETIME_ISO"),
        "\n",
        "  Profile: ",
        env!("EXPECT_PROFILE"),
    ),
    about
)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
    bale::run();
}
