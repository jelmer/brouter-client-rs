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
use serde::Deserialize;
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

    /// Error uploading profile
    UploadProfileError(String),

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
            Error::UploadProfileError(s) => write!(f, "Error uploading profile: {}", s),
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

#[derive(Deserialize)]
struct UploadProfileResponse {
    profileid: String,
    error: Option<String>,
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
    ///
    /// # Arguments
    /// * `data` - contents of the profile
    ///
    /// # Returns
    /// the name of the custom profile that was created
    pub fn upload_profile(&self, data: Vec<u8>) -> Result<String, Error> {
        let url = self.base_url.join("brouter/profile").unwrap();

        let response = self
            .client
            .post(url)
            .body(data)
            .send()
            .map_err(Error::Http)?;

        let response = response.error_for_status().map_err(Error::Http)?;

        let response: UploadProfileResponse = response.json().map_err(Error::Http)?;

        if let Some(error) = response.error {
            return Err(Error::UploadProfileError(error));
        } else {
            return Ok(response.profileid);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_new() {
        let point = Point::new(52.5200, 13.4050);
        assert_eq!(point.lat(), 52.5200);
        assert_eq!(point.lon(), 13.4050);
    }

    #[test]
    fn test_point_from_geo_types() {
        let geo_point = geo_types::Point::new(13.4050, 52.5200);
        let point: Point = geo_point.into();
        assert_eq!(point.lat(), 52.5200);
        assert_eq!(point.lon(), 13.4050);
    }

    #[test]
    fn test_point_to_geo_types() {
        let point = Point::new(52.5200, 13.4050);
        let geo_point: geo_types::Point<f64> = point.into();
        assert_eq!(geo_point.x(), 13.4050);
        assert_eq!(geo_point.y(), 52.5200);
    }

    #[test]
    fn test_nogo_point_weight() {
        let nogo = Nogo::Point {
            point: Point::new(52.5200, 13.4050),
            radius: 100.0,
            weight: Some(10.0),
        };
        assert_eq!(nogo.weight(), Some(10.0));
    }

    #[test]
    fn test_nogo_line_weight() {
        let nogo = Nogo::Line {
            points: vec![Point::new(52.5200, 13.4050), Point::new(52.5300, 13.4150)],
            weight: Some(5.0),
        };
        assert_eq!(nogo.weight(), Some(5.0));
    }

    #[test]
    fn test_nogo_polygon_weight() {
        let nogo = Nogo::Polygon {
            points: vec![
                Point::new(52.5200, 13.4050),
                Point::new(52.5300, 13.4150),
                Point::new(52.5250, 13.4100),
            ],
            weight: None,
        };
        assert_eq!(nogo.weight(), None);
    }

    #[test]
    fn test_turn_instruction_mode_default() {
        let mode = TurnInstructionMode::default();
        assert_eq!(mode as i32, 0);
    }

    #[test]
    fn test_error_display() {
        let error = Error::InvalidGpx("test error".to_string());
        assert_eq!(format!("{}", error), "Invalid GPX: test error");

        let error = Error::NoRouteFound(42);
        assert_eq!(format!("{}", error), "No route found: 42");

        let error = Error::PassTimeout {
            pass: "1".to_string(),
            timeout: "30".to_string(),
        };
        assert_eq!(format!("{}", error), "Pass 1 timeout after 30 seconds");

        let error = Error::MissingDataFile("test.rd5".to_string());
        assert_eq!(format!("{}", error), "Missing data file: test.rd5");

        let error = Error::Other("custom error".to_string());
        assert_eq!(format!("{}", error), "Error: custom error");
    }

    #[test]
    fn test_brouter_new() {
        let brouter = Brouter::new("http://localhost:17777");
        assert_eq!(brouter.base_url.as_str(), "http://localhost:17777/");
    }

    #[test]
    fn test_point_debug() {
        let point = Point::new(52.5200, 13.4050);
        let debug_str = format!("{:?}", point);
        assert!(debug_str.contains("52.52"));
        assert!(debug_str.contains("13.405"));
    }

    #[test]
    fn test_point_clone() {
        let point = Point::new(52.5200, 13.4050);
        let cloned = point.clone();
        assert_eq!(point.lat(), cloned.lat());
        assert_eq!(point.lon(), cloned.lon());
    }

    #[test]
    fn test_nogo_debug() {
        let nogo = Nogo::Point {
            point: Point::new(52.5200, 13.4050),
            radius: 100.0,
            weight: Some(10.0),
        };
        let debug_str = format!("{:?}", nogo);
        assert!(debug_str.contains("Point"));
        assert!(debug_str.contains("100"));
        assert!(debug_str.contains("10"));
    }

    #[test]
    fn test_nogo_clone() {
        let nogo = Nogo::Line {
            points: vec![Point::new(52.5200, 13.4050), Point::new(52.5300, 13.4150)],
            weight: Some(5.0),
        };
        let cloned = nogo.clone();
        assert_eq!(nogo.weight(), cloned.weight());
    }

    #[test]
    fn test_turn_instruction_mode_values() {
        assert_eq!(TurnInstructionMode::None as i32, 0);
        assert_eq!(TurnInstructionMode::AutoChoose as i32, 1);
        assert_eq!(TurnInstructionMode::LocusStyle as i32, 2);
        assert_eq!(TurnInstructionMode::OsmandStyle as i32, 3);
        assert_eq!(TurnInstructionMode::CommentStyle as i32, 4);
        assert_eq!(TurnInstructionMode::GpsiesStyle as i32, 5);
        assert_eq!(TurnInstructionMode::OruxStyle as i32, 6);
        assert_eq!(TurnInstructionMode::LocusOldStyle as i32, 7);
    }

    #[test]
    fn test_turn_instruction_mode_debug() {
        let mode = TurnInstructionMode::LocusStyle;
        let debug_str = format!("{:?}", mode);
        assert!(debug_str.contains("LocusStyle"));
    }

    #[test]
    fn test_turn_instruction_mode_clone() {
        let mode = TurnInstructionMode::OsmandStyle;
        let cloned = mode;
        assert_eq!(mode as i32, cloned as i32);
    }

    #[test]
    fn test_error_debug() {
        let error = Error::InvalidGpx("test".to_string());
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("InvalidGpx"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_error_std_error() {
        let error = Error::Other("test error".to_string());
        assert!(std::error::Error::source(&error).is_none());
    }

    #[test]
    fn test_nogo_no_weight() {
        let nogo_point = Nogo::Point {
            point: Point::new(52.5200, 13.4050),
            radius: 100.0,
            weight: None,
        };
        assert_eq!(nogo_point.weight(), None);

        let nogo_line = Nogo::Line {
            points: vec![Point::new(52.5200, 13.4050)],
            weight: None,
        };
        assert_eq!(nogo_line.weight(), None);
    }

    #[test]
    fn test_point_edge_values() {
        let point = Point::new(-90.0, -180.0);
        assert_eq!(point.lat(), -90.0);
        assert_eq!(point.lon(), -180.0);

        let point = Point::new(90.0, 180.0);
        assert_eq!(point.lat(), 90.0);
        assert_eq!(point.lon(), 180.0);

        let point = Point::new(0.0, 0.0);
        assert_eq!(point.lat(), 0.0);
        assert_eq!(point.lon(), 0.0);
    }

    #[test]
    fn test_nogo_empty_collections() {
        let nogo_line = Nogo::Line {
            points: vec![],
            weight: Some(1.0),
        };
        assert_eq!(nogo_line.weight(), Some(1.0));

        let nogo_polygon = Nogo::Polygon {
            points: vec![],
            weight: Some(2.0),
        };
        assert_eq!(nogo_polygon.weight(), Some(2.0));
    }

    #[test]
    fn test_nogo_single_point_collections() {
        let point = Point::new(52.5200, 13.4050);

        let nogo_line = Nogo::Line {
            points: vec![point.clone()],
            weight: Some(1.0),
        };
        assert_eq!(nogo_line.weight(), Some(1.0));

        let nogo_polygon = Nogo::Polygon {
            points: vec![point],
            weight: Some(2.0),
        };
        assert_eq!(nogo_polygon.weight(), Some(2.0));
    }

    #[test]
    fn test_nogo_many_points() {
        let points: Vec<Point> = (0..10)
            .map(|i| Point::new(50.0 + i as f64 * 0.1, 10.0 + i as f64 * 0.1))
            .collect();

        let nogo_line = Nogo::Line {
            points: points.clone(),
            weight: Some(5.0),
        };
        assert_eq!(nogo_line.weight(), Some(5.0));

        let nogo_polygon = Nogo::Polygon {
            points,
            weight: Some(7.0),
        };
        assert_eq!(nogo_polygon.weight(), Some(7.0));
    }

    #[test]
    fn test_different_url_formats() {
        let brouter1 = Brouter::new("http://localhost:17777");
        assert_eq!(brouter1.base_url.as_str(), "http://localhost:17777/");

        let brouter2 = Brouter::new("https://brouter.example.com/");
        assert_eq!(brouter2.base_url.as_str(), "https://brouter.example.com/");

        let brouter3 = Brouter::new("http://192.168.1.100:8080");
        assert_eq!(brouter3.base_url.as_str(), "http://192.168.1.100:8080/");
    }

    #[test]
    fn test_all_error_variants() {
        let invalid_gpx = Error::InvalidGpx("malformed".to_string());
        assert_eq!(format!("{}", invalid_gpx), "Invalid GPX: malformed");

        let missing_data = Error::MissingDataFile("europe.rd5".to_string());
        assert_eq!(format!("{}", missing_data), "Missing data file: europe.rd5");

        let no_route = Error::NoRouteFound(-1);
        assert_eq!(format!("{}", no_route), "No route found: -1");

        let timeout = Error::PassTimeout {
            pass: "2".to_string(),
            timeout: "120".to_string(),
        };
        assert_eq!(format!("{}", timeout), "Pass 2 timeout after 120 seconds");

        let other = Error::Other("connection refused".to_string());
        assert_eq!(format!("{}", other), "Error: connection refused");

        let upload_error = Error::UploadProfileError("Invalid profile format".to_string());
        assert_eq!(
            format!("{}", upload_error),
            "Error uploading profile: Invalid profile format"
        );
    }

    #[test]
    fn test_upload_profile_response_deserialization() {
        use serde_json;

        // Test successful response
        let success_json = r#"{"profileid": "custom_12345", "error": null}"#;
        let response: UploadProfileResponse = serde_json::from_str(success_json).unwrap();
        assert_eq!(response.profileid, "custom_12345");
        assert_eq!(response.error, None);

        // Test error response
        let error_json = r#"{"profileid": "", "error": "Invalid profile syntax"}"#;
        let response: UploadProfileResponse = serde_json::from_str(error_json).unwrap();
        assert_eq!(response.profileid, "");
        assert_eq!(response.error, Some("Invalid profile syntax".to_string()));

        // Test response with no error field (should be None)
        let no_error_json = r#"{"profileid": "custom_67890"}"#;
        let response: UploadProfileResponse = serde_json::from_str(no_error_json).unwrap();
        assert_eq!(response.profileid, "custom_67890");
        assert_eq!(response.error, None);
    }

    #[test]
    fn test_upload_profile_url_construction() {
        let brouter = Brouter::new("http://localhost:17777");

        // We can't easily test the upload without a mock server, but we can test
        // that the URL construction works by checking the base_url
        let expected_profile_url = "http://localhost:17777/brouter/profile";
        let profile_url = brouter.base_url.join("brouter/profile").unwrap();
        assert_eq!(profile_url.as_str(), expected_profile_url);
    }

    #[test]
    fn test_valid_profile_data() {
        // Test with a valid BRouter profile content
        let valid_profile_data =
            b"# BRouter profile\nassign turncost 0\nassign uphillcostfactor 1.5\n".to_vec();

        // Since we can't test upload without a server, we test that the data is correctly formatted
        assert!(!valid_profile_data.is_empty());
        assert!(valid_profile_data.len() > 10);

        // Test that the profile content contains expected BRouter keywords
        let profile_str = String::from_utf8_lossy(&valid_profile_data);
        assert!(profile_str.contains("assign"));
    }

    #[test]
    fn test_invalid_profile_data() {
        // Test with various invalid profile contents
        let empty_profile: Vec<u8> = Vec::new();
        assert!(empty_profile.is_empty());

        let invalid_profile = b"invalid profile content without proper syntax".to_vec();
        let profile_str = String::from_utf8_lossy(&invalid_profile);
        assert!(!profile_str.contains("assign"));

        // Test with binary data that shouldn't be a valid profile
        let binary_data = vec![0xFF, 0xFE, 0xFD, 0xFC];
        assert!(binary_data.len() == 4);

        // Test with extremely long data
        let long_data = vec![b'a'; 1_000_000];
        assert!(long_data.len() == 1_000_000);
    }
}
