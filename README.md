# Hits

![Endpoint Badge](https://hits.jannchie.com/svg/jannchie.github.hits.doc?style=social)

![Uptime Status](https://uptime.jannchie.com/api/badge/3/status)
![Uptime Uptime](https://uptime.jannchie.com/api/badge/3/uptime)
![Uptime Average Response](https://uptime.jannchie.com/api/badge/3/avg-response)

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
