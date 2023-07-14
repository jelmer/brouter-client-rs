use brouter_client::Brouter;
use brouter_client::Point;
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    profile: String,

    #[clap(long)]
    export_waypoints: bool,

    /// Name of the route
    #[clap(long)]
    name: Option<String>,
}

fn main() {
    let args = Args::parse();
    let router = Brouter::default();
    let gpx = router
        .broute(
            &[],
            &[],
            args.profile.as_str(),
            None,
            None,
            args.name.as_deref(),
            args.export_waypoints,
        )
        .unwrap();

    assert_eq!(gpx.routes.len(), 1);
    let route = &gpx.routes[0];

    println!("{:?}", route);
}
