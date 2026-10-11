fn main() {
    println!(
        "{}",
        forklaunch_billing_interface::module_contracts::registry(
            std::env::args().any(|arg| arg == "--stripe")
        )
        .into_artifact()
        .to_json()
    );
}
