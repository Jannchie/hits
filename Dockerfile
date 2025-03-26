# Use the latest stable Rust image as the base image for building the application
FROM rust:1.85 AS builder

# Set the working directory inside the container
WORKDIR /usr/src/hits

# Install sqlx-cli with minimal features
RUN cargo install sqlx-cli --no-default-features --features postgres

# Copy the Cargo.toml and Cargo.lock files to leverage Docker's caching mechanism
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build the dependencies
RUN cargo build --release && rm -f target/release/deps/hits*

# Copy the source code
COPY . .

# Build the application
RUN cargo build --release

# Use a smaller base image for the final stage
FROM debian:bookworm-slim AS runtime 


# Set the working directory inside the container
WORKDIR /usr/src/hits

# Install OpenSSL 3 and any other necessary libraries
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    libssl3 libpq5 && \
    rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/hits/target/release/hits .

# Copy sqlx-cli from the builder stage
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx

# Copy the migrations directory
COPY migrations ./migrations

# Expose the port the application runs on
EXPOSE 3030

# Run the application
CMD ["./hits"]