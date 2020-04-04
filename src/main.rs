mod minecraft;

use anyhow::Result;

use minecraft::Server;

#[tokio::main]
async fn main() -> Result<()> {
    let mut server = Server::build("minecraft.elwert.cloud".to_string());

    let status = server.status().await?;

    println!(
        "{} of {} player(s) online",
        status.players.online, status.players.max
    );

    Ok(())
}
