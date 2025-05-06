fn main() {
    let mut brouter = brouter_client::local::BRouterServer::home();

    println!("Starting BRouter server at {}", brouter.base_path.display());

    brouter.download_brouter().unwrap();

    let url = brouter.start().unwrap();

    println!("BRouter server started at {}", url);

    loop {}
}
