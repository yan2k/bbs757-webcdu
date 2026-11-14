fn main() {
    println!("cargo:rerun-if-changed=vendor/SimConnect.dll");
    println!("cargo:rerun-if-changed=web/index.html");
    if !std::path::Path::new("vendor/SimConnect.dll").is_file() {
        panic!("vendor/SimConnect.dll is missing - run scripts/fetch-simconnect.sh first");
    }
}
