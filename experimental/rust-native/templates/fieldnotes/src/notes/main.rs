#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fieldnotes_notes::bootstrapper::run().await
}
