# Hits

![Endpoint Badge](https://img.shields.io/endpoint?url=https%3A%2F%2Fhits.jannchie.com%2Fbadge%2Fjannchie.hits)

A Simple Rust Web Server Application that counts the number of hits on any keys.

Based on Axum, SQLx, and Postgres.

## Usage

### Docker

```bash
docker compose up --build
```

### Manual

```bash
# install sqlx-cli
cargo install sqlx-cli --no-default-features --features postgres

# create the database and run the migrations
sqlx database create
sqlx migrate run

# run the server
cargo run
```

## Build Docker Image

```bash
docker build -t jannchie/hits .
```
