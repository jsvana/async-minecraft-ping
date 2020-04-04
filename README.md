# async-minecraft-ping

![crates.io](https://img.shields.io/crates/v/async-minecraft-ping)
![docs.rs](https://docs.rs/async-minecraft-ping/badge.svg?version=0.1.0)
![crates.io](https://img.shields.io/crates/l/async-minecraft-ping/0.1.0)

An async [ServerListPing](https://wiki.vg/Server_List_Ping) client implementation in Rust.

## Usage

See [the example](./examples/status.rs).

```rust
let mut server = Server::build("some.cool.server".to_string());

let status = server.status().await?;

println!(
    "{} of {} player(s) online",
    status.players.online, status.players.max
);
```

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
