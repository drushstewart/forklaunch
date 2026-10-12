fn main() {
    println!(
        "{}",
        forklaunch_iam_better_auth::http::application::registry("iam", "1.0")
            .into_artifact()
            .to_json()
    );
}
