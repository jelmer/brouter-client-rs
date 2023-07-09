use lazy_regex::regex;
use log::info;
use reqwest::blocking::Client;
use reqwest::Url;
use std::io::BufReader;

// See https://github.com/abrensch/brouter/blob/77977677db5fe78593c6a55afec6a251e69b3449/brouter-server/src/main/java/btools/server/request/ServerHandler.java#L17

#[derive(Debug, Clone)]
pub enum Nogo {
    Point(Point, f64),
    Line(Vec<Point>),
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
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidGpx(s) => write!(f, "Invalid GPX: {}", s),
            Error::Http(e) => write!(f, "HTTP error: {}", e),
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
        name: Option<&str>,
    ) -> Result<gpx::Gpx, Error> {
        let lon_lat_strings: Vec<String> = points
            .iter()
            .map(|p| format!("{},{}", p.lon(), p.lat()))
            .collect();

        info!("Planning route along {:?}", points);

        let lonlats = lon_lat_strings.join("%7C");

        let nogos_string: String = nogos
            .iter()
            .filter_map(|nogo| match nogo {
                Nogo::Point(p, radius) => Some(format!("{},{},{}", p.lon(), p.lat(), radius)),
                Nogo::Line(_) => None,
            })
            .collect::<Vec<_>>()
            .join("%7C");

        let polylines = nogos
            .iter()
            .filter_map(|nogo| match nogo {
                Nogo::Point(_, _) => None,
                Nogo::Line(points) => {
                    let lat_lon_strings: Vec<String> = points
                        .iter()
                        .map(|p| format!("{},{}", p.lon(), p.lat()))
                        .collect();
                    Some(lat_lon_strings.join(","))
                }
            })
            .collect::<Vec<_>>()
            .join("%7C");

        let alternativeidx = alternativeidx.unwrap_or(0);

        assert!((0..=3).contains(&alternativeidx));

        let mut url = self.base_url.join("brouter").unwrap();

        url.query_pairs_mut()
            .append_pair("lonlats", &lonlats)
            .append_pair("profile", profile)
            .append_pair("alternativeidx", &alternativeidx.to_string())
            .append_pair("format", "gpx")
            .append_pair("timode", "3")
            .append_pair("nogos", &nogos_string)
            .append_pair("polylines", &polylines);

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
            panic!(
                "datafile {} not found",
                String::from_utf8_lossy(m.get(1).unwrap().as_bytes())
            );
        }

        let gpx: gpx::Gpx = gpx::read(BufReader::new(text.as_slice())).map_err(|_e| {
            Error::InvalidGpx(String::from_utf8_lossy(text.as_slice()).to_string())
        })?;

        Ok(gpx)
    }
}
