//! meme-arr binary entry point. All logic lives in the library crate
//! (`src/lib.rs`) so integration tests can reach the same modules.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let logs = meme_arr::logs::init();
    meme_arr::run(logs).await
}
