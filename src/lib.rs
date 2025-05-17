#![deny(missing_docs)]
//! A Rust client for the BRouter server.
//!
//! Example usage:
//!
//! ```rust,no_run
//! use brouter_client::Brouter;
//!
//! let brouter = Brouter::local().unwrap();
//! let points = vec![
//!    brouter_client::Point::new(52.5200, 13.4050), // Berlin
//!    brouter_client::Point::new(48.8566, 2.3522), // Paris
//! ];
//!
//! let route = brouter.broute(
//!   &points,
//!   &[],
//!   "trekking",
//!   None,
//!   None,
//!   Some("My Route"),
//!   false, // Export waypoints
//!   ).unwrap();
//!  ```

use lazy_regex::regex;
use log::info;
use reqwest::blocking::Client;
use reqwest::Url;
use std::io::BufReader;

#[cfg(feature = "local")]
pub mod local;

// See https://github.com/abrensch/brouter/blob/77977677db5fe78593c6a55afec6a251e69b3449/brouter-server/src/main/java/btools/server/request/ServerHandler.java#L17

#[derive(Debug, Clone)]
/// A description of some area that should be avoided
pub enum Nogo {
    /// A point with a radius
    Point {
        /// The point
        point: Point,

        /// The radius in meters
        radius: f64,

        /// Weight of the point
        weight: Option<f64>,
    },
    /// A line
    Line {
        /// A list of points that make up the line
        points: Vec<Point>,

        /// A weight
        weight: Option<f64>,
    },
    /// A polygon
    Polygon {
        /// A list of points that make up the polygon
        points: Vec<Point>,

        /// A weight
        weight: Option<f64>,
    },
}

impl Nogo {
    /// Return the weight of the nogo
    pub fn weight(&self) -> Option<f64> {
        match self {
            Nogo::Point { weight, .. } => *weight,
            Nogo::Line { weight, .. } => *weight,
            Nogo::Polygon { weight, .. } => *weight,
        }
    }
}

#[derive(Debug, Clone)]
/// A point with latitude and longitude
pub struct Point {
    /// Latitude
    lat: f64,

    /// Longitude
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
/// An error that can occur when using the BRouter client
pub enum Error {
    /// An error that occurs when the GPX file is invalid
    InvalidGpx(String),

    /// An error that occurs when the HTTP request fails
    Http(reqwest::Error),

    /// An error that occurs when the data file is missing
    MissingDataFile(String),

    /// An error that occurs when no route is found
    NoRouteFound(isize),

    /// An error that occurs when the pass times out
    PassTimeout {
        /// The pass number
        pass: String,

        /// The timeout in seconds
        timeout: String,
    },

    /// Another error
    Other(String),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidGpx(s) => write!(f, "Invalid GPX: {}", s),
            Error::Other(e) => write!(f, "Error: {}", e),
            Error::Http(e) => write!(f, "HTTP error: {}", e),
            Error::MissingDataFile(s) => write!(f, "Missing data file: {}", s),
            Error::PassTimeout { pass, timeout } => {
                write!(f, "Pass {} timeout after {} seconds", pass, timeout)
            }
            Error::NoRouteFound(i) => write!(f, "No route found: {}", i),
        }
    }
}

impl Point {
    /// Create a new point with the given latitude and longitude
    pub fn new(lat: f64, lon: f64) -> Self {
        Point { lat, lon }
    }

    /// Return the latitude of the point
    pub fn lat(&self) -> f64 {
        self.lat
    }

    /// Return the longitude of the point
    pub fn lon(&self) -> f64 {
        self.lon
    }
}

/// A client for the BRouter server
pub struct Brouter {
    client: Client,
    base_url: Url,
    #[cfg(feature = "local")]
    server: Option<local::BRouterServer>,
}

impl Drop for Brouter {
    fn drop(&mut self) {
        #[cfg(feature = "local")]
        if let Some(server) = &mut self.server {
            server.stop().unwrap_or_else(|e| {
                log::error!("Failed to stop BRouter server: {}", e);
            });
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
/// The mode for turn instructions
pub enum TurnInstructionMode {
    #[default]
    /// No turn instructions
    None = 0,

    /// Turn instructions with auto choosing
    AutoChoose = 1,

    /// Use locus style turn instructions
    LocusStyle = 2,

    /// Use osmand style turn instructions
    OsmandStyle = 3,

    /// Use comment style turn instructions
    CommentStyle = 4,

    /// Use gpx style turn instructions
    GpsiesStyle = 5,

    /// Use orux style turn instructions
    OruxStyle = 6,

    /// Use old style locus turn instructions
    LocusOldStyle = 7,
}

impl Brouter {
    /// Create a new BRouter client with the given base URL
    pub fn new(base_url: &str) -> Self {
        Brouter {
            client: Client::new(),
            base_url: Url::parse(base_url).unwrap(),
            #[cfg(feature = "local")]
            server: None,
        }
    }

    #[cfg(feature = "local")]
    /// Run the BRouter server locally and connect to it
    pub fn local() -> Result<Self, Box<dyn std::error::Error>> {
        let mut server = local::BRouterServer::home();
        server.download_brouter()?;
        let url = server.start()?;
        Ok(Self {
            client: Client::new(),
            base_url: Url::parse(&url).unwrap(),
            server: Some(server),
        })
    }

    /// Upload a profile to the BRouter server
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

    /// Route between the given points
    ///
    /// # Arguments
    /// * `points` - A list of points to route between
    /// * `nogos` - A list of nogos to avoid
    /// * `profile` - The profile to use for routing
    /// * `alternativeidx` - The index of the alternative route to use
    /// * `timode` - The mode for turn instructions
    /// * `name` - The name of the route
    /// * `export_waypoints` - Whether to export waypoints
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

        let status = response.status();

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

        if let Some(m) =
            regex!("pass([0-9]) timeout after ([0-9]+) seconds\n"B).captures(text.as_slice())
        {
            let pass = String::from_utf8_lossy(m.get(1).unwrap().as_bytes())
                .to_string()
                .parse()
                .unwrap();

            let timeout = String::from_utf8_lossy(m.get(2).unwrap().as_bytes())
                .to_string()
                .parse()
                .unwrap();
            return Err(Error::PassTimeout { pass, timeout });
        }

        if status == reqwest::StatusCode::BAD_REQUEST {
            return Err(Error::Other(format!("HTTP error: {}", status)));
        }

        let gpx: gpx::Gpx = gpx::read(BufReader::new(text.as_slice())).map_err(|_e| {
            Error::InvalidGpx(String::from_utf8_lossy(text.as_slice()).to_string())
        })?;

        Ok(gpx)
    }
}
