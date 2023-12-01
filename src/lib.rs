use lazy_regex::regex;
use log::info;
use reqwest::blocking::Client;
use reqwest::Url;
use std::io::BufReader;

// See https://github.com/abrensch/brouter/blob/77977677db5fe78593c6a55afec6a251e69b3449/brouter-server/src/main/java/btools/server/request/ServerHandler.java#L17

#[derive(Debug, Clone)]
pub enum Nogo {
    Point {
        point: Point,
        radius: f64,
        weight: Option<f64>,
    },
    Line {
        points: Vec<Point>,
        weight: Option<f64>,
    },
    Polygon {
        points: Vec<Point>,
        weight: Option<f64>,
    },
}

#[derive(Debug, Clone)]
pub struct Point {
    lat: f64,
    lon: f64,
}

impl From<geo_types::Point> for Point {
    fn from(p: geo_types::Point<f64>) -> Self {
        Point {
            lat: p.y(),
            lon: p.x(),
        }
    }
}

impl From<Point> for geo_types::Point<f64> {
    fn from(p: Point) -> Self {
        geo_types::Point::new(p.lon, p.lat)
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidGpx(String),
    Http(reqwest::Error),
    MissingDataFile(String),
    NoRouteFound(isize),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidGpx(s) => write!(f, "Invalid GPX: {}", s),
            Error::Http(e) => write!(f, "HTTP error: {}", e),
            Error::MissingDataFile(s) => write!(f, "Missing data file: {}", s),
            Error::NoRouteFound(i) => write!(f, "No route found: {}", i),
        }
    }
}

impl Point {
    pub fn new(lat: f64, lon: f64) -> Self {
        Point { lat, lon }
    }

    pub fn lat(&self) -> f64 {
        self.lat
    }

    pub fn lon(&self) -> f64 {
        self.lon
    }
}

pub struct Brouter {
    client: Client,
    base_url: Url,
}

impl Default for Brouter {
    fn default() -> Self {
        Self::new("http://localhost:17777")
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TurnInstructionMode {
    #[default]
    None = 0,
    AutoChoose = 1,
    LocusStyle = 2,
    OsmandStyle = 3,
    CommentStyle = 4,
    GpsiesStyle = 5,
    OruxStyle = 6,
    LocusOldStyle = 7,
}

impl Brouter {
    pub fn new(base_url: &str) -> Self {
        Brouter {
            client: Client::new(),
            base_url: Url::parse(base_url).unwrap(),
        }
    }

    pub fn upload_profile(&self, profile: &str, data: Vec<u8>) -> Result<(), Error> {
        let url = self
            .base_url
            .join("brouter/profile")
            .unwrap()
            .join(profile)
            .unwrap();

        let response = self
            .client
            .post(url)
            .body(data)
            .send()
            .map_err(Error::Http)?;

        response.error_for_status().map_err(Error::Http).map(|_| ())
    }

    pub fn broute(
        &self,
        points: &[Point],
        nogos: &[Nogo],
        profile: &str,
        alternativeidx: Option<u8>,
        timode: Option<TurnInstructionMode>,
        name: Option<&str>,
        export_waypoints: bool,
    ) -> Result<gpx::Gpx, Error> {
        let lon_lat_strings: Vec<String> = points
            .iter()
            .map(|p| format!("{},{}", p.lon(), p.lat()))
            .collect();

        info!("Planning route along {:?}", points);

        let lonlats = lon_lat_strings.join("|");

        let nogos_string: String = nogos
            .iter()
            .filter_map(|nogo| match nogo {
                Nogo::Point {
                    point,
                    radius,
                    weight,
                } => {
                    let mut v = vec![point.lon(), point.lat(), *radius];
                    if let Some(weight) = weight {
                        v.push(*weight);
                    }
                    Some(
                        v.iter()
                            .map(|f| f.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                }
                Nogo::Polygon { .. } => None,
                Nogo::Line { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("|");

        let polylines = nogos
            .iter()
            .filter_map(|nogo| match nogo {
                Nogo::Point { .. } => None,
                Nogo::Polygon { .. } => None,
                Nogo::Line { points, weight } => {
                    let mut v = points
                        .iter()
                        .flat_map(|p| vec![p.lon(), p.lat()])
                        .collect::<Vec<_>>();
                    if let Some(weight) = weight {
                        v.push(*weight);
                    }
                    Some(
                        v.iter()
                            .map(|f| f.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("|");

        let polygons = nogos
            .iter()
            .filter_map(|nogo| match nogo {
                Nogo::Point { .. } => None,
                Nogo::Line { .. } => None,
                Nogo::Polygon { points, weight } => {
                    let mut v = points
                        .iter()
                        .flat_map(|p| vec![p.lon(), p.lat()])
                        .collect::<Vec<_>>();
                    if let Some(weight) = weight {
                        v.push(*weight);
                    }
                    Some(
                        v.iter()
                            .map(|f| f.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("|");

        let mut url = self.base_url.join("brouter").unwrap();

        url.query_pairs_mut()
            .append_pair("lonlats", &lonlats)
            .append_pair("profile", profile)
            .append_pair("format", "gpx");

        if let Some(alternativeidx) = alternativeidx {
            assert!((0..=3).contains(&alternativeidx));

            url.query_pairs_mut()
                .append_pair("alternativeidx", alternativeidx.to_string().as_str());
        }

        if let Some(timode) = timode {
            url.query_pairs_mut()
                .append_pair("timode", (timode as i32).to_string().as_str());
        }

        if !polygons.is_empty() {
            url.query_pairs_mut().append_pair("polygons", &polygons);
        }

        if !nogos_string.is_empty() {
            url.query_pairs_mut().append_pair("nogos", &nogos_string);
        }

        if !polylines.is_empty() {
            url.query_pairs_mut().append_pair("polylines", &polylines);
        }

        if export_waypoints {
            url.query_pairs_mut().append_pair("exportWaypoints", "1");
        }

        if let Some(name) = name {
            url.query_pairs_mut().append_pair("trackname", name);
        }

        let response = self
            .client
            .get(url)
            .timeout(std::time::Duration::from_secs(3600))
            .send()
            .map_err(Error::Http)?
            .error_for_status()
            .map_err(Error::Http)?;

        let text = response.bytes().map_err(Error::Http)?.to_vec();

        if let Some(m) = regex!("datafile (.*) not found\n"B).captures(text.as_slice()) {
            return Err(Error::MissingDataFile(
                String::from_utf8_lossy(m.get(1).unwrap().as_bytes()).to_string(),
            ));
        }

        if let Some(m) = regex!("no track found at pass=([0-9]+)\n"B).captures(text.as_slice()) {
            return Err(Error::NoRouteFound(
                String::from_utf8_lossy(m.get(1).unwrap().as_bytes())
                    .to_string()
                    .parse()
                    .unwrap(),
            ));
        }

        let gpx: gpx::Gpx = gpx::read(BufReader::new(text.as_slice())).map_err(|_e| {
            Error::InvalidGpx(String::from_utf8_lossy(text.as_slice()).to_string())
        })?;

        Ok(gpx)
    }
}
