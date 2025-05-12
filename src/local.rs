//! This module contains the code to download and run BRouter locally.
use std::{
    fs::{self, File},
    io::{self, Cursor},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use reqwest::blocking::get;
use zip::ZipArchive;

/// BRouter version to use
const BROUTER_VERSION: &str = "1.7.7";

/// URL to the BRouter server package ZIP
const BROUTER_URL: &str = "https://github.com/abrensch/brouter/releases/download";

/// A struct representing the BRouter server
pub struct BRouterServer {
    /// Base path where BRouter is installed
    pub base_path: PathBuf,
    segments_dir: PathBuf,
    custom_profile_dir: PathBuf,
    process: Option<std::process::Child>,
}

impl BRouterServer {
    /// Create a new BRouterServer instance
    pub fn new(brouter_dir: &Path) -> Self {
        let segments_dir = brouter_dir.join("segments4");
        let custom_profile_dir = brouter_dir.join("custom_profiles");

        BRouterServer {
            base_path: brouter_dir.to_path_buf(),
            segments_dir,
            custom_profile_dir,
            process: None,
        }
    }

    /// Create a new BRouterServer instance in the user's home directory
    pub fn home() -> Self {
        let data_dir = xdg::BaseDirectories::new()
            .get_data_home()
            .unwrap()
            .join("brouter");

        std::fs::create_dir_all(&data_dir).unwrap();

        Self::new(&data_dir)
    }

    fn find_jar_file(&self) -> Option<PathBuf> {
        for entry in fs::read_dir(&self.base_path).unwrap() {
            let entry = entry.unwrap();

            if entry.file_name().to_str().unwrap().starts_with("brouter-") {
                for sub_entry in fs::read_dir(entry.path()).unwrap() {
                    let sub_entry = sub_entry.unwrap();
                    if sub_entry.file_name().to_str().unwrap().ends_with(".jar") {
                        return Some(sub_entry.path());
                    }
                }
            }
        }
        None
    }

    /// Check if the BRouter server has been downloaded
    pub fn has_downloaded(&self) -> bool {
        self.find_jar_file().is_some()
    }

    /// Download and extract the BRouter server
    pub fn download_brouter(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Check if the BRouter server is already downloaded
        if self.find_jar_file().is_some() {
            return Ok(());
        }

        let resp = get(format!(
            "{}/v{}/brouter-{}.zip",
            BROUTER_URL, BROUTER_VERSION, BROUTER_VERSION
        ))?;

        if resp.status() != reqwest::StatusCode::OK {
            return Err(
                format!("Failed to download BRouter server: {}", resp.status()).into(),
            );
        }

        let bytes = resp.bytes()?;

        let cursor = Cursor::new(bytes);

        let mut archive = ZipArchive::new(cursor)?;
        archive.extract(&self.base_path)?;

        Ok(())
    }

    /// Download all segments
    pub fn download_all_segments(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Check if the segments directory exists
        if !self.segments_dir.exists() {
            fs::create_dir_all(&self.segments_dir)?;
        }

        for e in (0..=175).step_by(5) {
            for n in (0..=90).step_by(5) {
                let segment = format!("E{}_N{}", e, n);
                self.download_segment(&segment)?;
            }

            for n in (0..=90).step_by(5) {
                let segment = format!("E{}_S{}", e, n);
                self.download_segment(&segment)?;
            }
        }

        for w in (0..=175).step_by(5) {
            for n in (0..=90).step_by(5) {
                let segment = format!("W{}_N{}", w, n);
                self.download_segment(&segment)?;
            }

            for n in (0..=90).step_by(5) {
                let segment = format!("W{}_S{}", w, n);
                self.download_segment(&segment)?;
            }
        }

        Ok(())
    }

    /// Download a specific segment
    pub fn download_segment(&self, segment: &str) -> Result<(), Box<dyn std::error::Error>> {

        // Check if the segments directory exists
        if !self.segments_dir.exists() {
            fs::create_dir_all(&self.segments_dir)?;
        }

        let segment_path = self.segments_dir.join(format!("{}.rd5", segment));

        // Check if the segment is already downloaded
        if segment_path.exists() {
            return Ok(());
        }

        // Create the segments directory if it doesn't exist
        fs::create_dir_all(&self.segments_dir)?;

        // Download the segment
        let mut resp = get(format!("https://brouter.de/brouter/segments4/{}.rd5", segment))?;
        if resp.status() != reqwest::StatusCode::OK {
            return Err(
                format!("Failed to download segment {}: {}", segment, resp.status()).into(),
            );
        }
        let mut out = File::create(&segment_path)?;
        io::copy(&mut resp, &mut out)?;

        Ok(())
    }

    /// Check if the BRouter server is running
    pub fn is_running(&mut self) -> bool {
        if let Some(process) = self.process.as_mut() {
            match process.try_wait() {
                Ok(Some(_)) => false, // Process has exited
                Ok(None) => true,    // Process is still running
                Err(_) => false,     // Error checking process status
            }
        } else {
            false // No process started
        }
    }

    /// Check if the BRouter server is serving requests
    pub fn is_serving(&self) -> bool {
        // Check if the server is running and responding on the port
        match get("http://localhost:17777") {
            // The root URL should return a 404 Not Found
            Ok(resp) => resp.status() == reqwest::StatusCode::NOT_FOUND,
            Err(_) => false,
        }
    }

    /// Start the BRouter server
    pub fn start(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        // Check if the BRouter server is already running
        if self.is_running() {
            return Ok(format!("http://localhost:17777"));
        }

        let jar_path = self
            .find_jar_file()
            .ok_or("BRouter server JAR file not found")?;

        let profile_dir = jar_path.parent().unwrap().join("profiles2");

        // Ensure the custom profile directory exists
        fs::create_dir_all(&self.custom_profile_dir)?;

        // Start the BRouter server
        let child = Command::new("java")
            .current_dir(&self.base_path)
            .arg("-Xmx128M")
            .arg("-Xms128M")
            .arg("-Xmn8M")
            .arg("-DmaxRunningTime=300") // Request timeout in seconds (0 for no timeout)
            .arg("-DuseRFCMimeType=false")
            .arg("-cp")
            .arg(jar_path)
            .arg("btools.server.RouteServer")
            .arg(self.segments_dir.to_str().unwrap())
            .arg(profile_dir.to_str().unwrap())
            .arg(self.custom_profile_dir.to_str().unwrap())
            .arg("17777") // Port
            .arg("1") // Number of threads
            .arg("localhost") // Host
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        self.process = Some(child);

        // Wait until the server is up and responding on the port
        let mut attempts = 0;
        while attempts < 10 {
            if self.is_serving() {
                break;
            }
            attempts += 1;
            thread::sleep(Duration::from_secs(1));
        }

        Ok(format!("http://localhost:17777"))
    }

    /// Stop the BRouter server
    pub fn stop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(mut process) = self.process.take() {
            process.kill()?;
        }
        Ok(())
    }
}

impl Drop for BRouterServer {
    fn drop(&mut self) {
        self.stop().unwrap_or_else(|_| {
            eprintln!("Failed to stop BRouter server: {}", self.base_path.display());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brouter_server() {
        let mut brouter = BRouterServer::home();
        brouter.download_brouter().unwrap();
        assert!(brouter.has_downloaded());
        brouter.download_segment("E0_N10").unwrap();
        assert!(brouter.segments_dir.join("E0_N10.rd5").exists());
        brouter.start().unwrap();
        assert!(brouter.is_running());
        brouter.stop().unwrap();
    }
}
