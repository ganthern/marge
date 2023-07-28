use clap::Parser;

#[derive(Parser, Debug)]
struct AppArgs {
    #[arg(long)]
    config: String
}

fn main() {
    let args = AppArgs::parse();

    println!("Hello, {}!", args.config);
}
