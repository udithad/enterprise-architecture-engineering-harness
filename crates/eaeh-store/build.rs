// Re-embed migrations whenever one is added or changed (sqlx::migrate!).
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
