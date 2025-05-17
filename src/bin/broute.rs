use brouter_client::Brouter;
use brouter_client::Nogo;
use brouter_client::Point;
use clap::Parser;

#[derive(Parser, Clone, Debug)]
struct Args {
    #[arg(long)]
    profile: String,

    #[arg(long)]
    export_waypoints: bool,

    /// Name of the route
    #[arg(long)]
    name: Option<String>,

    /// Nogo points
    #[arg(long)]
    nogos: Option<Vec<String>>,

    #[arg(name = "POINTS")]
    points: Vec<String>,
}

fn main() {
    let args = Args::parse();
    let router = Brouter::local().unwrap();
    let gpx = router
        .broute(
            args.points
                .iter()
                .map(|p| {
                    let mut parts = p.split(',');
                    let lon = parts.next().unwrap().parse::<f64>().unwrap();
                    let lat = parts.next().unwrap().parse::<f64>().unwrap();
                    Point::new(lat, lon)
                })
                .collect::<Vec<_>>()
                .as_slice(),
            args.nogos
                .unwrap_or_default()
                .iter()
                .map(|p| {
                    let p = p.split_once(':').unwrap();
                    let mut parts = p.1.split(',').collect::<Vec<_>>();
                    match p.0 {
                        "point" => {
                            let mut parts = parts.into_iter();
                            let lon = parts.next().unwrap().parse::<f64>().unwrap();
                            let lat = parts.next().unwrap().parse::<f64>().unwrap();
                            let radius = parts.next().unwrap().parse::<f64>().unwrap();
                            let weight = parts.next().map(|p| p.parse::<f64>().unwrap());
                            Nogo::Point {
                                point: Point::new(lat, lon),
                                radius,
                                weight,
                            }
                        }
                        "line" => {
                            // if the number of items in parts is odd, then the last entry is the
                            // weight
                            let weight = if parts.len() % 2 == 1 {
                                Some(parts.pop().unwrap().parse::<f64>().unwrap())
                            } else {
                                None
                            };
                            let points = parts
                                .chunks(2)
                                .map(|p| {
                                    let lat = p[1].parse::<f64>().unwrap();
                                    let lon = p[0].parse::<f64>().unwrap();
                                    Point::new(lat, lon)
                                })
                                .collect::<Vec<_>>();
                            Nogo::Line { points, weight }
                        }
                        "polygon" => {
                            // if the number of items in parts is odd, then the last entry is the
                            // weight
                            let weight = if parts.len() % 2 == 1 {
                                Some(parts.pop().unwrap().parse::<f64>().unwrap())
                            } else {
                                None
                            };
                            let points = parts
                                .chunks(2)
                                .map(|p| {
                                    let lat = p[1].parse::<f64>().unwrap();
                                    let lon = p[0].parse::<f64>().unwrap();
                                    Point::new(lat, lon)
                                })
                                .collect::<Vec<_>>();
                            Nogo::Polygon { points, weight }
                        }
                        _ => panic!("Unknown nogo type"),
                    }
                })
                .collect::<Vec<_>>()
                .as_slice(),
            args.profile.as_str(),
            None,
            None,
            args.name.as_deref(),
            args.export_waypoints,
        )
        .unwrap();

    println!("{:?}", gpx);
}
